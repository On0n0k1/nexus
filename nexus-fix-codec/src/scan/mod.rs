//! SIMD-accelerated byte scanning for FIX delimiters.
//!
//! Provides SOH (`\x01`) and `=` byte search with automatic dispatch
//! to the best available implementation at compile time:
//!
//! - AVX-512 if `target_feature = "avx512bw"` (64 bytes/iter)
//! - AVX2 if `target_feature = "avx2"` (32 bytes/iter)
//! - SSE2 on x86_64 (16 bytes/iter, always available)
//! - Scalar SWAR on other architectures (8 bytes/iter)
//!
//! Two API styles:
//! - [`find_soh`] / [`find_eq`] — single-result lookup
//! - [`soh_iter`] / [`eq_iter`] — iterator with SIMD mask caching

mod scalar;

#[cfg(target_arch = "x86_64")]
mod sse2;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
mod avx2;

#[cfg(all(target_arch = "x86_64", target_feature = "avx512bw"))]
mod avx512;

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

// =============================================================================
// Single-result lookups
// =============================================================================

/// Find the next SOH byte (`\x01`) at or after `pos`.
///
/// SOH is the FIX field delimiter. Every field access starts with
/// finding the next SOH.
///
/// Returns the absolute offset into `buf`, or `None` if no SOH found.
#[inline]
pub fn find_soh(buf: &[u8], pos: usize) -> Option<usize> {
    find_byte(buf, pos, 0x01)
}

/// Find the next `=` byte at or after `pos`.
///
/// Used for tag=value separation in FIX fields.
///
/// Returns the absolute offset into `buf`, or `None` if no `=` found.
#[inline]
pub fn find_eq(buf: &[u8], pos: usize) -> Option<usize> {
    find_byte(buf, pos, b'=')
}

#[inline]
fn find_byte(buf: &[u8], pos: usize, needle: u8) -> Option<usize> {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx512bw"))]
    {
        avx512::find_byte(buf, pos, needle)
    }

    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx2",
        not(target_feature = "avx512bw")
    ))]
    {
        avx2::find_byte(buf, pos, needle)
    }

    #[cfg(all(target_arch = "x86_64", not(target_feature = "avx2")))]
    {
        sse2::find_byte(buf, pos, needle)
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        scalar::find_byte(buf, pos, needle)
    }
}

// =============================================================================
// Iterator API — caches SIMD masks across calls
// =============================================================================

/// Iterator over positions of a delimiter byte in a buffer.
///
/// Caches SIMD comparison results so that multiple matches within
/// one chunk are yielded without re-scanning. On x86_64, a single
/// 16/32/64-byte SIMD comparison may find several delimiters — all
/// are drained before the next chunk is loaded.
///
/// Created via [`soh_iter`] or [`eq_iter`].
pub struct DelimiterScanner<'a> {
    buf: &'a [u8],
    pos: usize,
    needle: u8,
    #[cfg(target_arch = "x86_64")]
    mask: u64,
    #[cfg(target_arch = "x86_64")]
    mask_base: usize,
}

/// Iterate over all SOH (`\x01`) positions in `buf` starting from `pos`.
///
/// ```
/// use nexus_fix_codec::scan;
///
/// let msg = b"8=FIX.4.4\x0135=D\x0149=SENDER\x01";
/// let positions: Vec<usize> = scan::soh_iter(msg, 0).collect();
/// assert_eq!(positions, vec![9, 14, 24]);
/// ```
#[inline]
pub fn soh_iter(buf: &[u8], pos: usize) -> DelimiterScanner<'_> {
    DelimiterScanner::new(buf, pos, 0x01)
}

/// Iterate over all `=` positions in `buf` starting from `pos`.
#[inline]
pub fn eq_iter(buf: &[u8], pos: usize) -> DelimiterScanner<'_> {
    DelimiterScanner::new(buf, pos, b'=')
}

