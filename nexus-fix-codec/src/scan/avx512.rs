//! AVX-512 byte scanning (64 bytes at a time).
//!
//! Available when compiled with `-C target-feature=+avx512bw`.
//! Uses native mask registers — no `movemask` indirection needed.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use super::sse2;

/// Find the next occurrence of `needle` at or after `pos`.
///
/// Uses `_mm512_cmpeq_epi8_mask` to compare 64 bytes per iteration
/// with a native mask register result. Cascades to SSE2 for the tail.
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn find_byte(buf: &[u8], pos: usize, needle: u8) -> Option<usize> {
    let bytes = buf.get(pos..)?;
    if bytes.is_empty() {
        return None;
    }

    let mut i = 0;

    // SAFETY: AVX-512BW availability guaranteed by target_feature cfg
    unsafe {
        let target = _mm512_set1_epi8(needle as i8);

        while i + 64 <= bytes.len() {
            let chunk = _mm512_loadu_si512(bytes.as_ptr().add(i).cast());
            let mask = _mm512_cmpeq_epi8_mask(chunk, target);
            if mask != 0 {
                let offset = mask.trailing_zeros() as usize;
                return Some(pos + i + offset);
            }
            i += 64;
        }
    }

    if i < bytes.len() {
        return sse2::find_byte(buf, pos + i, needle);
    }

    None
}
