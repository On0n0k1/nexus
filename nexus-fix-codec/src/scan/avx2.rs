//! AVX2 byte scanning (32 bytes at a time).
//!
//! Available when compiled with `-C target-feature=+avx2`.

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use super::sse2;

/// Find the next occurrence of `needle` at or after `pos`.
///
/// Uses `_mm256_cmpeq_epi8` + `_mm256_movemask_epi8` to search
/// 32 bytes per iteration. Cascades to SSE2 for the tail.
#[inline]
#[cfg(target_arch = "x86_64")]
pub fn find_byte(buf: &[u8], pos: usize, needle: u8) -> Option<usize> {
    let bytes = buf.get(pos..)?;
    if bytes.is_empty() {
        return None;
    }

    let mut i = 0;

    // SAFETY: AVX2 availability guaranteed by target_feature cfg
    unsafe {
        let target = _mm256_set1_epi8(needle as i8);

        while i + 32 <= bytes.len() {
            let chunk = _mm256_loadu_si256(bytes.as_ptr().add(i).cast());
            let cmp = _mm256_cmpeq_epi8(chunk, target);
            let mask = _mm256_movemask_epi8(cmp);
            if mask != 0 {
                let offset = mask.trailing_zeros() as usize;
                return Some(pos + i + offset);
            }
            i += 32;
        }
    }

    if i < bytes.len() {
        return sse2::find_byte(buf, pos + i, needle);
    }

    None
}
