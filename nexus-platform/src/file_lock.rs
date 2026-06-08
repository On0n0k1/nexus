use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(target_os = "linux"))]
compile_error!(
    "nexus-platform file locking requires OFD locks (Linux). \
     macOS/Windows support is not yet implemented."
);

fn open_lock_file(path: impl AsRef<Path>) -> Result<File, io::Error> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// RAII exclusive file lock for mutual exclusion.
///
/// Acquires an exclusive advisory lock on a file at the given path,
/// creating it if necessary. The lock is released when this struct drops
/// (the kernel releases the lock when the file descriptor closes).
///
/// On Linux this uses OFD locks (`F_OFD_SETLK`), which are
/// per-file-description — they correctly serialize both cross-process
/// and in-process access when different file descriptors are used.
///
/// # Examples
///
/// ```no_run
/// # use nexus_platform::FileLock;
/// let mut lock = FileLock::lock("/tmp/my.lock").unwrap();
/// // lock is held until `lock` is dropped
/// ```
pub struct FileLock {
    file: File,
}

impl FileLock {
    /// Acquire an exclusive lock, blocking until available.
    pub fn lock(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = open_lock_file(path)?;
        imp::lock_exclusive_blocking(&file)?;
        Ok(Self { file })
    }

    /// Try to acquire an exclusive lock without blocking.
    ///
    /// Returns `Ok(None)` if another file description already holds it.
    pub fn try_lock(path: impl AsRef<Path>) -> Result<Option<Self>, io::Error> {
        let file = open_lock_file(path)?;
        if imp::try_lock_exclusive(&file)? {
            Ok(Some(Self { file }))
        } else {
            Ok(None)
        }
    }

    /// Access the underlying file for read/write operations.
    pub fn file(&mut self) -> &mut File {
        &mut self.file
    }
}
