use std::os::fd::BorrowedFd;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(target_os = "linux"))]
compile_error!(
    "nexus-platform process lease requires OFD locks (Linux). \
     macOS/Windows support is not yet implemented."
);

/// Result of probing a [`ProcessLease`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// The owning process is alive (lease still held).
    Alive,
    /// The owning process has exited (lease released by kernel).
    Dead,
    /// The probe failed (e.g. invalid fd). Treat as indeterminate.
    Unknown,
}

/// Kernel-mediated process liveness detection.
///
/// Uses an advisory byte-range lock on a file descriptor to signal that
/// a process is alive. When the owning process exits (even via `SIGKILL`),
/// the kernel releases the lock automatically. Peers can [`probe`](Self::probe)
/// the fd to check whether the owner is still alive.
///
/// This is **not** mutual exclusion — it is a liveness oracle backed by
/// the kernel's lock table. The lease is tied to the fd's file description
/// and lives as long as that file description remains open.
///
/// On Linux this uses OFD locks (`F_OFD_SETLK` / `F_OFD_GETLK`) on a
/// single-byte range at offset 0.
pub struct ProcessLease;

impl ProcessLease {
    /// Claim a lease on the given fd.
    ///
    /// Acquires an exclusive advisory lock on a single-byte range of the
    /// file. The lease is held as long as the fd's file description remains
    /// open (i.e., until the owner closes all fds sharing that description
    /// or exits).
    ///
    /// Returns `Ok(true)` if the lease was acquired, `Ok(false)` if another
    /// process already holds it.
    pub fn claim(fd: BorrowedFd<'_>) -> Result<bool, std::io::Error> {
        imp::try_acquire(fd)
    }

    /// Probe whether a lease is held on the given fd.
    ///
    /// This does not acquire any lock — it queries the kernel's lock table
    /// to determine if a write lock is held at the lease byte range.
    pub fn probe(fd: BorrowedFd<'_>) -> Liveness {
        imp::probe_liveness(fd)
    }
}
