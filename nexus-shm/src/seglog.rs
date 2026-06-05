use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;

use crate::error::ShmError;
use crate::region::MapOptions;
use crate::segment::Segment;

const FRAME_HDR: usize = 8;
const ALIGN: usize = 8;

const fn align_up(n: usize) -> usize {
    (n + ALIGN - 1) & !(ALIGN - 1)
}

const fn footprint(body: usize) -> usize {
    FRAME_HDR + align_up(body)
}

// Returns a `*mut AtomicU32` pointing at the commit_len field of a frame header.
// Callers must ensure `ptr` is 4-byte aligned and points into a live mmap'd segment.
fn commit_len_ptr(ptr: *mut u8) -> *mut AtomicU32 {
    ptr.cast()
}

/// Opaque position handle returned by [`SegmentedLog::append`], passed to
/// [`SegmentedLog::read`]. Valid until the slot it references is rotated out.
///
/// Encoding: `[63:34]` = generation, `[33:32]` = slot index, `[31:0]` = local offset.
/// The default value (`u64::MAX`) encodes slot index 3 (no valid slot), so
/// [`SegmentedLog::read`] always returns `None` for a default offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogOffset(u64);

impl Default for LogOffset {
    fn default() -> Self {
        Self(u64::MAX)
    }
}

impl LogOffset {
    fn new(slot: u8, local_off: usize, epoch: u32) -> Self {
        Self((epoch as u64) << 34 | (slot as u64) << 32 | local_off as u64)
    }

    fn slot(self) -> usize {
        ((self.0 >> 32) & 0x3) as usize
    }

    fn local_off(self) -> usize {
        (self.0 & 0xFFFF_FFFF) as usize
    }

