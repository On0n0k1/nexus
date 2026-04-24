//! Reproduction for suspected bug: slab task in all_tasks at Runtime::drop
//! triggers panic in slab_free_task because SLAB_FREE TLS is cleared after
//! run_loop exits, but Executor::drop still calls free_task on slab tasks.

use nexus_async_rt::{Runtime, spawn_slab};
use nexus_rt::WorldBuilder;

#[test]
fn slab_task_uncompleted_at_runtime_drop_panics() {
    let slab = unsafe { nexus_slab::byte::unbounded::Slab::<256>::with_chunk_capacity(8) };
    let wb = WorldBuilder::new();
    let mut world = wb.build();
    let mut rt = Runtime::builder(&mut world).slab_unbounded(slab).build();

    // Spawn a slab task that never completes, drop handle immediately.
    // Root future returns right away — the slab task is in executor.all_tasks
    // but never ran.
    rt.block_on(async {
        drop(spawn_slab(async move {
            std::future::pending::<()>().await;
        }));
    });
    // TLS cleared here. executor.all_tasks has one slab task, not completed.

    // drop(rt) should NOT panic — but I expect it does.
    drop(rt);
}

#[test]
fn slab_handle_dropped_outside_block_on_panics() {
    let slab = unsafe { nexus_slab::byte::unbounded::Slab::<256>::with_chunk_capacity(8) };
    let wb = WorldBuilder::new();
    let mut world = wb.build();
    let mut rt = Runtime::builder(&mut world).slab_unbounded(slab).build();

    // Return handle from block_on — task hasn't run.
    let handle = rt.block_on(async {
        spawn_slab(async { 42u32 })
    });
    // TLS cleared. Task not completed, refcount=2 (exec + handle).

    drop(handle);
    // JoinHandle::Drop: task not completed → don't drop output.
    // clear_has_join, ref_dec → refcount 1, Retain.
    // Task still in all_tasks.

    drop(rt);
    // Executor::drop: finds task, drop_task_future, complete_and_unref → FreeSlab,
    // free_task → slab_free_task → TLS null → PANIC.
}
