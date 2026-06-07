use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use nix::fcntl::{FcntlArg, fcntl};
use nix::libc;

use super::frame::commit_len_ptr;

pub(crate) struct CleanRequest {
    pub(crate) data: *mut u8,
    pub(crate) segment_size: usize,
    pub(crate) ready: Arc<AtomicBool>,
}

// SAFETY: `data` points into a mmap'd segment that remains mapped until the
// owning `Slot` drops, which happens only after the SegmentedLog drops. The
// SegmentedLog holds a sender clone — dropping it before the conductor thread
// processes the request would unmap the segment. But the segment file itself
// remains; the conductor only touches the mmap'd data before marking ready,
// and the SegmentedLog cannot drop until its own Drop runs (which waits for
// no pending clean via the ready flag in practice).
unsafe impl Send for CleanRequest {}

fn conductor_main(rx: std::sync::mpsc::Receiver<CleanRequest>) {
    for req in rx {
        // SAFETY: `req.data` points to the start of a live mmap'd segment.
        // See `CleanRequest` Send impl for lifetime reasoning.
        unsafe { (*commit_len_ptr(req.data)).store(0, Ordering::Release) };
        let _ = req.segment_size;
        req.ready.store(true, Ordering::Release);
    }
}

const LOCK_FILE: &str = "conductor.lock";

fn lock_exclusive(file: &File) {
    let mut lk: libc::flock = unsafe { std::mem::zeroed() };
    lk.l_type = libc::F_WRLCK as libc::c_short;
    lk.l_whence = libc::SEEK_SET as libc::c_short;
    lk.l_start = 0;
    lk.l_len = 0; // entire file
    // F_OFD_SETLKW: blocking, per-fd (not per-process)
    let _ = fcntl(file.as_fd(), FcntlArg::F_OFD_SETLKW(&lk));
}

fn unlock(file: &File) {
    let mut lk: libc::flock = unsafe { std::mem::zeroed() };
    lk.l_type = libc::F_UNLCK as libc::c_short;
    lk.l_whence = libc::SEEK_SET as libc::c_short;
    lk.l_start = 0;
    lk.l_len = 0;
    let _ = fcntl(file.as_fd(), FcntlArg::F_OFD_SETLK(&lk));
}

fn open_lock_file(dir: &Path) -> Result<File, super::SegmentedLogError> {
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE))?)
}

fn read_counter(file: &mut File) -> u32 {
    let mut buf = String::new();
    let _ = file.read_to_string(&mut buf);
    buf.trim().parse().unwrap_or(0)
}

fn write_counter(file: &mut File, val: u32) {
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.set_len(0);
    let _ = write!(file, "{val}");
}

/// Atomically claim the next session ID using a lock file.
///
/// Opens `{dir}/conductor.lock`, takes an exclusive OFD lock, reads the
/// current counter, increments it, writes back, and unlocks. The counter
/// file is a plain ASCII integer for easy inspection.
fn claim_next_session_id(dir: &Path) -> Result<u32, super::SegmentedLogError> {
    let mut file = open_lock_file(dir)?;
    lock_exclusive(&file);

    let current = read_counter(&mut file);
    let next = current + 1;
    write_counter(&mut file, next);

    unlock(&file);
    Ok(next)
}

/// Ensure the counter is at least `id` so future auto-assignments won't
/// collide with explicitly chosen IDs.
fn ensure_counter_at_least(dir: &Path, id: u32) -> Result<(), super::SegmentedLogError> {
    let mut file = open_lock_file(dir)?;
    lock_exclusive(&file);

    let current = read_counter(&mut file);
    if id > current {
        write_counter(&mut file, id);
    }

    unlock(&file);
    Ok(())
}

/// Top-level journal manager.
///
/// Owns the background cleanup thread and the root directory. All
/// [`SegmentedLog`](super::SegmentedLog) instances are opened through
/// the conductor via [`builder()`](Self::builder).
///
/// # Directory layout
///
/// ```text
/// {dir}/
///   conductor.lock      <- session ID counter (flock'd during assignment)
///   {session_id}/
///     journal.manifest
///     seg0.dat, seg1.dat, seg2.dat
/// ```
pub struct Conductor {
    dir: PathBuf,
    tx: Option<std::sync::mpsc::SyncSender<CleanRequest>>,
    thread: Option<JoinHandle<()>>,
    active: HashSet<u32>,
}

impl Conductor {
    /// Open a conductor rooted at `dir`.
    ///
    /// Scans for existing session subdirectories (those containing a
    /// `journal.manifest` file) but does not open any sessions — call
    /// [`builder()`](Self::builder) to open or create individual sessions.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, super::SegmentedLogError> {
        let dir = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;

        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let thread = std::thread::spawn(move || conductor_main(rx));

        Ok(Self {
            dir,
            tx: Some(tx),
            thread: Some(thread),
            active: HashSet::new(),
        })
    }

    /// Return a builder for opening or creating a session log.
    pub fn builder(&mut self) -> super::SegmentedLogBuilder<'_> {
        super::SegmentedLogBuilder::new(self)
    }

    /// List session IDs that have manifests on disk.
    pub fn sessions_on_disk(&self) -> Result<Vec<u32>, super::SegmentedLogError> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if entry.path().join(super::MANIFEST_FILE).exists()
                && let Some(name) = entry.file_name().to_str()
                && let Ok(id) = name.parse::<u32>()
            {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }

    /// Atomically claim the next session ID.
    ///
    /// Uses a lock file (`conductor.lock`) to coordinate across processes.
    /// Multiple conductors on the same directory will never assign the same ID.
    pub(crate) fn next_session_id(&self) -> Result<u32, super::SegmentedLogError> {
        claim_next_session_id(&self.dir)
    }

    /// Ensure the lock counter won't collide with an explicitly chosen ID.
    pub(crate) fn register_explicit_id(&self, id: u32) -> Result<(), super::SegmentedLogError> {
        ensure_counter_at_least(&self.dir, id)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn sender(&self) -> std::sync::mpsc::SyncSender<CleanRequest> {
        self.tx.as_ref().expect("conductor shut down").clone()
    }

    pub(crate) fn mark_active(&mut self, id: u32) {
        self.active.insert(id);
    }

    pub(crate) fn is_active(&self, id: u32) -> bool {
        self.active.contains(&id)
    }

    #[allow(dead_code)]
    pub(crate) fn release(&mut self, id: u32) {
        self.active.remove(&id);
    }
}

impl Drop for Conductor {
    fn drop(&mut self) {
        // Drop our sender first so the channel closes once all SegmentedLog
        // clones are also dropped.
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
