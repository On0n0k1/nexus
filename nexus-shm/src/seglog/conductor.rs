use std::io::{Read as _, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use super::frame::commit_len_ptr;
use super::platform;

const LOCK_FILE: &str = "conductor.lock";
const DEFAULT_CLEAN_QUEUE_DEPTH: usize = 4;

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Builder for configuring a [`Conductor`].
///
/// Use this when the default configuration is not suitable — for example,
/// to increase the clean queue depth when running many concurrent sessions.
///
/// ```no_run
/// # use nexus_shm::ConductorBuilder;
/// let mut conductor = ConductorBuilder::new("/tmp/journal")
///     .clean_queue_depth(16)
///     .open()
///     .unwrap();
/// ```
pub struct ConductorBuilder {
    dir: PathBuf,
    clean_queue_depth: usize,
}

impl ConductorBuilder {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            clean_queue_depth: DEFAULT_CLEAN_QUEUE_DEPTH,
        }
    }

    /// Maximum number of outstanding segment-clean requests (default: 4).
    ///
    /// Each session can have at most one outstanding clean request at a time.
    /// If multiple sessions rotate simultaneously and the queue is full,
    /// `append` will block briefly until the conductor thread drains one.
    pub fn clean_queue_depth(mut self, depth: usize) -> Self {
        self.clean_queue_depth = depth;
        self
    }

    /// Open the conductor, creating the root directory if needed.
    pub fn open(self) -> Result<Conductor, super::OpenError> {
        std::fs::create_dir_all(&self.dir)?;

        let (tx, rx) = std::sync::mpsc::sync_channel(self.clean_queue_depth);
        let thread = std::thread::spawn(move || conductor_main(rx));

        Ok(Conductor {
            dir: self.dir,
            tx: Some(tx),
            thread: Some(thread),
        })
    }
}

/// Top-level journal manager.
///
/// Owns the background cleanup thread and the root directory. All
/// [`SegmentedLog`](super::SegmentedLog) instances are opened through
/// the conductor via [`session()`](Self::session).
///
/// # Lifetime
///
/// The conductor **must** outlive all [`SegmentedLog`] instances opened
/// through it. `Conductor::drop` joins the cleanup thread, which blocks
/// until every session's sender is dropped. Dropping a conductor while
/// sessions are still alive will block indefinitely.
///
/// # Directory layout
///
/// ```text
/// {dir}/
///   conductor.lock      <- session ID counter (OFD-locked during assignment)
///   {session_id}/
///     session.lock      <- OFD-locked while open (prevents double-open)
///     journal.manifest
///     seg0.dat, seg1.dat, seg2.dat
/// ```
pub struct Conductor {
    dir: PathBuf,
    tx: Option<std::sync::mpsc::SyncSender<CleanRequest>>,
    thread: Option<JoinHandle<()>>,
}

impl Conductor {
    /// Open a conductor rooted at `dir` with default configuration.
    ///
    /// Creates the directory if it does not exist. Use [`ConductorBuilder`]
    /// for custom configuration (e.g. clean queue depth).
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, super::OpenError> {
        ConductorBuilder::new(dir).open()
    }

    /// Return a builder for opening or creating a session log.
    pub fn session(&mut self) -> super::SegmentedLogBuilder<'_> {
        super::SegmentedLogBuilder::new(self)
    }

    /// List session IDs that have manifests on disk.
    pub fn sessions_on_disk(&self) -> Result<Vec<u32>, super::OpenError> {
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
    pub(crate) fn next_session_id(&self) -> Result<u32, super::OpenError> {
        claim_next_session_id(&self.dir)
    }

    /// Ensure the lock counter won't collide with an explicitly chosen ID.
    pub(crate) fn register_explicit_id(&self, id: u32) -> Result<(), super::OpenError> {
        ensure_counter_at_least(&self.dir, id)
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn sender(&self) -> std::sync::mpsc::SyncSender<CleanRequest> {
        self.tx.as_ref().expect("conductor shut down").clone()
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

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Background cleanup loop for evicted segments.
///
/// Runs on a dedicated thread, processing one `CleanRequest` per segment
/// rotation. The thread stays alive as long as any `SyncSender` clone
/// exists — the `Conductor` holds one, and each `SegmentedLog` holds
/// another. When all senders drop, `rx.recv()` returns `Err`, the
/// for-loop exits, and the thread returns (unblocking `Conductor::drop`'s
/// `join()`).
fn conductor_main(rx: std::sync::mpsc::Receiver<CleanRequest>) {
    for req in rx {
        // TODO: archive the evicted segment to disk before cleaning
        //       (read segment data via req.data/req.segment_size, write
        //       to archive dir, then zero). Archival I/O errors must not
        //       panic — a panic here leaves in-flight `ready` flags false,
        //       causing SegmentedLog::drop to spin forever. Handle errors
        //       gracefully (log + skip) and always proceed to the zero +
        //       ready store below.

        // SAFETY: `req.data` points to the start of a live mmap'd segment.
        // See `CleanRequest` Send impl for lifetime reasoning.
        unsafe { (*commit_len_ptr(req.data)).store(0, Ordering::Release) };
        // segment_size is unused today but carried for archival (will need
        // it to know how many bytes to flush before zeroing).
        let _ = req.segment_size;
        req.ready.store(true, Ordering::Release);
    }
}

/// Atomically claim the next session ID using a lock file.
///
/// Acquires an exclusive lock on `{dir}/conductor.lock`, reads the
/// current counter, increments it, and writes back. The lock is released
/// when the `FileLock` drops. The counter file is a plain ASCII integer
/// for easy inspection.
fn claim_next_session_id(dir: &Path) -> Result<u32, super::OpenError> {
    let mut lock = platform::FileLock::blocking(dir.join(LOCK_FILE))?;
    let current = read_counter(lock.file())?;
    let next = current + 1;
    write_counter(lock.file(), next)?;
    Ok(next)
}

/// Ensure the counter is at least `id` so future auto-assignments won't
/// collide with explicitly chosen IDs.
fn ensure_counter_at_least(dir: &Path, id: u32) -> Result<(), super::OpenError> {
    let mut lock = platform::FileLock::blocking(dir.join(LOCK_FILE))?;
    let current = read_counter(lock.file())?;
    if id > current {
        write_counter(lock.file(), id)?;
    }
    Ok(())
}

fn read_counter(file: &mut std::fs::File) -> Result<u32, std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(buf.trim().parse().unwrap_or(0))
}

fn write_counter(file: &mut std::fs::File, val: u32) -> Result<(), std::io::Error> {
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    write!(file, "{val}")?;
    Ok(())
}