    fn epoch(self) -> u32 {
        (self.0 >> 34) as u32
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SegmentedLogError {
    RecordTooLarge { max: usize },
    StandbyNotReady,
    Shm(ShmError),
}

impl std::fmt::Display for SegmentedLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RecordTooLarge { max } => {
                write!(f, "payload exceeds segment capacity ({max} bytes max)")
            }
            Self::StandbyNotReady => {
                write!(f, "conductor has not finished cleaning the standby segment")
            }
            Self::Shm(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SegmentedLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Shm(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ShmError> for SegmentedLogError {
    fn from(e: ShmError) -> Self {
        Self::Shm(e)
    }
}

struct CleanRequest {
    data: *mut u8,
    segment_size: usize,
}

unsafe impl Send for CleanRequest {}

fn conductor_main(rx: std::sync::mpsc::Receiver<CleanRequest>, ready: &Arc<AtomicBool>) {
    for req in rx {
        // SAFETY: `req.data` points to the start of a live mmap'd segment sent by
        // `rotate()`. The segment remains mapped until the owning `Slot` drops,
        // which happens only after `ConductorHandle` drops and this thread joins.
        unsafe { (*commit_len_ptr(req.data)).store(0, Ordering::Release) };
        let _ = req.segment_size;
        ready.store(true, Ordering::Release);
    }
}

struct ConductorHandle {
    tx: Option<std::sync::mpsc::SyncSender<CleanRequest>>,
    ready: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ConductorHandle {
    fn drop(&mut self) {
        drop(self.tx.take()); // close channel → conductor loop exits
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Slot {
    _segment: Segment,
    data: *mut u8,
}

unsafe impl Send for Slot {}

/// Three-segment bounded append log with background segment rotation.
///
/// Maintains three fixed mmap'd segments: one active for appends, one
/// read-only for lookups, one being cleaned by a conductor thread. When the
/// active segment fills, roles rotate: old readable becomes conductor input,
/// active becomes readable, clean standby becomes active. The hot-path append
/// never blocks on cleaning; the conductor must finish before the *next*
/// rotation (size segments with enough headroom for the expected message rate).
pub struct SegmentedLog {
    // conductor is dropped first (joins thread), then slots (unmaps memory).
    conductor: ConductorHandle,
    slots: [Slot; 3],
    segment_size: usize,
    current: usize,
    prev: usize,
    standby: usize,
    cursor: usize,
    epoch: u32,
    slot_gen: [u32; 3],
}

fn slot_path(base: &Path, i: u8) -> PathBuf {
    let mut s = base.as_os_str().to_owned();
    s.push(format!(".seg{i}"));
    PathBuf::from(s)
}

impl SegmentedLog {
    /// Open (or recreate) a three-segment log rooted at `base`.
    ///
    /// Segment files are `{base}.seg0`, `{base}.seg1`, `{base}.seg2`.
    /// `segment_size` is rounded up to an 8-byte boundary.
    pub fn open(
        base: &Path,
        segment_size: usize,
        map: MapOptions,
    ) -> Result<Self, SegmentedLogError> {
        let size = align_up(segment_size.max(FRAME_HDR * 8));

        let mk = |i: u8| -> Result<Slot, SegmentedLogError> {
            let seg = Segment::create(&slot_path(base, i), size, map)?;
            let data = seg.data();
            Ok(Slot {
                _segment: seg,
                data,
            })
        };

        let s0 = mk(0)?;
        let s1 = mk(1)?;
        let s2 = mk(2)?;

        // SAFETY: each `sN.data` points to the start of a freshly mapped segment
        // with at least `FRAME_HDR` bytes and 4-byte alignment guaranteed by mmap.
        // Zeroing commit_len prevents stale data from a prior run being interpreted
        // as a committed record by `read()`.
        unsafe {
            (*commit_len_ptr(s0.data)).store(0, Ordering::Relaxed);
            (*commit_len_ptr(s1.data)).store(0, Ordering::Relaxed);
            (*commit_len_ptr(s2.data)).store(0, Ordering::Relaxed);
        }

        let ready = Arc::new(AtomicBool::new(true));
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let thread = {
            let ready = Arc::clone(&ready);
            std::thread::spawn(move || conductor_main(rx, &ready))
        };

        Ok(Self {
            conductor: ConductorHandle {
                tx: Some(tx),
                ready,
                thread: Some(thread),
            },
            slots: [s0, s1, s2],
            segment_size: size,
            current: 0,
            prev: 1,
            standby: 2,
            cursor: 0,
            epoch: 0,
            // Slot 0 is current at generation 0. Slots 1 and 2 hold no valid
            // data yet; u32::MAX will never match an offset encoded in this run.
            slot_gen: [0, u32::MAX, u32::MAX],
        })
    }

    pub fn segment_size(&self) -> usize {
        self.segment_size
    }

    /// Append `payload` to the active segment.
    ///
    /// Returns a [`LogOffset`] valid for reads until the slot is rotated out
    /// (two rotations after this write).
    pub fn append(&mut self, payload: &[u8]) -> Result<LogOffset, SegmentedLogError> {
        let body = payload.len();
        let foot = footprint(body);
        if foot > self.segment_size {
            return Err(SegmentedLogError::RecordTooLarge {
                max: self.segment_size.saturating_sub(FRAME_HDR),
            });
        }
        if self.cursor + foot > self.segment_size {
            self.rotate()?;
        }
        let off = self.cursor;
        let data = self.slots[self.current].data;
        // SAFETY: `off + foot <= self.segment_size` (checked above or after rotate).
        // `data` points into a live mmap'd segment that is at least `segment_size`
        // bytes. Frame header fields are at 4-byte-aligned offsets within the
        // segment. The sentinel store at `data.add(next)` is bounds-checked before
        // it is written.
        unsafe {
            let ptr = data.add(off);
            std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr.add(FRAME_HDR), body);
            let next = off + foot;
            if next + FRAME_HDR <= self.segment_size {
                (*commit_len_ptr(data.add(next))).store(0, Ordering::Relaxed);
            }
            // Store body+1 so that 0 remains the unambiguous "not committed" sentinel,
            // allowing empty payloads (body=0) to be stored and read back correctly.
            (*commit_len_ptr(ptr)).store((body as u32).wrapping_add(1), Ordering::Release);
        }
        self.cursor += foot;
        Ok(LogOffset::new(
            self.current as u8,
            off,
            self.slot_gen[self.current],
        ))
    }

    /// Return the payload stored at `offset`, or `None` if the slot has been
    /// rotated out and is no longer readable.
    pub fn read(&self, offset: LogOffset) -> Option<&[u8]> {
        let slot = offset.slot();
        if slot != self.current && slot != self.prev {
            return None;
        }
        if offset.epoch() != self.slot_gen[slot] {
            return None;
        }
        let off = offset.local_off();
        let data = self.slots[slot].data;
        // SAFETY: `slot` is either `current` or `prev`, both of which hold live
        // mmap'd segments. `off` is a value previously returned by `append()` for
        // this slot, so `off < segment_size`. The bounds check on `off + FRAME_HDR
        // + body` prevents reading past the end of the segment.
        unsafe {
            let ptr = data.add(off);
            let stored = (*commit_len_ptr(ptr)).load(Ordering::Acquire);
            if stored == 0 {
                return None;
            }
            let body = (stored - 1) as usize;
            if off + FRAME_HDR + body > self.segment_size {
                return None;
            }
            Some(std::slice::from_raw_parts(ptr.add(FRAME_HDR), body))
        }
    }

    fn rotate(&mut self) -> Result<(), SegmentedLogError> {
        if !self.conductor.ready.load(Ordering::Acquire) {
            return Err(SegmentedLogError::StandbyNotReady);
        }
        let old_prev = self.prev;
        self.prev = self.current;
        self.current = self.standby;
        self.standby = old_prev;
        self.cursor = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.slot_gen[self.current] = self.epoch;
        // TODO: if the conductor gains fallible work (archival, compression), add
        // error propagation here — the current `let _ =` would silently swallow
        // conductor failures. A flush-on-exit signal (distinct from channel close)
        // will also be needed if we require the final segment to be archived before
        // dropping.
        self.conductor.ready.store(false, Ordering::Release);
        let _ = self.conductor.tx.as_ref().map(|tx| {
            tx.try_send(CleanRequest {
                data: self.slots[old_prev].data,
                segment_size: self.segment_size,
            })
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nexus-seglog-{}-{}", std::process::id(), name))
    }

    fn cleanup(base: &Path) {
        for i in 0..3u8 {
            let _ = std::fs::remove_file(slot_path(base, i));
        }
    }

    fn open(base: &Path, size: usize) -> SegmentedLog {
        SegmentedLog::open(base, size, MapOptions::default()).unwrap()
    }

    #[test]
    fn roundtrip() {
        let b = base("rt");
        cleanup(&b);
        let mut log = open(&b, 1 << 16);
        let off = log.append(b"hello").unwrap();
        assert_eq!(log.read(off), Some(b"hello".as_ref()));
        cleanup(&b);
    }

    #[test]
    fn multiple_records_in_one_segment() {
        let b = base("multi");
        cleanup(&b);
        let mut log = open(&b, 1 << 16);
        let o1 = log.append(b"aaa").unwrap();
        let o2 = log.append(b"bb").unwrap();
        let o3 = log.append(b"cccc").unwrap();
        assert_eq!(log.read(o1), Some(b"aaa".as_ref()));
        assert_eq!(log.read(o2), Some(b"bb".as_ref()));
        assert_eq!(log.read(o3), Some(b"cccc".as_ref()));
        cleanup(&b);
    }

    #[test]
    fn rotation_makes_prev_slot_readable() {
        let b = base("rot");
        cleanup(&b);
        // footprint(8) = 16; 4 records = 64 bytes; segment_size = 64
        let mut log = open(&b, 64);
        let o0 = log.append(&[0u8; 8]).unwrap();
        let _o1 = log.append(&[1u8; 8]).unwrap();
        let _o2 = log.append(&[2u8; 8]).unwrap();
        let o3 = log.append(&[3u8; 8]).unwrap();
        // cursor now == 64 == segment_size → next append triggers rotation
        let o4 = log.append(&[4u8; 8]).unwrap();
        // slot 0 (prev) still readable
        assert_eq!(log.read(o0), Some([0u8; 8].as_ref()));
        assert_eq!(log.read(o3), Some([3u8; 8].as_ref()));
        // slot 1 (current) readable
        assert_eq!(log.read(o4), Some([4u8; 8].as_ref()));
        cleanup(&b);
    }

    #[test]
    fn evicted_slot_returns_none() {
        let b = base("evict");
        cleanup(&b);
        // footprint(8) = 16; 4 records per segment; segment_size = 64
        let mut log = open(&b, 64);
        let o0 = log.append(&[0u8; 8]).unwrap();
        // fill slot 0
        for _ in 0..3 {
            log.append(&[0u8; 8]).unwrap();
        }
        // rotation 1 triggered by the first of the next 4 appends:
        //   slot 0 → prev, slot 2 → current, slot 1 → standby (conductor cleaning slot 1)
        for _ in 0..4 {
            log.append(&[0u8; 8]).unwrap();
        }
        // wait for conductor to finish cleaning slot 1 before triggering rotation 2
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !log.conductor.ready.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "conductor timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // rotation 2: slot 2 → prev, slot 1 → current, slot 0 → standby
        log.append(&[0u8; 1]).unwrap();
        // slot 0 is standby → no longer readable
        assert_eq!(log.read(o0), None);
        cleanup(&b);
    }

    #[test]
    fn stale_offset_after_full_cycle_returns_none() {
        let b = base("gen");
        cleanup(&b);
        // footprint(8) = 16; 4 records per segment; segment_size = 64
        let mut log = open(&b, 64);
        // Write to slot 0 (gen 0)
        let stale = log.append(&[0u8; 8]).unwrap();
        for _ in 0..3 {
            log.append(&[0u8; 8]).unwrap();
        }
        // Rotation 1: slot 2 → current (gen 1), slot 0 → prev
        for _ in 0..4 {
            log.append(&[0u8; 8]).unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !log.conductor.ready.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "conductor timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Rotation 2: slot 1 → current (gen 2), slot 2 → prev, slot 0 → standby
        for _ in 0..4 {
            log.append(&[0u8; 8]).unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !log.conductor.ready.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline, "conductor timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Rotation 3: slot 0 → current (gen 3), slot 1 → prev, slot 2 → standby.
        // Slot 0 is current again — same slot index as `stale` — but gen 3 != gen 0.
        log.append(&[0u8; 8]).unwrap();
        assert_eq!(log.read(stale), None);
        cleanup(&b);
    }

    #[test]
    fn record_too_large_rejected() {
        let b = base("large");
        cleanup(&b);
        let mut log = open(&b, 64);
        assert!(log.append(&[0u8; 1024]).is_err());
        cleanup(&b);
    }

    #[test]
    fn empty_payload_roundtrip() {
        // footprint(0) = 8, which is FRAME_HDR — valid
        let b = base("empty");
        cleanup(&b);
        let mut log = open(&b, 1 << 16);
        let off = log.append(&[]).unwrap();
        assert_eq!(log.read(off), Some(b"".as_ref()));
        cleanup(&b);
    }
}
