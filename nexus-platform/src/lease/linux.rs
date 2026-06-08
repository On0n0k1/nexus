use std::os::fd::BorrowedFd;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl};
use nix::libc;

use super::Liveness;

const LEASE_OFFSET: i64 = 0;
const LEASE_LEN: i64 = 1;

fn lease_flock(l_type: libc::c_short) -> libc::flock {
    // SAFETY: `flock` is a plain C struct of integers; all-zero is valid.
    let mut lk: libc::flock = unsafe { std::mem::zeroed() };
    lk.l_type = l_type;
    lk.l_whence = libc::SEEK_SET as libc::c_short;
    lk.l_start = LEASE_OFFSET;
    lk.l_len = LEASE_LEN;
    lk
}

/// Try to acquire the lease (non-blocking).
///
/// Returns `Ok(true)` if acquired, `Ok(false)` if another description holds it.
pub(super) fn try_acquire(fd: BorrowedFd<'_>) -> Result<bool, std::io::Error> {
    let lk = lease_flock(libc::F_WRLCK as libc::c_short);
    match fcntl(fd, FcntlArg::F_OFD_SETLK(&lk)) {
        Ok(_) => Ok(true),
        Err(Errno::EACCES | Errno::EAGAIN) => Ok(false),
        Err(e) => Err(std::io::Error::from(e)),
    }
}

/// Probe whether a lease is held on `fd`.
pub(super) fn probe_liveness(fd: BorrowedFd<'_>) -> Liveness {
    let mut lk = lease_flock(libc::F_WRLCK as libc::c_short);
    match fcntl(fd, FcntlArg::F_OFD_GETLK(&mut lk)) {
        Ok(_) if lk.l_type == libc::F_UNLCK as libc::c_short => Liveness::Dead,
        Ok(_) => Liveness::Alive,
        Err(_) => Liveness::Unknown,
    }
}
