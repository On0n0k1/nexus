//! Miri tests for nexus-pool object pools.
//!
//! Run: `cargo +nightly miri test -p nexus-pool --test miri_tests`

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn local_bounded_acquire_release() {
    let pool = nexus_pool::local::BoundedPool::new(
        4,
        || Vec::<u8>::with_capacity(64),
        |v: &mut Vec<u8>| v.clear(),
    );

    assert_eq!(pool.available(), 4);

    // Acquire 3 items and use them.
    let mut a = pool.try_acquire().unwrap();
    let mut b = pool.try_acquire().unwrap();
    let mut c = pool.try_acquire().unwrap();

    a.extend_from_slice(b"aaa");
    b.extend_from_slice(b"bbb");
    c.extend_from_slice(b"ccc");

    assert_eq!(pool.available(), 1);

    // Drop guards -- items return to pool.
    drop(a);
    drop(b);
    drop(c);

    assert_eq!(pool.available(), 4);

    // Re-acquire and verify reset was called (vec should be empty).
    let d = pool.try_acquire().unwrap();
    assert!(d.is_empty(), "reset should have cleared the vec");
}

#[test]
fn local_pool_take_put() {
    let pool =
        nexus_pool::local::Pool::new(|| Vec::<u8>::with_capacity(64), |v: &mut Vec<u8>| v.clear());

    // Take a value (creates via factory since pool is empty).
    let mut buf = pool.take();
    buf.extend_from_slice(b"hello");
    assert_eq!(&buf, b"hello");

    // Put it back -- reset (clear) is called.
    pool.put(buf);
    assert_eq!(pool.available(), 1);

    // Take again -- should get the reset (empty) value.
    let reused = pool.take();
    assert!(reused.is_empty(), "reset should have cleared the vec");
    pool.put(reused);
}

#[test]
fn sync_acquire_release() {
    let pool = nexus_pool::sync::Pool::new(
        4,
        || Vec::<u8>::with_capacity(64),
        |v: &mut Vec<u8>| v.clear(),
    );

    assert_eq!(pool.available(), 4);

    // Acquire 2 items.
    let mut a = pool.try_acquire().unwrap();
    let mut b = pool.try_acquire().unwrap();

    assert_eq!(pool.available(), 2);

    a.extend_from_slice(b"aaa");
    b.extend_from_slice(b"bbb");

    // Drop items -- should return to pool.
    drop(a);
    drop(b);

    assert_eq!(pool.available(), 4);

    // Verify reset was applied.
    let c = pool.try_acquire().unwrap();
    assert!(c.is_empty(), "reset should have cleared the vec");
}

#[test]
fn local_guard_outlives_pool_retains_inpool_values_until_last_guard() {
    // New (1.1.0) contract: in-pool values live until the last
    // `Pooled<T>` guard drops. Dropping the pool with outstanding
    // guards no longer drops in-pool values immediately — they sit in
    // the orphaned Inner's Vec until the strong-count reaches zero.

    struct Tracked {
        counter: Rc<Cell<u32>>,
    }
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.counter.set(self.counter.get() + 1);
        }
    }

    let drop_count = Rc::new(Cell::new(0u32));
    let dc = drop_count.clone();
    let pool = nexus_pool::local::BoundedPool::new(
        4,
        move || Tracked {
            counter: dc.clone(),
        },
        |_| {},
    );

    // Acquire 2 items, hold them. 2 still in the pool, 2 out.
    let a = pool.try_acquire().unwrap();
    let b = pool.try_acquire().unwrap();
    assert_eq!(drop_count.get(), 0);

    // Drop pool. NEW SEMANTICS: in-pool values are NOT dropped yet —
    // Inner survives via the strong Rc held by `a` and `b`.
    drop(pool);
    assert_eq!(
        drop_count.get(),
        0,
        "in-pool values retained until last guard drops"
    );

    // Drop first guard — it returns to orphaned Inner; no drop yet.
    drop(a);
    assert_eq!(drop_count.get(), 0);

    // Drop last guard — Inner finally dies; all 4 values drop together
    // (2 returned-to-orphan + 2 still-in-pool).
    drop(b);
    assert_eq!(drop_count.get(), 4);
}

#[test]
fn sync_guard_outlives_pool_miri() {
    // Same pattern for sync::Pool — must not UAF.
    // Pool's Arc drops -> Inner survives via Pooled's Arc.
    // Last guard drops -> Inner::drop walks the free list,
    // assume_init_drops every in-pool slot.

    struct Tracked {
        c: Arc<AtomicUsize>,
    }
    impl Drop for Tracked {
        fn drop(&mut self) {
            self.c.fetch_add(1, Ordering::Relaxed);
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);
    let pool = nexus_pool::sync::Pool::new(4, move || Tracked { c: Arc::clone(&c) }, |_| {});

    let g1 = pool.try_acquire().unwrap();
    let g2 = pool.try_acquire().unwrap();

    drop(pool);
    assert_eq!(
        counter.load(Ordering::Relaxed),
        0,
        "in-pool slots retained while guards alive"
    );

    drop(g1);
    assert_eq!(counter.load(Ordering::Relaxed), 0);

    drop(g2);
    assert_eq!(
        counter.load(Ordering::Relaxed),
        4,
        "all 4 slots drop when last guard exits and Inner::drop runs"
    );
}
