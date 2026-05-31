//! SSE2 byte scanning (16 bytes at a time).
//!
//! Available on all x86_64 targets (SSE2 is baseline).

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use super::scalar;

/// Find the next occurrence of `needle` at or after `pos`.
///
/// Uses `_mm_cmpeq_epi8` + `_mm_movemask_epi8` to search 16 bytes
/// per iteration. Cascades to scalar SWAR for the tail.
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn find_byte(buf: &[u8], pos: usize, needle: u8) -> Option<usize> {
    let bytes = buf.get(pos..)?;
    if bytes.is_empty() {
        return None;
    }

    let mut i = 0;

    // SAFETY: SSE2 is baseline for x86_64
    unsafe {
        let target = _mm_set1_epi8(needle as i8);

        while i + 16 <= bytes.len() {
            let chunk = _mm_loadu_si128(bytes.as_ptr().add(i).cast());
            let cmp = _mm_cmpeq_epi8(chunk, target);
            let mask = _mm_movemask_epi8(cmp);
            if mask != 0 {
                let offset = mask.trailing_zeros() as usize;
                return Some(pos + i + offset);
            }
            i += 16;
        }
    }

    if i < bytes.len() {
        return scalar::find_byte(buf, pos + i, needle);
    }

    None
}
