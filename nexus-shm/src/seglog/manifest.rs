use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ShmError;
use crate::region::{MapOptions, Mapping};

const MAGIC: u32 = u32::from_le_bytes(*b"NXLG");
const VERSION: u16 = 1;

pub(crate) const SESSION_NAME_LEN: usize = 64;

/// On-disk manifest header. Mmap'd at the start of `journal.manifest`.
///
/// `epoch` is updated on each rotation via an atomic store. All other fields
/// are written once at creation and never change.
#[repr(C)]
struct ManifestHeader {
    magic: u32,
    version: u16,
    _pad: u16,
    segment_size: u64,
    session_id: u32,
    name_len: u8,
    _pad2: [u8; 3],
    name: [u8; SESSION_NAME_LEN],
    epoch: AtomicU64,
}

const _: () = {
    assert!(size_of::<ManifestHeader>() == 96);
    assert!(align_of::<ManifestHeader>() == 8);
};

const MANIFEST_FILE_SIZE: usize = 4096;

pub(crate) struct Manifest {
    mapping: Mapping,
}

impl Manifest {
    pub(crate) fn create(
        path: &Path,
        segment_size: u64,
        session_id: u32,
        name: &[u8],
    ) -> Result<Self, ShmError> {
        let len = NonZeroUsize::new(MANIFEST_FILE_SIZE).unwrap();
        let mapping = Mapping::create(path, len, MapOptions::default())?;

        // SAFETY: the mapping covers at least MANIFEST_FILE_SIZE bytes and is
        // page-aligned. We hold exclusive access (just created the file).
        let hdr = unsafe { &mut *mapping.as_ptr().cast::<ManifestHeader>() };
        hdr.magic = MAGIC;
        hdr.version = VERSION;
        hdr._pad = 0;
        hdr.segment_size = segment_size;
        hdr.session_id = session_id;
        let n = name.len().min(SESSION_NAME_LEN);
        hdr.name_len = n as u8;
        hdr._pad2 = [0; 3];
        hdr.name = [0; SESSION_NAME_LEN];
        hdr.name[..n].copy_from_slice(&name[..n]);
        *hdr.epoch.get_mut() = 0;

        Ok(Self { mapping })
    }

    pub(crate) fn open(path: &Path) -> Result<Self, ShmError> {
        let mapping = Mapping::open(path, MapOptions::default())?;
        let hdr = Self::header_of(&mapping);

        if hdr.magic != MAGIC {
            return Err(ShmError::BadMagic { found: hdr.magic });
        }
        if hdr.version != VERSION {
            return Err(ShmError::UnsupportedLayout {
                found: hdr.version,
                expected: VERSION,
            });
        }

        Ok(Self { mapping })
    }

    pub(crate) fn segment_size(&self) -> u64 {
        self.header().segment_size
    }

    pub(crate) fn session_id(&self) -> u32 {
        self.header().session_id
    }

    pub(crate) fn name(&self) -> &[u8] {
        let hdr = self.header();
        let n = hdr.name_len as usize;
        &hdr.name[..n]
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.header().epoch.load(Ordering::Acquire)
    }

    pub(crate) fn set_epoch(&self, epoch: u64) {
        self.header().epoch.store(epoch, Ordering::Release);
    }

    fn header(&self) -> &ManifestHeader {
        Self::header_of(&self.mapping)
    }

    fn header_of(mapping: &Mapping) -> &ManifestHeader {
        // SAFETY: the mapping is at least MANIFEST_FILE_SIZE bytes and
        // page-aligned. ManifestHeader is 96 bytes with 8-byte alignment.
        unsafe { &*mapping.as_ptr().cast::<ManifestHeader>() }
    }
}
