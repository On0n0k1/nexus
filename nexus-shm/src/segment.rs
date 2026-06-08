use std::num::NonZeroUsize;
use std::path::Path;

use nexus_platform::{Liveness, ProcessLease};

use crate::control::{ControlBlock, status};
use crate::error::ShmError;
use crate::region::{MapOptions, Mapping};

const HEADER: NonZeroUsize = match NonZeroUsize::new(size_of::<ControlBlock>()) {
    Some(n) => n,
    None => panic!("control block is zero-sized"),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Uninit,
    Alive,
    Dead,
}

/// A mapped shared-memory segment: a control block followed by a payload.
///
/// File lifecycle (unlink, rotate, archive) is the caller's responsibility. The
/// backing file persists after drop so a restarting peer can run crash
/// recovery; segment owns only the mapping and its liveness signals.
pub struct Segment {
    mapping: Mapping,
    creator: bool,
}

impl Segment {
    pub fn create(path: &Path, data_len: usize, opts: MapOptions) -> Result<Self, ShmError> {
        if data_len == 0 {
            return Err(ShmError::EmptySegment);
        }
        let total = HEADER.checked_add(data_len).ok_or(ShmError::SizeOverflow)?;
        let mapping = Mapping::create(path, total, opts)?;

        if !ProcessLease::claim(mapping.as_fd())? {
            return Err(ShmError::OwnerActive);
        }

        // SAFETY: we hold the OFD owner lock acquired above, so no other process
        // is writing the control block; mmap returns page-aligned memory (hence
        // ControlBlock-aligned) covering at least the header.
        let cb = unsafe { &mut *mapping.as_ptr().cast::<ControlBlock>() };
        let generation = cb.generation().wrapping_add(1);
        cb.write_header(flags(opts), generation, std::process::id(), data_len as u64);

        Ok(Self {
            mapping,
            creator: true,
        })
    }

    pub fn attach(path: &Path, opts: MapOptions) -> Result<Self, ShmError> {
        let mapping = Mapping::open(path, opts)?;
        Self::control_of(&mapping).validate()?;
        Ok(Self {
            mapping,
            creator: false,
        })
    }

    /// Tier-1 liveness from the atomic status field.
    ///
    /// `Dead` is authoritative. `Alive` may be stale if the owner died without
    /// running its drop guard (`SIGKILL`, `panic=abort`); confirm with
    /// [`Segment::peer_liveness`], which consults the kernel-held OFD lock.
    pub fn status(&self) -> Status {
        match self.control().status() {
            s if s == status::ALIVE => Status::Alive,
            s if s == status::DEAD => Status::Dead,
            _ => Status::Uninit,
        }
    }

    pub fn peer_liveness(&self) -> Liveness {
        ProcessLease::probe(self.mapping.as_fd())
    }

    /// Pointer to the payload region, valid for [`Segment::data_len`] bytes for
    /// as long as this `Segment` lives.
    ///
    /// Reads and writes through it must be synchronized by the caller — the
    /// foundation provides no ordering for the payload (the control block's
    /// atomics cover only liveness). The primitives built on top supply their
    /// own sequencing.
    pub fn data(&self) -> *mut u8 {
        // SAFETY: the mapping is HEADER + data_len bytes, so HEADER is in bounds.
        unsafe { self.mapping.as_ptr().add(HEADER.get()) }
    }

    pub fn data_len(&self) -> usize {
        self.control().data_len() as usize
    }

    fn control(&self) -> &ControlBlock {
        Self::control_of(&self.mapping)
    }

    fn control_of(mapping: &Mapping) -> &ControlBlock {
        // SAFETY: mmap maps whole pages, so the control block (<= one page) is
        // mapped and page-aligned. All control-block fields are atomic or
        // written once before sharing, so shared `&` access is sound.
        unsafe { &*mapping.as_ptr().cast::<ControlBlock>() }
    }
}

impl Drop for Segment {
    fn drop(&mut self) {
        if self.creator {
            self.control().mark_dead();
        }
    }
}

fn flags(opts: MapOptions) -> u16 {
    u16::from(opts.pretouch) | (u16::from(opts.huge_pages) << 1)
}

#[cfg(test)]
mod tests {
    use super::{Liveness, Segment, Status};
    use crate::error::ShmError;
    use crate::region::MapOptions;

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("nexus-shm-{}-{}", std::process::id(), name))
    }

    #[test]
    fn create_attach_roundtrip() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);

        let seg = Segment::create(&path, 4096, MapOptions::default()).unwrap();
        assert_eq!(seg.data_len(), 4096);
        assert_eq!(seg.status(), Status::Alive);

        unsafe { seg.data().write(0xAB) };

        let peer = Segment::attach(&path, MapOptions::default()).unwrap();
        assert_eq!(peer.data_len(), 4096);
        assert_eq!(peer.status(), Status::Alive);
        assert_eq!(unsafe { peer.data().read() }, 0xAB);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn creator_drop_marks_dead() {
        let path = temp_path("dead");
        let _ = std::fs::remove_file(&path);

        let seg = Segment::create(&path, 64, MapOptions::default()).unwrap();
        drop(seg);

        let peer = Segment::attach(&path, MapOptions::default()).unwrap();
        assert_eq!(peer.status(), Status::Dead);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_zero_data_len() {
        let path = temp_path("zero");
        let _ = std::fs::remove_file(&path);
        assert!(matches!(
            Segment::create(&path, 0, MapOptions::default()),
            Err(ShmError::EmptySegment)
        ));
    }

    #[test]
    fn kernel_liveness_tracks_owner() {
        let path = temp_path("liveness");
        let _ = std::fs::remove_file(&path);

        let owner = Segment::create(&path, 64, MapOptions::default()).unwrap();
        let peer = Segment::attach(&path, MapOptions::default()).unwrap();
        assert_eq!(peer.peer_liveness(), Liveness::Alive);

        drop(owner);
        assert_eq!(peer.peer_liveness(), Liveness::Dead);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn rejects_foreign_file() {
        let path = temp_path("foreign");
        std::fs::write(&path, vec![0u8; 4096]).unwrap();

        assert!(Segment::attach(&path, MapOptions::default()).is_err());

        std::fs::remove_file(&path).unwrap();
    }
}
