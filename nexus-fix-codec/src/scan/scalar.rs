//! Scalar SWAR (SIMD Within A Register) byte scanning.
//!
//! Searches for a target byte 8 bytes at a time using u64 arithmetic.
//! This is the fallback for non-x86_64 architectures and handles
//! the tail bytes after SIMD loops.

const HI: u64 = 0x8080_8080_8080_8080;
const LO: u64 = 0x0101_0101_0101_0101;

/// Find the next occurrence of `needle` at or after `pos`.
///
/// XORs each 8-byte chunk with a splat of the needle, then applies
/// SWAR zero-byte detection: a zero byte in the XOR result means
/// the corresponding input byte matched the needle.
#[inline]
pub fn find_byte(buf: &[u8], pos: usize, needle: u8) -> Option<usize> {
    let bytes = buf.get(pos..)?;
    if bytes.is_empty() {
        return None;
    }

    let splat = LO.wrapping_mul(needle as u64);
    let mut i = 0;

    while i + 8 <= bytes.len() {
        // SAFETY: bounds checked by the while condition
        let chunk: [u8; 8] = unsafe { bytes.as_ptr().add(i).cast::<[u8; 8]>().read_unaligned() };
        let word = u64::from_ne_bytes(chunk);
        let xored = word ^ splat;
        let mask = xored.wrapping_sub(LO) & !xored & HI;
        if mask != 0 {
            let offset = (mask.trailing_zeros() / 8) as usize;
            return Some(pos + i + offset);
        }
        i += 8;
    }

    while i < bytes.len() {
        if bytes[i] == needle {
            return Some(pos + i);
        }
        i += 1;
    }

    None
}
