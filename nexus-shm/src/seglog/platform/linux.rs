use std::fs::File;
use std::os::fd::AsFd;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl};
use nix::libc;

fn wrlck() -> libc::flock {
    // SAFETY: `flock` is a plain C struct of integers; all-zero is valid.
    let mut lk: libc::flock = unsafe { std::mem::zeroed() };
    lk.l_type = libc::F_WRLCK as libc::c_short;
    lk.l_whence = libc::SEEK_SET as libc::c_short;
    lk.l_start = 0;
    lk.l_len = 0; // entire file
    lk
}

/// Acquire an exclusive OFD lock on `file`, blocking until available.
pub(crate) fn lock_exclusive_blocking(file: &File) -> Result<(), std::io::Error> {
    fcntl(file.as_fd(), FcntlArg::F_OFD_SETLKW(&wrlck())).map_err(std::io::Error::from)?;
    Ok(())
}

/// Try to acquire an exclusive OFD lock without blocking.
///
/// Returns `Ok(true)` if the lock was acquired, `Ok(false)` if another
/// file description already holds it.
pub(crate) fn try_lock_exclusive(file: &File) -> Result<bool, std::io::Error> {
    match fcntl(file.as_fd(), FcntlArg::F_OFD_SETLK(&wrlck())) {
        Ok(_) => Ok(true),
        Err(Errno::EAGAIN | Errno::EACCES) => Ok(false),
        Err(e) => Err(std::io::Error::from(e)),
    }
}
