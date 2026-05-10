# Integration with nexus-rt

nexus-rt is a **dispatch framework** — Handlers, Pipelines, DAGs, Templates,
Reactors. It has no executor: you drive it from a poll loop.

nexus-async-rt is an **async executor** — single-threaded, slab-backed,
mio-driven. It provides a poll loop that can host nexus-rt's world.

Together they form a complete runtime: async tasks drive IO and drive
nexus-rt handlers, all on one thread.

## The Big Picture

```text
+--------------------------------------------------------+
|  Runtime::block_on()                                   |  <- nexus-async-rt
|                                                        |
|    async tasks   (sockets, timers)                     |
|        |                                               |
|        | WorldCtx::current().with_world(|w| handler.run()) |
|        v                                               |
|    World / Resources  (nexus-rt)                       |
+--------------------------------------------------------+
```

The `World` is owned by the `Runtime`. Async tasks access it through
[`WorldCtx`] — a `Copy` handle wrapping the World pointer — whenever
they need to update state or dispatch a handler.

## `WorldCtx` and `WorldCtx::current()`

```rust
pub struct WorldCtx { /* Copy */ }

impl WorldCtx {
    /// Construct from a mutable World reference (use before `block_on`).
    pub fn new(world: &mut World) -> Self;

    /// Fetch the runtime's current WorldCtx from TLS (use inside tasks).
    pub fn current() -> Self;

    pub fn with_world<R>(&self, f: impl FnOnce(&mut World) -> R) -> R;
    pub fn with_world_ref<R>(&self, f: impl FnOnce(&World) -> R) -> R;
}
```

The `Runtime` installs the World pointer in TLS during `block_on`.
`WorldCtx::current()` reads that pointer; outside `block_on` it panics.
`with_world` / `with_world_ref` are inherent methods that run a closure
synchronously inline against the borrowed World — no await point.

Two ways to obtain a `WorldCtx`:

| When | Method | Why |
|---|---|---|
| Inside a task, occasional access | `WorldCtx::current()` | One TLS read per call site; fine for cold paths. |
| Inside a task, hot loop | `let ctx = WorldCtx::current();` once, then reuse | `WorldCtx` is `Copy` — capture saves repeated TLS reads. |
| Before `block_on`, capturing into closures | `WorldCtx::new(&mut world)` | No runtime context to read from yet. |

```rust
use nexus_async_rt::{Runtime, WorldCtx, spawn_boxed};
use nexus_rt::{Resource, WorldBuilder};

#[derive(Resource, Default)]
struct Counter(u64);

fn main() {
    let mut world = WorldBuilder::new()
        .with_resource(Counter::default())
        .build();
    let mut rt = Runtime::new(&mut world);

    rt.block_on(async {
        spawn_boxed(async {
            // Mutable access — scoped to the closure.
            WorldCtx::current().with_world(|w| {
                w.resource_mut::<Counter>().0 += 1;
            });

            // Read-only access.
            let n = WorldCtx::current().with_world_ref(|w| w.resource::<Counter>().0);
            assert_eq!(n, 1);
        })
        .await;
    });
}
```

**Rules:**

- Do not call `with_world` recursively from inside another `with_world`
  closure — it will panic (World is already borrowed).
- Do not hold references across `.await`. The borrow must end before any
  yield point.
- Handlers, Pipelines, and Reactors are `!Send` — that's fine, the runtime
  is single-threaded.

## Pre-resolving Handlers at Setup Time

The fastest dispatch pattern: resolve Handler parameters **once** at setup,
then dispatch repeatedly from async tasks without re-resolution.

```rust
use nexus_async_rt::{Runtime, WorldCtx, spawn_boxed};
use nexus_rt::{IntoHandler, Handler, Res, ResMut, Resource, WorldBuilder};

#[derive(Resource, Default)]
struct OrderBook { /* ... */ }

#[derive(Resource, Default)]
struct Metrics { updates: u64 }

struct Tick { price: f64, qty: f64 }

fn on_tick(mut book: ResMut<OrderBook>, mut m: ResMut<Metrics>, tick: Tick) {
    // Update the book.
    let _ = (&mut *book, tick);
    m.updates += 1;
}

fn main() {
    let mut world = WorldBuilder::new()
        .with_resource(OrderBook::default())
        .with_resource(Metrics::default())
        .build();

    // Resolve the handler ONCE — IDs are cached inside.
    let mut handler = on_tick.into_handler(world.registry());

    let mut rt = Runtime::new(&mut world);
    rt.block_on(async move {
        spawn_boxed(async move {
            // Cache the WorldCtx outside the loop — saves one TLS read per tick.
            let ctx = WorldCtx::current();
            let tick = recv_tick().await;

            // Dispatch is one World borrow + the handler's pre-resolved fetch.
            ctx.with_world(|w| handler.run(w, tick));
        })
        .await;
    });
}

async fn recv_tick() -> Tick { Tick { price: 0.0, qty: 0.0 } }
```

