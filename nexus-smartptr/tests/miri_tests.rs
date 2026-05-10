//! Miri tests for nexus-smartptr.
//!
//! Run: `MIRIFLAGS="-Zmiri-ignore-leaks" cargo +nightly miri test -p nexus-smartptr --test miri_tests`
//!
//! Exercises the unsafe pointer machinery (fat-pointer decomposition,
//! inline storage of `?Sized` values, heap fallback in `Flex`, drop
//! ordering) through the public API only. Each test is small and
//! focused so miri's stacked-borrows + provenance model has a clean
//! shot at the unsafe paths.

use std::cell::Cell;
use std::fmt::{self, Display};
use std::rc::Rc;

use nexus_smartptr::{B16, B32, Flat, Flex, flat, flex};

// =============================================================================
// Trait used across tests
// =============================================================================

trait Greet {
    fn greet(&self) -> String;
}

struct Hello;
impl Greet for Hello {
    fn greet(&self) -> String {
        "hello".into()
    }
}

struct Bye(u32);
impl Greet for Bye {
    fn greet(&self) -> String {
        format!("bye-{}", self.0)
    }
}

// =============================================================================
// Pass 1.1 — Flat with two different concretes (no data/vtable mixing)
// =============================================================================

#[test]
fn flat_two_concrete_impls_dont_mix_vtables() {
    let a: Flat<dyn Greet, B32> = flat!(Hello);
    let b: Flat<dyn Greet, B32> = flat!(Bye(7));

    assert_eq!(a.greet(), "hello");
    assert_eq!(b.greet(), "bye-7");

    // Drop ordering — drop a then b; if vtables had been crossed, miri
    // would surface either a UB read or a wrong destructor invocation.
    drop(a);
    drop(b);
}

// =============================================================================
// Pass 1.2 — Flex with two different concretes (inline path)
// =============================================================================

#[test]
fn flex_two_concrete_impls_dont_mix_vtables() {
    let a: Flex<dyn Greet, B32> = flex!(Hello);
    let b: Flex<dyn Greet, B32> = flex!(Bye(11));

    assert!(a.is_inline());
    assert!(b.is_inline());
    assert_eq!(a.greet(), "hello");
    assert_eq!(b.greet(), "bye-11");
}

// =============================================================================
// Pass 1.3 — Metadata round-trip via the public API
// =============================================================================
//
// `meta::{extract_metadata, make_ptr}` are pub(crate). Exercising the
// round-trip from outside the crate has to go through Flat/Flex
// construction + deref, which is the same code path. This test
// constructs via `flat!`, derefs many times to force re-reads of the
// stored metadata word, and checks the value each time.

#[test]
fn flat_metadata_word_survives_repeated_deref() {
    let f: Flat<dyn Display, B32> = flat!(123_u64);

    // Repeated deref re-reads the metadata each call (see `as_ptr`).
    // Miri will flag any provenance loss / type confusion.
    for _ in 0..16 {
        assert_eq!(format!("{}", &*f), "123");
    }
}

// =============================================================================
// Pass 1.4 — Drop ordering
// =============================================================================

struct DropTracker {
    counter: Rc<Cell<u32>>,
}

impl Drop for DropTracker {
    fn drop(&mut self) {
        self.counter.set(self.counter.get() + 1);
    }
}

#[test]
fn flat_runs_inner_drop() {
    let counter = Rc::new(Cell::new(0u32));
    {
        let _f: Flat<DropTracker, B32> = Flat::new(DropTracker {
            counter: counter.clone(),
        });
        assert_eq!(counter.get(), 0);
    }
    assert_eq!(counter.get(), 1, "inner Drop ran exactly once");
}

#[test]
fn flex_inline_runs_inner_drop() {
    let counter = Rc::new(Cell::new(0u32));
    {
        let f: Flex<DropTracker, B32> = Flex::new(DropTracker {
            counter: counter.clone(),
        });
        assert!(f.is_inline());
    }
    assert_eq!(counter.get(), 1, "inner Drop ran exactly once (inline)");
}

