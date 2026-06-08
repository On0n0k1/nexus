use std::fs::{File, OpenOptions};
use std::num::NonZeroUsize;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;
use std::ptr::NonNull;

use nix::errno::Errno;
use nix::sys::mman::{MapFlags, ProtFlags, mmap, munmap};

use crate::error::ShmError;

#[derive(Clone, Copy, Default)]
pub struct MapOptions {
    pub pretouch: bool,
    pub huge_pages: bool,
}

pub(crate) struct Mapping {
    ptr: NonNull<u8>,
    len: NonZeroUsize,
    file: File,
}

impl Mapping {
    pub(crate) fn create(
        path: &Path,
        len: NonZeroUsize,
        opts: MapOptions,
    ) -> Result<Self, ShmError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.set_len(len.get() as u64)?;
        Self::map(file, len, opts)
    }

    pub(crate) fn open(path: &Path, opts: MapOptions) -> Result<Self, ShmError> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let len =
            NonZeroUsize::new(file.metadata()?.len() as usize).ok_or(ShmError::EmptySegment)?;
        Self::map(file, len, opts)
    }

    fn map(file: File, len: NonZeroUsize, opts: MapOptions) -> Result<Self, ShmError> {
        let mut flags = MapFlags::MAP_SHARED;
        if opts.pretouch {
            flags |= MapFlags::MAP_POPULATE;
        }
        if opts.huge_pages {
            flags |= MapFlags::MAP_HUGETLB;
        }

        // SAFETY: `len` is non-zero; the kernel chooses the address (`None`).
        // `file` is a valid open fd held for the mapping's lifetime via the
        // returned `Mapping`, and the mapped length matches the file length set
        // by the caller.
        let ptr = unsafe {
            mmap(
                None,
                len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                flags,
                file.as_fd(),
                0,
            )
        }
        .map_err(|e| {
            // ENOMEM with MAP_HUGETLB means the huge-page pool is exhausted;
            // other errnos are unrelated failures, not a huge-pages verdict.
            if opts.huge_pages && e == Errno::ENOMEM {
                ShmError::HugePagesUnavailable(e.into())
            } else {
                ShmError::Os(e.into())
            }
        })?;

        Ok(Self {
            ptr: ptr.cast(),
            len,
            file,
        })
    }

    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    pub(crate) fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: `ptr` and `len` come from the successful `mmap` in `map` and
        // are unchanged since construction, so they are a valid mapping to unmap.
        unsafe {
            let _ = munmap(self.ptr.cast(), self.len.get());
        }
    }
}

// SAFETY: a mapping is a raw pointer plus its backing fd; the bytes live in
// shared memory, not thread-local state. Concurrent access to the contents is
// governed by the atomics in the control block, so the handle itself is safe to
// move and share across threads.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}