This is the canonical pattern for market data loops: the async task owns
the socket and the parsing state, and every parsed message goes through a
pre-resolved handler. Dispatch cost is one `with_world` borrow plus the
handler's parameter fetch (~1 cycle per `Res`/`ResMut`).

## Capturing `WorldCtx` Across Many Tasks

`WorldCtx` is `Copy`, so a single handle captures cleanly into as many
closures as you like with no reference-counting cost. This is useful when
you want to spawn a fan-out of tasks that all share World access.

```rust
use nexus_async_rt::{Runtime, WorldCtx, spawn_boxed};
use nexus_rt::{Resource, WorldBuilder};

#[derive(Resource, Default)]
struct Stats { msgs: u64 }

fn main() {
    let mut world = WorldBuilder::new()
        .with_resource(Stats::default())
        .build();

    let mut rt = Runtime::new(&mut world);
    rt.block_on(async {
        // One TLS read; reuse across all spawned tasks below.
        let ctx = WorldCtx::current();

        for _ in 0..4 {
            spawn_boxed(async move {
                ctx.with_world(|w| {
                    w.resource_mut::<Stats>().msgs += 1;
                });
            });
        }
    });
}
```

`WorldCtx::new(&mut world)` is the alternative for the rarer case where
you need to construct the handle *before* `block_on` is running (e.g.,
moving it into a task that you'll spawn from inside `block_on`). Inside a
task, prefer `current()`.

## Driving a Pipeline From an Async Task

Pipelines are just `Handler`s — the same pattern works.

```rust
use nexus_async_rt::{Runtime, WorldCtx, spawn_boxed};
use nexus_rt::{Handler, Pipeline, Res, ResMut, Resource, WorldBuilder};

#[derive(Resource, Default)]
struct Book { bids: u64, asks: u64 }

fn validate(tick: u64) -> Option<u64> {
    if tick > 0 { Some(tick) } else { None }
}

fn apply(mut book: ResMut<Book>, tick: u64) {
    book.bids += tick;
}

fn main() {
    let mut world = WorldBuilder::new()
        .with_resource(Book::default())
        .build();

    let reg = world.registry();
    let mut pipeline = Pipeline::<u64>::new()
        .filter_map(validate, &reg)
        .then(apply, &reg)
        .build();

    let mut rt = Runtime::new(&mut world);
    rt.block_on(async move {
        spawn_boxed(async move {
            let ctx = WorldCtx::current();
            for tick in 1..=10 {
                ctx.with_world(|w| pipeline.run(w, tick));
            }
        }).await;
    });
}
```

## Complete Example: WebSocket → OrderBook

Market data websocket task parses messages and updates a nexus-rt Resource.
A separate task subscribes via an `EventReader`-style reactor (out of scope
here — see nexus-rt `reactors.md`).

```rust
use nexus_async_rt::{
    Runtime, ShutdownSignal, TcpStream, WorldCtx, spawn_boxed,
};
use nexus_rt::{Handler, IntoHandler, ResMut, Resource, WorldBuilder};

#[derive(Resource, Default)]
struct OrderBook {
    best_bid: f64,
    best_ask: f64,
}

struct Quote { bid: f64, ask: f64 }

fn apply_quote(mut book: ResMut<OrderBook>, q: Quote) {
    book.best_bid = q.bid;
    book.best_ask = q.ask;
}

fn main() -> std::io::Result<()> {
    let mut world = WorldBuilder::new()
        .with_resource(OrderBook::default())
        .build();

    // Resolve once, dispatch many.
    let mut handler = apply_quote.into_handler(world.registry());

    let mut rt = Runtime::new(&mut world);
    rt.block_on(async move {
        spawn_boxed(async move {
            let ctx = WorldCtx::current();
            let mut stream = connect_ws().await?;
            loop {
                let q = read_quote(&mut stream).await?;
                ctx.with_world(|w| handler.run(w, q));
            }
            #[allow(unreachable_code)]
            Ok::<_, std::io::Error>(())
        });

        ShutdownSignal::current().await;
        Ok(())
    })
}

async fn connect_ws() -> std::io::Result<TcpStream> { todo!() }
async fn read_quote(_s: &mut TcpStream) -> std::io::Result<Quote> { todo!() }
```

## Anti-patterns

- **Holding a World reference across `await`:** the borrow checker will stop
  you at compile time. Good.
- **Recursive `with_world`:** runtime panic. Refactor the inner call to take
  `&mut World` as a parameter, or do the work after the outer borrow ends.
- **Spawning a task, then awaiting it from inside a `with_world` closure:**
  you cannot `await` inside the closure — the closure is synchronous. Spawn
  outside, await outside, call `with_world` at the boundaries.
- **Resolving handlers on every dispatch:** use `into_handler(registry)`
  once at setup. The cost is the HashMap lookups, which are zero per
  subsequent `run`.

## See Also

- [Task Spawning](task-spawning.md) — spawn strategies
- [nexus-rt handlers.md](../../nexus-rt/docs/handlers.md) — Handler traits
  and `IntoHandler`
- [nexus-rt world.md](../../nexus-rt/docs/world.md) — Resource model
- [Patterns](patterns.md) — end-to-end recipes
