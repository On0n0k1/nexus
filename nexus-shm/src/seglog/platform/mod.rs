use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
use linux as imp;

#[cfg(not(target_os = "linux"))]
compile_error!(
    "seglog file locking requires OFD locks (Linux). \
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

/// RAII exclusive file lock backed by platform-specific advisory locks.
///
/// On Linux this uses OFD locks (`F_OFD_SETLK`), which are per-file-description
/// rather than per-process — they correctly serialize both cross-process and
/// in-process access when different file descriptors are used. The lock is
/// released when the file descriptor closes (i.e., when this struct drops).
pub(crate) struct FileLock {
    file: File,
}

impl FileLock {
    /// Acquire an exclusive lock, blocking until available.
    pub(crate) fn blocking(path: impl AsRef<Path>) -> Result<Self, io::Error> {
        let file = open_lock_file(path)?;
        imp::lock_exclusive_blocking(&file)?;
        Ok(Self { file })
    }

    /// Try to acquire an exclusive lock without blocking.
    ///
    /// Returns `Ok(None)` if another file description already holds it.
    pub(crate) fn try_lock(path: impl AsRef<Path>) -> Result<Option<Self>, io::Error> {
        let file = open_lock_file(path)?;
        if imp::try_lock_exclusive(&file)? {
            Ok(Some(Self { file }))
        } else {
            Ok(None)
        }
    }

    /// Access the underlying file for read/write operations.
    pub(crate) fn file(&mut self) -> &mut File {
        &mut self.file
    }
}
