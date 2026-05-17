#![cfg(loom)]

use loom::thread;
use nexus_pool::sync::Pool;

#[test]
fn acquire_return_roundtrip() {
    loom::model(|| {
        let pool = Pool::new(2, || 42u32, |_| {});

        let item = pool.try_acquire().unwrap();
        assert_eq!(*item, 42);
        drop(item);

        let item2 = pool.try_acquire().unwrap();
        assert_eq!(*item2, 42);
    });
}

#[test]
fn cross_thread_return() {
    loom::model(|| {
        let pool = Pool::new(2, || 0u32, |v| *v = 0);

        let mut item = pool.try_acquire().unwrap();
        *item = 99;

        let handle = thread::spawn(move || {
            assert_eq!(*item, 99);
            drop(item);
        });

        handle.join().unwrap();

        let reacquired = pool.try_acquire().unwrap();
        assert_eq!(*reacquired, 0); // reset was called
    });
}

#[test]
fn no_double_issue() {
    loom::model(|| {
        let pool = Pool::new(2, || 0u32, |_| {});

        let a = pool.try_acquire().unwrap();
        let b = pool.try_acquire().unwrap();
        assert!(pool.try_acquire().is_none());

        drop(a);

        let c = pool.try_acquire().unwrap();
        assert!(pool.try_acquire().is_none());

        drop(b);
        drop(c);
    });
}

#[test]
fn concurrent_returns() {
    loom::model(|| {
        let pool = Pool::new(2, || 0u32, |_| {});

        let a = pool.try_acquire().unwrap();
        let b = pool.try_acquire().unwrap();
        assert!(pool.try_acquire().is_none());

        let h1 = thread::spawn(move || {
            drop(a);
        });
        let h2 = thread::spawn(move || {
            drop(b);
        });

        h1.join().unwrap();
        h2.join().unwrap();

        let c = pool.try_acquire().unwrap();
        let d = pool.try_acquire().unwrap();
        assert!(pool.try_acquire().is_none());

        drop(c);
        drop(d);
    });
}

#[test]
fn reset_called_on_return() {
    loom::model(|| {
        let pool = Pool::new(2, || Vec::<u8>::with_capacity(4), |v| v.clear());

        let mut item = pool.try_acquire().unwrap();
        item.push(1);
        item.push(2);

        let handle = thread::spawn(move || {
            drop(item);
        });

        handle.join().unwrap();

        let reacquired = pool.try_acquire().unwrap();
        assert!(reacquired.is_empty());
    });
}