impl<'a> DelimiterScanner<'a> {
    #[inline]
    fn new(buf: &'a [u8], pos: usize, needle: u8) -> Self {
        Self {
            buf,
            pos,
            needle,
            #[cfg(target_arch = "x86_64")]
            mask: 0,
            #[cfg(target_arch = "x86_64")]
            mask_base: 0,
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn emit_mask(&mut self, mask: u64, chunk_offset: usize, chunk_size: usize) -> usize {
        self.mask_base = self.pos + chunk_offset;
        self.pos = self.pos + chunk_offset + chunk_size;
        self.mask = mask;
        let bit = self.mask.trailing_zeros() as usize;
        self.mask &= self.mask - 1;
        self.mask_base + bit
    }
}

#[cfg(target_arch = "x86_64")]
impl Iterator for DelimiterScanner<'_> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<usize> {
        if self.mask != 0 {
            let bit = self.mask.trailing_zeros() as usize;
            self.mask &= self.mask - 1;
            return Some(self.mask_base + bit);
        }

        let bytes = self.buf.get(self.pos..)?;
        if bytes.is_empty() {
            return None;
        }

        let mut i = 0;

        #[cfg(target_feature = "avx512bw")]
        {
            // SAFETY: avx512bw availability guaranteed by cfg
            unsafe {
                let target = _mm512_set1_epi8(self.needle as i8);
                while i + 64 <= bytes.len() {
                    let chunk = _mm512_loadu_si512(bytes.as_ptr().add(i).cast());
                    let m = _mm512_cmpeq_epi8_mask(chunk, target);
                    if m != 0 {
                        return Some(self.emit_mask(m, i, 64));
                    }
                    i += 64;
                }
            }
        }

        #[cfg(target_feature = "avx2")]
        {
            // SAFETY: avx2 availability guaranteed by cfg
            unsafe {
                let target = _mm256_set1_epi8(self.needle as i8);
                while i + 32 <= bytes.len() {
                    let chunk = _mm256_loadu_si256(bytes.as_ptr().add(i).cast());
                    let cmp = _mm256_cmpeq_epi8(chunk, target);
                    let m = _mm256_movemask_epi8(cmp) as u32 as u64;
                    if m != 0 {
                        return Some(self.emit_mask(m, i, 32));
                    }
                    i += 32;
                }
            }
        }

        // SSE2 is baseline on x86_64
        // SAFETY: SSE2 always available on x86_64
        unsafe {
            let target = _mm_set1_epi8(self.needle as i8);
            while i + 16 <= bytes.len() {
                let chunk = _mm_loadu_si128(bytes.as_ptr().add(i).cast());
                let cmp = _mm_cmpeq_epi8(chunk, target);
                let m = _mm_movemask_epi8(cmp) as u32 as u64;
                if m != 0 {
                    return Some(self.emit_mask(m, i, 16));
                }
                i += 16;
            }
        }

        // Scalar tail (< 16 bytes remaining)
        while i < bytes.len() {
            if bytes[i] == self.needle {
                let result = self.pos + i;
                self.pos = result + 1;
                return Some(result);
            }
            i += 1;
        }

        self.pos = self.buf.len();
        None
    }
}