#[test]
fn flex_heap_runs_inner_drop() {
    // 24 bytes of payload won't fit Flex<_, B16>'s ?Sized capacity
    // (B16 - 16 = 0). Force the heap path.
    struct BigDrop {
        _payload: [u64; 4],
        counter: Rc<Cell<u32>>,
    }
    impl Drop for BigDrop {
        fn drop(&mut self) {
            self.counter.set(self.counter.get() + 1);
        }
    }
    impl Greet for BigDrop {
        fn greet(&self) -> String {
            "big".into()
        }
    }

    let counter = Rc::new(Cell::new(0u32));
    {
        let f: Flex<dyn Greet, B16> = flex!(BigDrop {
            _payload: [1, 2, 3, 4],
            counter: counter.clone(),
        });
        assert!(!f.is_inline(), "BigDrop should route to heap");
        assert_eq!(f.greet(), "big");
    }
    assert_eq!(counter.get(), 1, "heap allocation freed and Drop ran");
}

// =============================================================================
// Pass 1.5 — Panic-during-drop
// =============================================================================
//
// Exercises the unwind path through the smart-pointer's own Drop. Miri
// is sensitive to double-free / use-after-free even on unwind. The
// outer `catch_unwind` swallows the panic so the test doesn't abort.

#[test]
fn flat_panic_in_inner_drop_is_safe() {
    struct Panicker {
        ran: Rc<Cell<bool>>,
    }
    impl Drop for Panicker {
        fn drop(&mut self) {
            self.ran.set(true);
            // SAFETY of unwind isn't on the inner type — it's whether
            // Flat<T,B>::drop calls our Drop only once and doesn't touch
            // freed memory after.
            panic!("intentional panic during inner Drop");
        }
    }

    let ran = Rc::new(Cell::new(false));
    let ran_clone = ran.clone();

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _f: Flat<Panicker, B32> = Flat::new(Panicker { ran: ran_clone });
        // _f drops here, panicking.
    }));
    assert!(result.is_err(), "panic must propagate out of catch_unwind");
    assert!(ran.get(), "inner Drop must have run");
}

// =============================================================================
// Pass 1.6 — B16 ?Sized boundary (8-byte concrete fits exactly)
// =============================================================================
//
// Flat<dyn Display, B16> reserves 8 bytes for metadata, leaving 8 bytes
// for the concrete value. `u64` is exactly 8 bytes — the "exactly fits"
// edge case from #173.

#[test]
fn flat_b16_fits_8_byte_concrete() {
    let f: Flat<dyn Display, B16> = flat!(0xCAFE_BABEu64);
    assert_eq!(format!("{}", &*f), "3405691582");
}

#[test]
fn flat_b16_fits_8_byte_newtype() {
    // 8-byte newtype with a non-trivial Display impl — exercises the
    // exactly-fits boundary with both the construction and the
    // destruction side present.
    struct EightByteVal(u64);
    impl Display for EightByteVal {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "v{}", self.0)
        }
    }
    assert_eq!(std::mem::size_of::<EightByteVal>(), 8);

    let f: Flat<dyn Display, B16> = flat!(EightByteVal(99));
    assert_eq!(format!("{}", &*f), "v99");
}

// =============================================================================
// Pass 1.7 — Heap fallback round-trip (Flex)
// =============================================================================

#[test]
fn flex_heap_path_deref_and_drop() {
    // Force the heap path: payload bigger than B16 ?Sized capacity (0 bytes).
    let payload: Vec<u8> = (0..64).collect();
    let f: Flex<dyn Greet, B16> = {
        struct Wrap(Vec<u8>);
        impl Greet for Wrap {
            fn greet(&self) -> String {
                format!("wrap-{}", self.0.len())
            }
        }
        flex!(Wrap(payload))
    };
    assert!(!f.is_inline(), "must be heap-allocated");
    assert_eq!(f.greet(), "wrap-64");
    drop(f);
}

// =============================================================================
// Pass 1.8 — Slice trait object via Flat (length metadata, not vtable)
// =============================================================================

#[test]
fn flat_slice_carries_length_metadata() {
    let f: Flat<[u32], B32> = flat!([10u32, 20, 30, 40]);
    assert_eq!(f.len(), 4);
    assert_eq!(&*f, &[10, 20, 30, 40][..]);
}
