//! Parsing helpers for typed FIX field values.

/// Parse a FIX integer (`INT`, `SEQNUM`) value. Returns `None` on empty
/// input, non-digit bytes, or overflow.
#[inline]
pub fn parse_fix_int(bytes: &[u8]) -> Option<i64> {
    let (neg, digits) = match bytes.first()? {
        b'-' => (true, &bytes[1..]),
        b'+' => (false, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    let mut acc: i64 = 0;
    for &b in digits {
        let d = b.wrapping_sub(b'0');
        if d > 9 {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add(d as i64)?;
    }
    Some(if neg { -acc } else { acc })
}

/// Parse a FIX unsigned integer (`LENGTH`, `NUMINGROUP`) value. Returns
/// `None` on empty input, non-digit bytes, or overflow.
#[inline]
pub fn parse_fix_uint(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for &b in bytes {
        let d = b.wrapping_sub(b'0');
        if d > 9 {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add(d as u32)?;
    }
    Some(acc)
}

/// Format an unsigned integer into `buf` as ASCII digits, returning the
/// written slice. `buf` of 20 bytes holds any `u64`. Allocation-free.
#[inline]
pub fn format_uint(buf: &mut [u8; 20], mut v: u64) -> &[u8] {
    let mut p = buf.len();
    loop {
        p -= 1;
        buf[p] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    &buf[p..]
}

/// Parse a FIX boolean (`BOOLEAN`) value: `Y` is true, `N` is false.
#[inline]
pub fn parse_fix_bool(bytes: &[u8]) -> Option<bool> {
    match bytes {
        b"Y" => Some(true),
        b"N" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints() {
        assert_eq!(parse_fix_int(b"0"), Some(0));
        assert_eq!(parse_fix_int(b"42"), Some(42));
        assert_eq!(parse_fix_int(b"-17"), Some(-17));
        assert_eq!(parse_fix_int(b"+5"), Some(5));
        assert_eq!(parse_fix_int(b""), None);
        assert_eq!(parse_fix_int(b"-"), None);
        assert_eq!(parse_fix_int(b"1a"), None);
    }

    #[test]
    fn uints() {
        assert_eq!(parse_fix_uint(b"123"), Some(123));
        assert_eq!(parse_fix_uint(b""), None);
        assert_eq!(parse_fix_uint(b"-1"), None);
        assert_eq!(parse_fix_uint(b"99999999999"), None);
    }

    #[test]
    fn uint_format() {
        let mut buf = [0u8; 20];
        assert_eq!(format_uint(&mut buf, 0), b"0");
        assert_eq!(format_uint(&mut buf, 7), b"7");
        assert_eq!(format_uint(&mut buf, 12345), b"12345");
        assert_eq!(format_uint(&mut buf, u64::from(u32::MAX)), b"4294967295");
    }

    #[test]
    fn bools() {
        assert_eq!(parse_fix_bool(b"Y"), Some(true));
        assert_eq!(parse_fix_bool(b"N"), Some(false));
        assert_eq!(parse_fix_bool(b"y"), None);
        assert_eq!(parse_fix_bool(b""), None);
    }
}