#[cfg(not(target_arch = "x86_64"))]
impl Iterator for DelimiterScanner<'_> {
    type Item = usize;

    #[inline]
    fn next(&mut self) -> Option<usize> {
        let result = scalar::find_byte(self.buf, self.pos, self.needle)?;
        self.pos = result + 1;
        Some(result)
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // find_soh
    // =========================================================================

    #[test]
    fn find_soh_empty() {
        assert_eq!(find_soh(b"", 0), None);
    }

    #[test]
    fn find_soh_not_found() {
        assert_eq!(find_soh(b"8=FIX.4.4", 0), None);
    }

    #[test]
    fn find_soh_at_start() {
        assert_eq!(find_soh(b"\x018=FIX", 0), Some(0));
    }

    #[test]
    fn find_soh_at_end() {
        assert_eq!(find_soh(b"8=FIX.4.4\x01", 0), Some(9));
    }

    #[test]
    fn find_soh_with_offset() {
        let msg = b"8=FIX.4.4\x0135=D\x01";
        assert_eq!(find_soh(msg, 0), Some(9));
        assert_eq!(find_soh(msg, 10), Some(14));
    }

    #[test]
    fn find_soh_pos_past_end() {
        assert_eq!(find_soh(b"hello", 100), None);
    }

    #[test]
    fn find_soh_pos_at_end() {
        assert_eq!(find_soh(b"hello", 5), None);
    }

    // =========================================================================
    // find_eq
    // =========================================================================

    #[test]
    fn find_eq_empty() {
        assert_eq!(find_eq(b"", 0), None);
    }

    #[test]
    fn find_eq_not_found() {
        assert_eq!(find_eq(b"hello world", 0), None);
    }

    #[test]
    fn find_eq_basic() {
        assert_eq!(find_eq(b"35=D", 0), Some(2));
    }

    #[test]
    fn find_eq_with_offset() {
        let msg = b"8=FIX.4.4\x0135=D\x01";
        assert_eq!(find_eq(msg, 0), Some(1));
        assert_eq!(find_eq(msg, 2), Some(12));
    }

    // =========================================================================
    // Long buffer tests (exercise SIMD paths)
    // =========================================================================

    #[test]
    fn find_soh_long_buffer() {
        let mut buf = vec![b'A'; 64];
        buf.push(0x01);
        assert_eq!(find_soh(&buf, 0), Some(64));
    }

    #[test]
    fn find_soh_at_every_position() {
        for soh_pos in 0..65 {
            let mut buf = vec![b'A'; 65];
            buf[soh_pos] = 0x01;
            assert_eq!(
                find_soh(&buf, 0),
                Some(soh_pos),
                "expected SOH at position {}",
                soh_pos
            );
        }
    }

    #[test]
    fn find_eq_at_every_position() {
        for eq_pos in 0..65 {
            let mut buf = vec![b'A'; 65];
            buf[eq_pos] = b'=';
            assert_eq!(
                find_eq(&buf, 0),
                Some(eq_pos),
                "expected = at position {}",
                eq_pos
            );
        }
    }

    #[test]
    fn find_soh_with_varying_offsets() {
        let mut buf = vec![b'A'; 64];
        buf[50] = 0x01;

        for start in 0..=50 {
            assert_eq!(
                find_soh(&buf, start),
                Some(50),
                "start={}, expected SOH at 50",
                start
            );
        }
        for start in 51..64 {
            assert_eq!(
                find_soh(&buf, start),
                None,
                "start={}, expected None",
                start
            );
        }
    }

    // =========================================================================
    // Cross-implementation consistency (scalar vs dispatch)
    // =========================================================================

    #[test]
    fn scalar_matches_dispatch_soh() {
        for len in 0..=128 {
            for soh_pos in 0..len {
                let mut buf = vec![b'X'; len];
                buf[soh_pos] = 0x01;
                let expected = scalar::find_byte(&buf, 0, 0x01);
                let actual = find_soh(&buf, 0);
                assert_eq!(actual, expected, "len={}, soh_pos={}", len, soh_pos);
            }
        }
    }

    #[test]
    fn scalar_matches_dispatch_eq() {
        for len in 0..=128 {
            for eq_pos in 0..len {
                let mut buf = vec![b'X'; len];
                buf[eq_pos] = b'=';
                let expected = scalar::find_byte(&buf, 0, b'=');
                let actual = find_eq(&buf, 0);
                assert_eq!(actual, expected, "len={}, eq_pos={}", len, eq_pos);
            }
        }
    }

    #[test]
    fn no_match_all_lengths() {
        for len in 0..=128 {
            let buf = vec![b'X'; len];
            assert_eq!(find_soh(&buf, 0), None, "soh len={}", len);
            assert_eq!(find_eq(&buf, 0), None, "eq len={}", len);
        }
    }

    // =========================================================================
    // Iterator tests
    // =========================================================================

    #[test]
    fn soh_iter_empty() {
        let positions: Vec<usize> = soh_iter(b"", 0).collect();
        assert!(positions.is_empty());
    }

    #[test]
    fn soh_iter_no_match() {
        let positions: Vec<usize> = soh_iter(b"hello world", 0).collect();
        assert!(positions.is_empty());
    }

    #[test]
    fn soh_iter_single() {
        let positions: Vec<usize> = soh_iter(b"hello\x01", 0).collect();
        assert_eq!(positions, vec![5]);
    }

    #[test]
    fn soh_iter_multiple() {
        let msg = b"8=FIX.4.4\x0135=D\x0149=SENDER\x01";
        let positions: Vec<usize> = soh_iter(msg, 0).collect();
        assert_eq!(positions, vec![9, 14, 24]);
    }

    #[test]
    fn soh_iter_from_offset() {
        let msg = b"8=FIX.4.4\x0135=D\x0149=SENDER\x01";
        let positions: Vec<usize> = soh_iter(msg, 10).collect();
        assert_eq!(positions, vec![14, 24]);
    }

    #[test]
    fn soh_iter_consecutive() {
        let buf = b"\x01\x01\x01";
        let positions: Vec<usize> = soh_iter(buf, 0).collect();
        assert_eq!(positions, vec![0, 1, 2]);
    }

    #[test]
    fn eq_iter_basic() {
        let msg = b"8=FIX.4.4\x0135=D\x0149=SENDER\x01";
        let positions: Vec<usize> = eq_iter(msg, 0).collect();
        assert_eq!(positions, vec![1, 12, 17]);
    }

    #[test]
    fn soh_iter_matches_find_soh_loop() {
        let msg = b"8=FIX.4.4\x019=120\x0135=D\x0149=SENDER\x0156=TARGET\x01\
                     34=42\x0152=20260530-12:00:00\x0111=order1\x0155=BTC-USD\x01\
                     54=1\x0138=100\x0140=2\x0144=50000.00\x0110=123\x01";

        let iter_positions: Vec<usize> = soh_iter(msg, 0).collect();

        let mut loop_positions = Vec::new();
        let mut pos = 0;
        while let Some(soh) = find_soh(msg, pos) {
            loop_positions.push(soh);
            pos = soh + 1;
        }

        assert_eq!(iter_positions, loop_positions);
    }

    #[test]
    fn soh_iter_long_buffer_multiple_matches() {
        let mut buf = vec![b'A'; 128];
        let expected_positions = vec![10, 25, 40, 55, 70, 85, 100, 115];
        for &p in &expected_positions {
            buf[p] = 0x01;
        }

        let positions: Vec<usize> = soh_iter(&buf, 0).collect();
        assert_eq!(positions, expected_positions);
    }

    #[test]
    fn soh_iter_dense_matches_in_one_chunk() {
        let mut buf = vec![0x01u8; 16];
        let positions: Vec<usize> = soh_iter(&buf, 0).collect();
        let expected: Vec<usize> = (0..16).collect();
        assert_eq!(positions, expected);

        buf = vec![0x01u8; 32];
        let positions: Vec<usize> = soh_iter(&buf, 0).collect();
        let expected: Vec<usize> = (0..32).collect();
        assert_eq!(positions, expected);
    }

    #[test]
    fn iter_scalar_consistency_all_lengths() {
        for len in 0..=128 {
            for needle_pos in 0..len {
                let mut buf = vec![b'X'; len];
                buf[needle_pos] = 0x01;

                let iter_result: Vec<usize> = soh_iter(&buf, 0).collect();
                assert_eq!(
                    iter_result,
                    vec![needle_pos],
                    "len={}, pos={}",
                    len,
                    needle_pos
                );
            }
        }
    }

    #[test]
    fn iter_multiple_needles_all_lengths() {
        for len in 1..=64 {
            let buf = vec![0x01u8; len];
            let positions: Vec<usize> = soh_iter(&buf, 0).collect();
            let expected: Vec<usize> = (0..len).collect();
            assert_eq!(positions, expected, "len={}", len);
        }
    }

    // =========================================================================
    // Realistic FIX message
    // =========================================================================

    #[test]
    fn scan_realistic_fix_message() {
        let msg = b"8=FIX.4.4\x019=65\x0135=D\x0149=SENDER\x0156=TARGET\x01\
                     34=1\x0152=20260530-12:00:00\x0111=order1\x0155=BTC-USD\x01\
                     54=1\x0138=100\x0140=2\x0144=50000.00\x0110=123\x01";

        let field_count = soh_iter(msg, 0).count();
        assert_eq!(field_count, 14);
    }

    #[test]
    fn find_eq_in_fix_field() {
        let field = b"44=50000.00\x01";
        assert_eq!(find_eq(field, 0), Some(2));
        assert_eq!(find_soh(field, 0), Some(11));
    }
}
