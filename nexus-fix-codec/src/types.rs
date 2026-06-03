use core::fmt;

/// Parsed FIX decimal value (FLOAT, PRICE, QTY, AMT, PERCENTAGE, PRICEOFFSET).
///
/// Captures the wire representation without imposing a precision opinion.
/// `"123.456"` parses to `mantissa: 123_456, scale: 3`.
///
/// Convert to your preferred decimal type at the call site:
/// ```
/// # use nexus_fix_codec::FixDecimal;
/// let d = FixDecimal::parse(b"99.50").unwrap();
/// let price: f64 = d.into();
/// assert!((price - 99.5).abs() < f64::EPSILON);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FixDecimal {
    pub mantissa: i64,
    pub scale: u8,
}

impl FixDecimal {
    /// Parse a FIX decimal from wire bytes.
    ///
    /// Accepts: optional sign, digits, optional `.` + fractional digits.
    /// Returns `None` on empty input, non-digit characters, or overflow.
    ///
    /// Uses SWAR (SIMD Within A Register) to parse up to 8 ASCII digits
    /// in parallel per block — three multiply+shift stages vs one
    /// multiply-add per digit in the scalar loop.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }

        let (negative, start) = match bytes[0] {
            b'-' => (true, 1),
            b'+' => (false, 1),
            _ => (false, 0),
        };

        let src = &bytes[start..];
        if src.is_empty() {
            return None;
        }

        let dot_pos = src.iter().position(|&b| b == b'.');

        let (mantissa_u64, scale) = if let Some(dp) = dot_pos {
            let int_part = &src[..dp];
            let frac_part = &src[dp + 1..];
            if frac_part.is_empty() && int_part.is_empty() {
                return None;
            }
            let scale = frac_part.len() as u8;

            let int_val = if int_part.is_empty() {
                0u64
            } else {
                parse_unsigned_digits(int_part)?
            };

            let frac_val = if frac_part.is_empty() {
                0u64
            } else {
                parse_unsigned_digits(frac_part)?
            };

            let scale_mul = 10u64.checked_pow(scale as u32)?;
            let mantissa = int_val.checked_mul(scale_mul)?.checked_add(frac_val)?;
            (mantissa, scale)
        } else {
            let val = parse_unsigned_digits(src)?;
            (val, 0u8)
        };

        let mantissa = if negative {
            let signed = mantissa_u64 as i128;
            let neg = -signed;
            if neg < i64::MIN as i128 {
                return None;
            }
            neg as i64
        } else {
            if mantissa_u64 > i64::MAX as u64 {
                return None;
            }
            mantissa_u64 as i64
        };

        Some(Self { mantissa, scale })
    }

    /// Encode this decimal to wire bytes.
    ///
    /// Writes the FIX representation (e.g., `"-123.456"`) into `buf` and
    /// returns the number of bytes written. Buffer must be at least 21 bytes.
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let mut pos = 0;

        if self.mantissa < 0 {
            buf[pos] = b'-';
            pos += 1;
        }

        let abs = self.mantissa.unsigned_abs();

        if self.scale == 0 {
            pos += encode_u64(abs, &mut buf[pos..]);
            return pos;
        }

        let scale_pow = 10u64.pow(self.scale as u32);
        let integer = abs / scale_pow;
        let frac = abs % scale_pow;

        pos += encode_u64(integer, &mut buf[pos..]);
        buf[pos] = b'.';
        pos += 1;
        encode_u64_padded(frac, self.scale as usize, &mut buf[pos..]);
        pos += self.scale as usize;

        pos
    }
}

impl From<FixDecimal> for f64 {
    #[inline]
    fn from(d: FixDecimal) -> Self {
        d.mantissa as f64 / 10_f64.powi(d.scale as i32)
    }
}

impl From<FixDecimal> for f32 {
    #[inline]
    fn from(d: FixDecimal) -> Self {
        d.mantissa as f32 / 10_f32.powi(d.scale as i32)
    }
}

impl fmt::Display for FixDecimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.mantissa);
        }
        let divisor = 10_i64.pow(self.scale as u32);
        let integer = self.mantissa / divisor;
        let frac = self.mantissa.unsigned_abs() % divisor as u64;
        if self.mantissa < 0 && integer == 0 {
            write!(f, "-0.{:0>width$}", frac, width = self.scale as usize)
        } else {
            write!(
                f,
                "{}.{:0>width$}",
                integer,
                frac,
                width = self.scale as usize
            )
        }
    }
}

/// Parsed FIX timestamp as nanos since unix epoch.
///
/// FIX timestamps are UTC by convention (`YYYYMMDD-HH:MM:SS[.sss[sss[sss]]]`).
///
/// ```
/// # use nexus_fix_codec::FixTimestamp;
/// let ts = FixTimestamp::parse(b"20260602-14:30:00.123456").unwrap();
/// assert_eq!(ts.subsec_nanos(), 123_456_000);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixTimestamp(pub i128);

impl FixTimestamp {
    const NANOS_PER_SEC: i128 = 1_000_000_000;
    const SECS_PER_DAY: i128 = 86400;

    /// Parse a FIX UTC timestamp: `YYYYMMDD-HH:MM:SS[.sss[sss[sss]]]`.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        // Minimum: YYYYMMDD-HH:MM:SS = 17 bytes
        if bytes.len() < 17 {
            return None;
        }

        let date = FixDate::parse(&bytes[..8])?;
        if bytes[8] != b'-' {
            return None;
        }
        let (time, _) = parse_time_of_day(&bytes[9..])?;

        let epoch_days = date.to_epoch_days()? as i128;
        let secs = epoch_days * Self::SECS_PER_DAY
            + time.nanos_since_midnight as i128 / Self::NANOS_PER_SEC;
        let sub_nanos = time.nanos_since_midnight as i128 % Self::NANOS_PER_SEC;

        Some(Self(secs * Self::NANOS_PER_SEC + sub_nanos))
    }

    /// Nanosecond value (nanos since unix epoch).
    #[inline]
    pub const fn as_nanos(self) -> i128 {
        self.0
    }

    /// Microseconds since unix epoch (truncates sub-microsecond).
    #[inline]
    pub const fn as_micros(self) -> i128 {
        self.0 / 1_000
    }

    /// Milliseconds since unix epoch (truncates sub-millisecond).
    #[inline]
    pub const fn as_millis(self) -> i128 {
        self.0 / 1_000_000
    }

    /// Seconds since unix epoch (truncates sub-second).
    #[inline]
    pub const fn as_secs(self) -> i64 {
        (self.0 / Self::NANOS_PER_SEC) as i64
    }

    /// Sub-second nanos component (0..999_999_999).
    #[inline]
    pub const fn subsec_nanos(self) -> u32 {
        (self.0.unsigned_abs() % Self::NANOS_PER_SEC as u128) as u32
    }

    /// Decompose into date and time-of-day components.
    pub fn decompose(self) -> (FixDate, FixTime) {
        let total_secs = self.0.div_euclid(Self::NANOS_PER_SEC);
        let sub_nanos = self.0.rem_euclid(Self::NANOS_PER_SEC) as u64;

        let epoch_days = total_secs.div_euclid(Self::SECS_PER_DAY) as i32;
        let secs_in_day = total_secs.rem_euclid(Self::SECS_PER_DAY) as u64;

        let date = FixDate::from_epoch_days(epoch_days);
        let time = FixTime {
            nanos_since_midnight: secs_in_day * FixTime::NANOS_PER_SEC + sub_nanos,
        };
        (date, time)
    }

    /// Encode as FIX timestamp wire bytes (`YYYYMMDD-HH:MM:SS[.fractional]`).
    ///
    /// Returns the number of bytes written. Buffer must be at least 27 bytes.
    /// Fractional precision is auto-detected: millis (3), micros (6), or nanos (9).
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        let (date, time) = self.decompose();
        let mut pos = date.encode(buf);
        buf[pos] = b'-';
        pos += 1;
        pos += time.encode(&mut buf[pos..]);
        pos
    }
}

impl fmt::Display for FixTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

/// Parsed FIX date (`YYYYMMDD`, UTC by convention).
///
/// ```
/// # use nexus_fix_codec::FixDate;
/// let d = FixDate::parse(b"20260602").unwrap();
/// assert_eq!(d.year, 2026);
/// assert_eq!(d.month, 6);
/// assert_eq!(d.day, 2);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FixDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl FixDate {
    /// Parse `YYYYMMDD` from wire bytes.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }

        let year = parse_digits_u16(&bytes[..4])?;
        let month = parse_digits_u8(&bytes[4..6])?;
        let day = parse_digits_u8(&bytes[6..8])?;

        if month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }

        Some(Self { year, month, day })
    }

    /// Days since unix epoch (1970-01-01). Returns `None` for dates before epoch.
    pub fn to_epoch_days(&self) -> Option<i32> {
        // Rata Die algorithm (Howard Hinnant)
        let y = if self.month <= 2 {
            self.year as i32 - 1
        } else {
            self.year as i32
        };
        let m = if self.month <= 2 {
            self.month as i32 + 9
        } else {
            self.month as i32 - 3
        };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * m + 2) / 5 + self.day as i32 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(days)
    }

    /// Construct a date from days since unix epoch (1970-01-01).
    ///
    /// Inverse of [`to_epoch_days`](Self::to_epoch_days). Uses the Hinnant
    /// civil-from-days algorithm.
    pub fn from_epoch_days(days: i32) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe as i32 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        Self {
            year: y as u16,
            month: m as u8,
            day: d as u8,
        }
    }

    /// Encode as `YYYYMMDD` wire bytes. Always writes exactly 8 bytes.
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        encode_4_digits(buf, self.year);
        encode_2_digits(&mut buf[4..], self.month);
        encode_2_digits(&mut buf[6..], self.day);
        8
    }
}

impl fmt::Display for FixDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

/// Parsed FIX time of day (`HH:MM:SS[.sss[sss[sss]]]`, UTC by convention).
///
/// ```
/// # use nexus_fix_codec::FixTime;
/// let t = FixTime::parse(b"14:30:00.500").unwrap();
/// assert_eq!(t.nanos_since_midnight, 52_200_500_000_000);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FixTime {
    pub nanos_since_midnight: u64,
}

impl FixTime {
    const NANOS_PER_SEC: u64 = 1_000_000_000;
    const NANOS_PER_MIN: u64 = 60 * Self::NANOS_PER_SEC;
    const NANOS_PER_HOUR: u64 = 3600 * Self::NANOS_PER_SEC;

    /// Parse `HH:MM:SS[.sss[sss[sss]]]` from wire bytes.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let (time, _) = parse_time_of_day(bytes)?;
        Some(time)
    }

    /// Hours component (0..23).
    #[inline]
    pub const fn hour(&self) -> u8 {
        (self.nanos_since_midnight / Self::NANOS_PER_HOUR) as u8
    }

    /// Minutes component (0..59).
    #[inline]
    pub const fn minute(&self) -> u8 {
        ((self.nanos_since_midnight % Self::NANOS_PER_HOUR) / Self::NANOS_PER_MIN) as u8
    }

    /// Seconds component (0..59).
    #[inline]
    pub const fn second(&self) -> u8 {
        ((self.nanos_since_midnight % Self::NANOS_PER_MIN) / Self::NANOS_PER_SEC) as u8
    }

    /// Sub-second nanos (0..999_999_999).
    #[inline]
    pub const fn subsec_nanos(&self) -> u32 {
        (self.nanos_since_midnight % Self::NANOS_PER_SEC) as u32
    }

    /// Encode as `HH:MM:SS[.sss[sss[sss]]]` wire bytes.
    ///
    /// Returns the number of bytes written (8, 12, 15, or 18).
    /// Fractional precision is auto-detected from the value.
    /// Buffer must be at least 18 bytes.
    pub fn encode(&self, buf: &mut [u8]) -> usize {
        encode_2_digits(buf, self.hour());
        buf[2] = b':';
        encode_2_digits(&mut buf[3..], self.minute());
        buf[5] = b':';
        encode_2_digits(&mut buf[6..], self.second());

        let sub = self.subsec_nanos();
        if sub == 0 {
            return 8;
        }

        buf[8] = b'.';

        if sub.is_multiple_of(1_000_000) {
            encode_u64_padded(sub as u64 / 1_000_000, 3, &mut buf[9..]);
            12
        } else if sub.is_multiple_of(1_000) {
            encode_u64_padded(sub as u64 / 1_000, 6, &mut buf[9..]);
            15
        } else {
            encode_u64_padded(sub as u64, 9, &mut buf[9..]);
            18
        }
    }
}

impl fmt::Display for FixTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sub = self.subsec_nanos();
        if sub == 0 {
            write!(
                f,
                "{:02}:{:02}:{:02}",
                self.hour(),
                self.minute(),
                self.second()
            )
        } else {
            write!(
                f,
                "{:02}:{:02}:{:02}.{:09}",
                self.hour(),
                self.minute(),
                self.second(),
                sub
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Tier 1 parsing helpers (used by generated code)
// ---------------------------------------------------------------------------

/// Parse a FIX integer field (INT type) from wire bytes.
///
/// Handles optional leading sign. Returns `None` on empty, non-digit, or overflow.
/// Uses SWAR for the digit portion.
pub fn parse_fix_int(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() {
        return None;
    }

    let (negative, start) = match bytes[0] {
        b'-' => (true, 1),
        b'+' => (false, 1),
        _ => (false, 0),
    };

    let digits = &bytes[start..];
    if digits.is_empty() {
        return None;
    }

    let unsigned = parse_unsigned_digits(digits)?;

    if negative {
        let signed = unsigned as i128;
        let neg = -signed;
        if neg < i64::MIN as i128 {
            return None;
        }
        Some(neg as i64)
    } else {
        if unsigned > i64::MAX as u64 {
            return None;
        }
        Some(unsigned as i64)
    }
}

/// Parse a FIX unsigned integer (LENGTH, NUMINGROUP) from wire bytes.
///
/// Uses SWAR for the digit portion.
pub fn parse_fix_uint(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() {
        return None;
    }
    let val = parse_unsigned_digits(bytes)?;
    u32::try_from(val).ok()
}

/// Parse a FIX sequence number (SEQNUM) from wire bytes.
///
/// Uses SWAR for the digit portion.
pub fn parse_fix_seqnum(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }
    parse_unsigned_digits(bytes)
}

/// Parse a FIX boolean (`Y` / `N`) from wire bytes.
#[inline]
pub fn parse_fix_bool(bytes: &[u8]) -> Option<bool> {
    match bytes {
        [b'Y'] => Some(true),
        [b'N'] => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tier 1 encoding helpers (used by generated code)
// ---------------------------------------------------------------------------

/// Encode a FIX integer field (INT type) to wire bytes.
///
/// Writes the decimal representation (with leading `-` for negatives).
/// Buffer must be at least 20 bytes. Returns the number of bytes written.
pub fn encode_fix_int(value: i64, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    if value < 0 {
        buf[pos] = b'-';
        pos += 1;
    }
    pos += encode_u64(value.unsigned_abs(), &mut buf[pos..]);
    pos
}

/// Encode a FIX unsigned integer (LENGTH, NUMINGROUP) to wire bytes.
///
/// Buffer must be at least 10 bytes. Returns the number of bytes written.
pub fn encode_fix_uint(value: u32, buf: &mut [u8]) -> usize {
    encode_u64(value as u64, buf)
}

/// Encode a FIX sequence number (SEQNUM) to wire bytes.
///
/// Buffer must be at least 20 bytes. Returns the number of bytes written.
pub fn encode_fix_seqnum(value: u64, buf: &mut [u8]) -> usize {
    encode_u64(value, buf)
}

/// Encode a FIX boolean as a single byte (`Y` or `N`).
#[inline]
pub fn encode_fix_bool(value: bool) -> u8 {
    if value { b'Y' } else { b'N' }
}

// ---------------------------------------------------------------------------
// SWAR digit parsing
// ---------------------------------------------------------------------------

/// Parse up to 8 ASCII digits in parallel using SWAR.
///
/// Digits are left-padded with '0' in an 8-byte register, then combined
/// pairwise: 8 single digits -> 4 two-digit pairs -> 2 four-digit values -> result.
/// Three multiply+shift stages vs 8 scalar multiply-add iterations.
#[inline]
fn swar_parse_8(digits: &[u8]) -> Option<u32> {
    debug_assert!(!digits.is_empty() && digits.len() <= 8);

    let mut buf = [b'0'; 8];
    buf[8 - digits.len()..].copy_from_slice(digits);

    let v = u64::from_le_bytes(buf).wrapping_sub(0x3030_3030_3030_3030);

    // Validate: every byte must be 0..=9. Adding 6 to any value >= 10
    // sets bits in the 0xF0 mask; values that wrapped (original < '0')
    // already have those bits set.
    let chk = v.wrapping_add(0x0606_0606_0606_0606);
    if (chk | v) & 0xF0F0_F0F0_F0F0_F0F0 != 0 {
        return None;
    }

    // Combine adjacent byte pairs: d0*10+d1, d2*10+d3, d4*10+d5, d6*10+d7
    let lo = v & 0x00FF_00FF_00FF_00FF;
    let hi = (v >> 8) & 0x00FF_00FF_00FF_00FF;
    let v = lo * 10 + hi;

    // Combine u16 pairs: pair0*100+pair1, pair2*100+pair3
    let lo = v & 0x0000_FFFF_0000_FFFF;
    let hi = (v >> 16) & 0x0000_FFFF_0000_FFFF;
    let v = lo * 100 + hi;

    // Combine u32 halves: lo*10000 + hi
    let lo = v as u32;
    let hi = (v >> 32) as u32;
    Some(lo * 10_000 + hi)
}

/// Parse up to 16 ASCII digits using two SWAR blocks.
#[inline]
fn swar_parse_16(digits: &[u8]) -> Option<u64> {
    debug_assert!(!digits.is_empty() && digits.len() <= 16);

    if digits.len() <= 8 {
        return swar_parse_8(digits).map(|v| v as u64);
    }

    let split = digits.len() - 8;
    let hi = swar_parse_8(&digits[..split])? as u64;
    let lo = swar_parse_8(&digits[split..])? as u64;
    Some(hi * 100_000_000 + lo)
}

/// Parse an unsigned digit string into u64. SWAR for <= 16 digits, scalar fallback for 17-19.
fn parse_unsigned_digits(digits: &[u8]) -> Option<u64> {
    if digits.is_empty() || digits.len() > 19 {
        return None;
    }
    if digits.len() <= 16 {
        return swar_parse_16(digits);
    }
    // 17-19 digits: parse leading scalar digits, then two SWAR blocks
    let leading = digits.len() - 16;
    let mut hi = 0u64;
    for &b in &digits[..leading] {
        match b {
            b'0'..=b'9' => hi = hi * 10 + (b - b'0') as u64,
            _ => return None,
        }
    }
    let lo = swar_parse_16(&digits[leading..])?;
    hi.checked_mul(10_000_000_000_000_000)?.checked_add(lo)
}

// ---------------------------------------------------------------------------
// Digit encoding helpers
// ---------------------------------------------------------------------------

const DIGIT_PAIRS: [u8; 200] = {
    let mut lut = [0u8; 200];
    let mut i = 0;
    while i < 100 {
        lut[i * 2] = b'0' + (i / 10) as u8;
        lut[i * 2 + 1] = b'0' + (i % 10) as u8;
        i += 1;
    }
    lut
};

#[inline]
fn encode_2_digits(buf: &mut [u8], value: u8) {
    let idx = value as usize * 2;
    buf[0] = DIGIT_PAIRS[idx];
    buf[1] = DIGIT_PAIRS[idx + 1];
}

#[inline]
fn encode_4_digits(buf: &mut [u8], value: u16) {
    encode_2_digits(buf, (value / 100) as u8);
    encode_2_digits(&mut buf[2..], (value % 100) as u8);
}

/// Encode a u64 as decimal ASCII. Returns the number of bytes written.
fn encode_u64(value: u64, buf: &mut [u8]) -> usize {
    if value == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut tmp = [0u8; 20];
    let mut pos = 20usize;
    let mut v = value;

    while v >= 100 {
        let rem = (v % 100) as usize;
        v /= 100;
        pos -= 2;
        tmp[pos] = DIGIT_PAIRS[rem * 2];
        tmp[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }

    if v >= 10 {
        pos -= 2;
        tmp[pos] = DIGIT_PAIRS[v as usize * 2];
        tmp[pos + 1] = DIGIT_PAIRS[v as usize * 2 + 1];
    } else {
        pos -= 1;
        tmp[pos] = b'0' + v as u8;
    }

    let len = 20 - pos;
    buf[..len].copy_from_slice(&tmp[pos..]);
    len
}

/// Encode a u64 as zero-padded decimal ASCII of exactly `width` digits.
fn encode_u64_padded(value: u64, width: usize, buf: &mut [u8]) {
    debug_assert!(width <= 20);
    let mut tmp = [b'0'; 20];
    let mut pos = 20usize;
    let mut v = value;

    while v > 0 {
        let rem = (v % 100) as usize;
        v /= 100;
        pos -= 2;
        tmp[pos] = DIGIT_PAIRS[rem * 2];
        tmp[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }

    buf[..width].copy_from_slice(&tmp[20 - width..]);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse `HH:MM:SS[.fractional]`, returning the time and bytes consumed.
fn parse_time_of_day(bytes: &[u8]) -> Option<(FixTime, usize)> {
    // Minimum: HH:MM:SS = 8 bytes
    if bytes.len() < 8 {
        return None;
    }

    let hour = parse_digits_u8(&bytes[..2])?;
    if bytes[2] != b':' {
        return None;
    }
    let minute = parse_digits_u8(&bytes[3..5])?;
    if bytes[5] != b':' {
        return None;
    }
    let second = parse_digits_u8(&bytes[6..8])?;

    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let mut nanos = hour as u64 * FixTime::NANOS_PER_HOUR
        + minute as u64 * FixTime::NANOS_PER_MIN
        + second as u64 * FixTime::NANOS_PER_SEC;

    let mut consumed = 8;

    // Optional fractional seconds: .sss, .ssssss, or .sssssssss
    if bytes.len() > 8 && bytes[8] == b'.' {
        consumed = 9;
        let mut frac: u64 = 0;
        let mut frac_digits: u32 = 0;

        for &b in &bytes[9..] {
            match b {
                b'0'..=b'9' if frac_digits < 9 => {
                    frac = frac * 10 + (b - b'0') as u64;
                    frac_digits += 1;
                    consumed += 1;
                }
                _ => break,
            }
        }

        // Scale to nanoseconds (pad with zeros if fewer than 9 digits)
        if frac_digits > 0 {
            while frac_digits < 9 {
                frac *= 10;
                frac_digits += 1;
            }
            nanos += frac;
        }
    }

    Some((
        FixTime {
            nanos_since_midnight: nanos,
        },
        consumed,
    ))
}

fn parse_digits_u16(bytes: &[u8]) -> Option<u16> {
    let mut value: u16 = 0;
    for &b in bytes {
        match b {
            b'0'..=b'9' => {
                value = value.checked_mul(10)?.checked_add((b - b'0') as u16)?;
            }
            _ => return None,
        }
    }
    Some(value)
}

fn parse_digits_u8(bytes: &[u8]) -> Option<u8> {
    let mut value: u8 = 0;
    for &b in bytes {
        match b {
            b'0'..=b'9' => {
                value = value.checked_mul(10)?.checked_add(b - b'0')?;
            }
            _ => return None,
        }
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// Feature-gated conversions
// ---------------------------------------------------------------------------

#[cfg(feature = "nexus-decimal")]
mod decimal_conv {
    use super::FixDecimal;
    use nexus_decimal::{Backing, Decimal};

    /// Error when a [`FixDecimal`] cannot be represented in the target decimal type.
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct DecimalConvError {
        pub mantissa: i64,
        pub scale: u8,
    }

    impl core::fmt::Display for DecimalConvError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                f,
                "cannot convert FixDecimal(mantissa={}, scale={}) to target decimal: overflow on rescale",
                self.mantissa, self.scale
            )
        }
    }

    impl std::error::Error for DecimalConvError {}

    // i128 backing: infallible — i128 always has headroom for i64 mantissa rescale.
    impl<const D: u8> From<FixDecimal> for Decimal<i128, D>
    where
        i128: Backing,
    {
        fn from(d: FixDecimal) -> Self {
            let mantissa = d.mantissa as i128;
            let scaled = if D >= d.scale {
                mantissa * 10_i128.pow((D - d.scale) as u32)
            } else {
                mantissa / 10_i128.pow((d.scale - D) as u32)
            };
            Self::from_raw(scaled)
        }
    }

    // i64 backing: fallible — rescale can overflow i64.
    impl<const D: u8> TryFrom<FixDecimal> for Decimal<i64, D>
    where
        i64: Backing,
    {
        type Error = DecimalConvError;

        fn try_from(d: FixDecimal) -> Result<Self, Self::Error> {
            let err = || DecimalConvError {
                mantissa: d.mantissa,
                scale: d.scale,
            };

            let scaled = if D >= d.scale {
                d.mantissa
                    .checked_mul(10_i64.pow((D - d.scale) as u32))
                    .ok_or_else(err)?
            } else {
                d.mantissa / 10_i64.pow((d.scale - D) as u32)
            };
            Ok(Self::from_raw(scaled))
        }
    }

    // i32 backing: fallible — even more constrained.
    impl<const D: u8> TryFrom<FixDecimal> for Decimal<i32, D>
    where
        i32: Backing,
    {
        type Error = DecimalConvError;

        fn try_from(d: FixDecimal) -> Result<Self, Self::Error> {
            let err = || DecimalConvError {
                mantissa: d.mantissa,
                scale: d.scale,
            };

            let scaled = if D >= d.scale {
                d.mantissa
                    .checked_mul(10_i64.pow((D - d.scale) as u32))
                    .ok_or_else(err)?
            } else {
                d.mantissa / 10_i64.pow((d.scale - D) as u32)
            };
            let narrow = i32::try_from(scaled).map_err(|_| err())?;
            Ok(Self::from_raw(narrow))
        }
    }

    // -- Reverse: Decimal → FixDecimal --

    // i64 backing → FixDecimal: infallible — i64 mantissa maps directly.
    impl<const D: u8> From<Decimal<i64, D>> for FixDecimal
    where
        i64: Backing,
    {
        fn from(d: Decimal<i64, D>) -> Self {
            Self {
                mantissa: d.to_raw(),
                scale: D,
            }
        }
    }

    // i32 backing → FixDecimal: infallible — i32 widens to i64.
    impl<const D: u8> From<Decimal<i32, D>> for FixDecimal
    where
        i32: Backing,
    {
        fn from(d: Decimal<i32, D>) -> Self {
            Self {
                mantissa: d.to_raw() as i64,
                scale: D,
            }
        }
    }

    /// Error when a decimal mantissa exceeds i64 range for [`FixDecimal`].
    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    pub struct DecimalToFixError;

    impl core::fmt::Display for DecimalToFixError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "decimal mantissa exceeds i64 range for FixDecimal")
        }
    }

    impl std::error::Error for DecimalToFixError {}

    // i128 backing → FixDecimal: fallible — i128 may not fit in i64.
    impl<const D: u8> TryFrom<Decimal<i128, D>> for FixDecimal
    where
        i128: Backing,
    {
        type Error = DecimalToFixError;

        fn try_from(d: Decimal<i128, D>) -> Result<Self, Self::Error> {
            let mantissa = i64::try_from(d.to_raw()).map_err(|_| DecimalToFixError)?;
            Ok(Self { mantissa, scale: D })
        }
    }
}

#[cfg(feature = "nexus-decimal")]
pub use decimal_conv::DecimalConvError;

#[cfg(feature = "nexus-decimal")]
pub use decimal_conv::DecimalToFixError;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- FixDecimal --

    #[test]
    fn decimal_parse_integer() {
        let d = FixDecimal::parse(b"12345").unwrap();
        assert_eq!(d.mantissa, 12345);
        assert_eq!(d.scale, 0);
    }

    #[test]
    fn decimal_parse_fractional() {
        let d = FixDecimal::parse(b"123.456").unwrap();
        assert_eq!(d.mantissa, 123_456);
        assert_eq!(d.scale, 3);
    }

    #[test]
    fn decimal_parse_negative() {
        let d = FixDecimal::parse(b"-99.5").unwrap();
        assert_eq!(d.mantissa, -995);
        assert_eq!(d.scale, 1);
    }

    #[test]
    fn decimal_parse_positive_sign() {
        let d = FixDecimal::parse(b"+42.0").unwrap();
        assert_eq!(d.mantissa, 420);
        assert_eq!(d.scale, 1);
    }

    #[test]
    fn decimal_parse_leading_zero() {
        let d = FixDecimal::parse(b"0.001").unwrap();
        assert_eq!(d.mantissa, 1);
        assert_eq!(d.scale, 3);
    }

    #[test]
    fn decimal_parse_zero() {
        let d = FixDecimal::parse(b"0").unwrap();
        assert_eq!(d.mantissa, 0);
        assert_eq!(d.scale, 0);
    }

    #[test]
    fn decimal_parse_empty() {
        assert!(FixDecimal::parse(b"").is_none());
    }

    #[test]
    fn decimal_parse_sign_only() {
        assert!(FixDecimal::parse(b"-").is_none());
        assert!(FixDecimal::parse(b"+").is_none());
    }

    #[test]
    fn decimal_parse_non_digit() {
        assert!(FixDecimal::parse(b"12.3a4").is_none());
    }

    #[test]
    fn decimal_parse_double_dot() {
        assert!(FixDecimal::parse(b"12.3.4").is_none());
    }

    #[test]
    fn decimal_to_f64() {
        let d = FixDecimal::parse(b"123.456").unwrap();
        let f: f64 = d.into();
        assert!((f - 123.456).abs() < 1e-10);
    }

    #[test]
    fn decimal_to_f64_negative() {
        let d = FixDecimal::parse(b"-0.5").unwrap();
        let f: f64 = d.into();
        assert!((f - (-0.5)).abs() < 1e-10);
    }

    #[test]
    fn decimal_display_integer() {
        let d = FixDecimal {
            mantissa: 42,
            scale: 0,
        };
        assert_eq!(d.to_string(), "42");
    }

    #[test]
    fn decimal_display_fractional() {
        let d = FixDecimal {
            mantissa: 12345,
            scale: 2,
        };
        assert_eq!(d.to_string(), "123.45");
    }

    #[test]
    fn decimal_display_negative_frac() {
        let d = FixDecimal {
            mantissa: -5,
            scale: 1,
        };
        assert_eq!(d.to_string(), "-0.5");
    }

    #[test]
    fn decimal_display_leading_zeros() {
        let d = FixDecimal {
            mantissa: 1,
            scale: 3,
        };
        assert_eq!(d.to_string(), "0.001");
    }

    // -- FixDate --

    #[test]
    fn date_parse() {
        let d = FixDate::parse(b"20260602").unwrap();
        assert_eq!(d.year, 2026);
        assert_eq!(d.month, 6);
        assert_eq!(d.day, 2);
    }

    #[test]
    fn date_parse_too_short() {
        assert!(FixDate::parse(b"2026060").is_none());
    }

    #[test]
    fn date_parse_invalid_month() {
        assert!(FixDate::parse(b"20261302").is_none());
    }

    #[test]
    fn date_parse_zero_month() {
        assert!(FixDate::parse(b"20260002").is_none());
    }

    #[test]
    fn date_parse_zero_day() {
        assert!(FixDate::parse(b"20260600").is_none());
    }

    #[test]
    fn date_epoch_days() {
        // 1970-01-01 is day 0
        let d = FixDate {
            year: 1970,
            month: 1,
            day: 1,
        };
        assert_eq!(d.to_epoch_days(), Some(0));
    }

    #[test]
    fn date_epoch_days_known() {
        // 2000-01-01 = 10957 days after epoch
        let d = FixDate {
            year: 2000,
            month: 1,
            day: 1,
        };
        assert_eq!(d.to_epoch_days(), Some(10957));
    }

    #[test]
    fn date_display() {
        let d = FixDate {
            year: 2026,
            month: 6,
            day: 2,
        };
        assert_eq!(d.to_string(), "20260602");
    }

    // -- FixTime --

    #[test]
    fn time_parse_no_frac() {
        let t = FixTime::parse(b"14:30:00").unwrap();
        assert_eq!(t.hour(), 14);
        assert_eq!(t.minute(), 30);
        assert_eq!(t.second(), 0);
        assert_eq!(t.subsec_nanos(), 0);
    }

    #[test]
    fn time_parse_millis() {
        let t = FixTime::parse(b"09:05:30.123").unwrap();
        assert_eq!(t.hour(), 9);
        assert_eq!(t.minute(), 5);
        assert_eq!(t.second(), 30);
        assert_eq!(t.subsec_nanos(), 123_000_000);
    }

    #[test]
    fn time_parse_micros() {
        let t = FixTime::parse(b"23:59:59.123456").unwrap();
        assert_eq!(t.subsec_nanos(), 123_456_000);
    }

    #[test]
    fn time_parse_nanos() {
        let t = FixTime::parse(b"00:00:00.000000001").unwrap();
        assert_eq!(t.subsec_nanos(), 1);
    }

    #[test]
    fn time_parse_too_short() {
        assert!(FixTime::parse(b"14:30:0").is_none());
    }

    #[test]
    fn time_parse_invalid_hour() {
        assert!(FixTime::parse(b"24:00:00").is_none());
    }

    #[test]
    fn time_parse_invalid_minute() {
        assert!(FixTime::parse(b"14:60:00").is_none());
    }

    #[test]
    fn time_display_no_frac() {
        let t = FixTime {
            nanos_since_midnight: 14 * 3600_000_000_000 + 30 * 60_000_000_000,
        };
        assert_eq!(t.to_string(), "14:30:00");
    }

    #[test]
    fn time_display_with_nanos() {
        let t = FixTime {
            nanos_since_midnight: 500_000_000,
        };
        assert_eq!(t.to_string(), "00:00:00.500000000");
    }

    // -- FixTimestamp --

    #[test]
    fn timestamp_parse_no_frac() {
        let ts = FixTimestamp::parse(b"19700101-00:00:00").unwrap();
        assert_eq!(ts.as_nanos(), 0);
    }

    #[test]
    fn timestamp_parse_with_frac() {
        let ts = FixTimestamp::parse(b"19700101-00:00:01.500").unwrap();
        assert_eq!(ts.as_nanos(), 1_500_000_000);
    }

    #[test]
    fn timestamp_parse_2026() {
        let ts = FixTimestamp::parse(b"20260602-14:30:00").unwrap();
        assert!(ts.as_nanos() > 0);
        assert_eq!(ts.subsec_nanos(), 0);
    }

    #[test]
    fn timestamp_accessors() {
        let ts = FixTimestamp(1_500_000_000_i128); // 1.5 seconds
        assert_eq!(ts.as_secs(), 1);
        assert_eq!(ts.as_millis(), 1500);
        assert_eq!(ts.as_micros(), 1_500_000);
        assert_eq!(ts.subsec_nanos(), 500_000_000);
    }

    #[test]
    fn timestamp_too_short() {
        assert!(FixTimestamp::parse(b"20260602-14:30").is_none());
    }

    #[test]
    fn timestamp_bad_separator() {
        assert!(FixTimestamp::parse(b"20260602T14:30:00").is_none());
    }

    // -- parse_fix_int --

    #[test]
    fn int_parse_positive() {
        assert_eq!(parse_fix_int(b"12345"), Some(12345));
    }

    #[test]
    fn int_parse_negative() {
        assert_eq!(parse_fix_int(b"-42"), Some(-42));
    }

    #[test]
    fn int_parse_zero() {
        assert_eq!(parse_fix_int(b"0"), Some(0));
    }

    #[test]
    fn int_parse_empty() {
        assert_eq!(parse_fix_int(b""), None);
    }

    #[test]
    fn int_parse_non_digit() {
        assert_eq!(parse_fix_int(b"12x"), None);
    }

    // -- parse_fix_uint --

    #[test]
    fn uint_parse() {
        assert_eq!(parse_fix_uint(b"256"), Some(256));
    }

    #[test]
    fn uint_parse_zero() {
        assert_eq!(parse_fix_uint(b"0"), Some(0));
    }

    // -- parse_fix_seqnum --

    #[test]
    fn seqnum_parse() {
        assert_eq!(parse_fix_seqnum(b"1000000"), Some(1_000_000));
    }

    // -- parse_fix_bool --

    #[test]
    fn bool_parse_y() {
        assert_eq!(parse_fix_bool(b"Y"), Some(true));
    }

    #[test]
    fn bool_parse_n() {
        assert_eq!(parse_fix_bool(b"N"), Some(false));
    }

    #[test]
    fn bool_parse_invalid() {
        assert_eq!(parse_fix_bool(b"y"), None);
        assert_eq!(parse_fix_bool(b""), None);
        assert_eq!(parse_fix_bool(b"YES"), None);
    }

    // -- SWAR boundary tests --

    #[test]
    fn swar_single_digit() {
        assert_eq!(parse_fix_int(b"7"), Some(7));
        assert_eq!(parse_fix_seqnum(b"1"), Some(1));
    }

    #[test]
    fn swar_exactly_8_digits() {
        assert_eq!(parse_fix_int(b"12345678"), Some(12_345_678));
        assert_eq!(parse_fix_seqnum(b"99999999"), Some(99_999_999));
    }

    #[test]
    fn swar_9_digits_crosses_block() {
        assert_eq!(parse_fix_int(b"123456789"), Some(123_456_789));
    }

    #[test]
    fn swar_16_digits_two_blocks() {
        assert_eq!(
            parse_fix_seqnum(b"1234567890123456"),
            Some(1_234_567_890_123_456)
        );
    }

    #[test]
    fn swar_17_digits_scalar_plus_blocks() {
        assert_eq!(
            parse_fix_seqnum(b"12345678901234567"),
            Some(12_345_678_901_234_567)
        );
    }

    #[test]
    fn swar_19_digits_max_i64() {
        assert_eq!(parse_fix_int(b"9223372036854775807"), Some(i64::MAX));
    }

    #[test]
    fn swar_19_digits_min_i64() {
        assert_eq!(parse_fix_int(b"-9223372036854775808"), Some(i64::MIN));
    }

    #[test]
    fn swar_decimal_8_digit_mantissa() {
        let d = FixDecimal::parse(b"1234.5678").unwrap();
        assert_eq!(d.mantissa, 12_345_678);
        assert_eq!(d.scale, 4);
    }

    #[test]
    fn swar_decimal_16_digit_mantissa() {
        let d = FixDecimal::parse(b"12345678.90123456").unwrap();
        assert_eq!(d.mantissa, 1_234_567_890_123_456);
        assert_eq!(d.scale, 8);
    }

    #[test]
    fn swar_decimal_realistic_price() {
        let d = FixDecimal::parse(b"50123.45000000").unwrap();
        assert_eq!(d.mantissa, 5_012_345_000_000);
        assert_eq!(d.scale, 8);
        let f: f64 = d.into();
        assert!((f - 50123.45).abs() < 1e-6);
    }

    #[test]
    fn swar_all_digit_lengths() {
        for n in 1..=19u64 {
            let s = n.to_string();
            assert_eq!(parse_fix_seqnum(s.as_bytes()), Some(n), "failed for {n}");
        }
    }

    // -- Encode: encode_fix_int --

    #[test]
    fn encode_int_positive() {
        let mut buf = [0u8; 20];
        let n = encode_fix_int(12345, &mut buf);
        assert_eq!(&buf[..n], b"12345");
    }

    #[test]
    fn encode_int_negative() {
        let mut buf = [0u8; 20];
        let n = encode_fix_int(-42, &mut buf);
        assert_eq!(&buf[..n], b"-42");
    }

    #[test]
    fn encode_int_zero() {
        let mut buf = [0u8; 20];
        let n = encode_fix_int(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn encode_int_max() {
        let mut buf = [0u8; 20];
        let n = encode_fix_int(i64::MAX, &mut buf);
        assert_eq!(&buf[..n], b"9223372036854775807");
    }

    #[test]
    fn encode_int_min() {
        let mut buf = [0u8; 20];
        let n = encode_fix_int(i64::MIN, &mut buf);
        assert_eq!(&buf[..n], b"-9223372036854775808");
    }

    // -- Encode: encode_fix_uint --

    #[test]
    fn encode_uint() {
        let mut buf = [0u8; 10];
        let n = encode_fix_uint(256, &mut buf);
        assert_eq!(&buf[..n], b"256");
    }

    #[test]
    fn encode_uint_zero() {
        let mut buf = [0u8; 10];
        let n = encode_fix_uint(0, &mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    // -- Encode: encode_fix_seqnum --

    #[test]
    fn encode_seqnum() {
        let mut buf = [0u8; 20];
        let n = encode_fix_seqnum(1_000_000, &mut buf);
        assert_eq!(&buf[..n], b"1000000");
    }

    // -- Encode: encode_fix_bool --

    #[test]
    fn encode_bool_true() {
        assert_eq!(encode_fix_bool(true), b'Y');
    }

    #[test]
    fn encode_bool_false() {
        assert_eq!(encode_fix_bool(false), b'N');
    }

    // -- Encode: FixDecimal --

    #[test]
    fn decimal_encode_integer() {
        let d = FixDecimal {
            mantissa: 12345,
            scale: 0,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"12345");
    }

    #[test]
    fn decimal_encode_fractional() {
        let d = FixDecimal {
            mantissa: 123_456,
            scale: 3,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"123.456");
    }

    #[test]
    fn decimal_encode_negative() {
        let d = FixDecimal {
            mantissa: -995,
            scale: 1,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"-99.5");
    }

    #[test]
    fn decimal_encode_leading_frac_zeros() {
        let d = FixDecimal {
            mantissa: 1,
            scale: 3,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"0.001");
    }

    #[test]
    fn decimal_encode_zero() {
        let d = FixDecimal {
            mantissa: 0,
            scale: 0,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"0");
    }

    #[test]
    fn decimal_encode_negative_sub_unit() {
        let d = FixDecimal {
            mantissa: -5,
            scale: 1,
        };
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"-0.5");
    }

    // -- Encode: FixDate --

    #[test]
    fn date_encode() {
        let d = FixDate {
            year: 2026,
            month: 6,
            day: 2,
        };
        let mut buf = [0u8; 8];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"20260602");
    }

    #[test]
    fn date_from_epoch_days_epoch() {
        let d = FixDate::from_epoch_days(0);
        assert_eq!(
            d,
            FixDate {
                year: 1970,
                month: 1,
                day: 1
            }
        );
    }

    #[test]
    fn date_from_epoch_days_y2k() {
        let d = FixDate::from_epoch_days(10957);
        assert_eq!(
            d,
            FixDate {
                year: 2000,
                month: 1,
                day: 1
            }
        );
    }

    #[test]
    fn date_epoch_days_roundtrip() {
        for days in [0, 1, 365, 10957, 20000, -1, -365] {
            let date = FixDate::from_epoch_days(days);
            assert_eq!(date.to_epoch_days(), Some(days), "failed for days={days}");
        }
    }

    // -- Encode: FixTime --

    #[test]
    fn time_encode_no_frac() {
        let t = FixTime {
            nanos_since_midnight: 14 * 3_600_000_000_000 + 30 * 60_000_000_000,
        };
        let mut buf = [0u8; 18];
        let n = t.encode(&mut buf);
        assert_eq!(&buf[..n], b"14:30:00");
    }

    #[test]
    fn time_encode_millis() {
        let t = FixTime {
            nanos_since_midnight: 9 * 3_600_000_000_000
                + 5 * 60_000_000_000
                + 30_000_000_000
                + 123_000_000,
        };
        let mut buf = [0u8; 18];
        let n = t.encode(&mut buf);
        assert_eq!(&buf[..n], b"09:05:30.123");
    }

    #[test]
    fn time_encode_micros() {
        let t = FixTime {
            nanos_since_midnight: 23 * 3_600_000_000_000
                + 59 * 60_000_000_000
                + 59_000_000_000
                + 123_456_000,
        };
        let mut buf = [0u8; 18];
        let n = t.encode(&mut buf);
        assert_eq!(&buf[..n], b"23:59:59.123456");
    }

    #[test]
    fn time_encode_nanos() {
        let t = FixTime {
            nanos_since_midnight: 1,
        };
        let mut buf = [0u8; 18];
        let n = t.encode(&mut buf);
        assert_eq!(&buf[..n], b"00:00:00.000000001");
    }

    // -- Encode: FixTimestamp --

    #[test]
    fn timestamp_encode_epoch() {
        let ts = FixTimestamp(0);
        let mut buf = [0u8; 27];
        let n = ts.encode(&mut buf);
        assert_eq!(&buf[..n], b"19700101-00:00:00");
    }

    #[test]
    fn timestamp_encode_with_frac() {
        let ts = FixTimestamp(1_500_000_000);
        let mut buf = [0u8; 27];
        let n = ts.encode(&mut buf);
        assert_eq!(&buf[..n], b"19700101-00:00:01.500");
    }

    // -- Roundtrip: parse → encode --

    #[test]
    fn decimal_roundtrip() {
        for input in &[
            &b"12345"[..],
            &b"123.456"[..],
            &b"0.001"[..],
            &b"99.50"[..],
            &b"12345678"[..],
            &b"50123.45000000"[..],
            &b"1234567.890123456"[..],
        ] {
            let d = FixDecimal::parse(input).unwrap();
            let mut buf = [0u8; 21];
            let n = d.encode(&mut buf);
            assert_eq!(
                &buf[..n],
                *input,
                "roundtrip failed for {:?}",
                core::str::from_utf8(input).unwrap()
            );
        }
    }

    #[test]
    fn decimal_roundtrip_negative() {
        let d = FixDecimal::parse(b"-123.456").unwrap();
        let mut buf = [0u8; 21];
        let n = d.encode(&mut buf);
        assert_eq!(&buf[..n], b"-123.456");
    }

    #[test]
    fn date_roundtrip() {
        for input in &[b"20260602", b"19700101", b"20000101", b"19991231"] {
            let d = FixDate::parse(&input[..]).unwrap();
            let mut buf = [0u8; 8];
            let n = d.encode(&mut buf);
            assert_eq!(
                &buf[..n],
                &input[..],
                "roundtrip failed for {:?}",
                core::str::from_utf8(&input[..]).unwrap()
            );
        }
    }

    #[test]
    fn time_roundtrip() {
        for input in &[
            &b"14:30:00"[..],
            &b"09:05:30.123"[..],
            &b"23:59:59.123456"[..],
            &b"00:00:00.000000001"[..],
        ] {
            let t = FixTime::parse(input).unwrap();
            let mut buf = [0u8; 18];
            let n = t.encode(&mut buf);
            assert_eq!(
                &buf[..n],
                *input,
                "roundtrip failed for {:?}",
                core::str::from_utf8(input).unwrap()
            );
        }
    }

    #[test]
    fn timestamp_roundtrip() {
        for input in &[
            &b"19700101-00:00:00"[..],
            &b"20260602-14:30:00"[..],
            &b"20260602-14:30:00.123"[..],
            &b"20260602-14:30:00.123456"[..],
            &b"20260602-14:30:00.123456789"[..],
        ] {
            let ts = FixTimestamp::parse(input).unwrap();
            let mut buf = [0u8; 27];
            let n = ts.encode(&mut buf);
            assert_eq!(
                &buf[..n],
                *input,
                "roundtrip failed for {:?}",
                core::str::from_utf8(input).unwrap()
            );
        }
    }

    #[test]
    fn int_roundtrip() {
        for val in [0i64, 1, -1, 42, -42, 12345, -12345, i64::MAX, i64::MIN] {
            let s = val.to_string();
            let parsed = parse_fix_int(s.as_bytes()).unwrap();
            assert_eq!(parsed, val);
            let mut buf = [0u8; 20];
            let n = encode_fix_int(parsed, &mut buf);
            assert_eq!(&buf[..n], s.as_bytes(), "roundtrip failed for {val}");
        }
    }

    // -- nexus-decimal conversions --

    #[cfg(feature = "nexus-decimal")]
    mod decimal_conv_tests {
        use super::*;
        use nexus_decimal::Decimal;

        #[test]
        fn to_i128_decimal_widening() {
            let d = FixDecimal::parse(b"123.45").unwrap();
            let dec: Decimal<i128, 8> = d.into();
            assert_eq!(dec.to_raw(), 12_345_000_000);
        }

        #[test]
        fn to_i128_decimal_narrowing() {
            let d = FixDecimal::parse(b"1.123456789").unwrap();
            let dec: Decimal<i128, 4> = d.into();
            assert_eq!(dec.to_raw(), 11234);
        }

        #[test]
        fn to_i64_decimal_ok() {
            let d = FixDecimal::parse(b"99.50").unwrap();
            let dec: Decimal<i64, 8> = d.try_into().unwrap();
            assert_eq!(dec.to_raw(), 9_950_000_000);
        }

        #[test]
        fn to_i64_decimal_overflow() {
            let d = FixDecimal {
                mantissa: i64::MAX,
                scale: 0,
            };
            let result: Result<Decimal<i64, 8>, _> = d.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn to_i32_decimal_ok() {
            let d = FixDecimal::parse(b"1.25").unwrap();
            let dec: Decimal<i32, 4> = d.try_into().unwrap();
            assert_eq!(dec.to_raw(), 12500);
        }

        #[test]
        fn to_i32_decimal_overflow() {
            let d = FixDecimal::parse(b"999999999.99").unwrap();
            let result: Result<Decimal<i32, 4>, _> = d.try_into();
            assert!(result.is_err());
        }

        // -- Reverse: Decimal → FixDecimal --

        #[test]
        fn from_i64_decimal() {
            let dec = Decimal::<i64, 8>::from_raw(9_950_000_000);
            let d: FixDecimal = dec.into();
            assert_eq!(d.mantissa, 9_950_000_000);
            assert_eq!(d.scale, 8);
        }

        #[test]
        fn from_i32_decimal() {
            let dec = Decimal::<i32, 4>::from_raw(12500);
            let d: FixDecimal = dec.into();
            assert_eq!(d.mantissa, 12500);
            assert_eq!(d.scale, 4);
        }

        #[test]
        fn from_i128_decimal_ok() {
            let dec = Decimal::<i128, 8>::from_raw(12_345_000_000);
            let d: FixDecimal = dec.try_into().unwrap();
            assert_eq!(d.mantissa, 12_345_000_000);
            assert_eq!(d.scale, 8);
        }

        #[test]
        fn from_i128_decimal_overflow() {
            let dec = Decimal::<i128, 8>::from_raw(i128::MAX);
            let result: Result<FixDecimal, _> = dec.try_into();
            assert!(result.is_err());
        }

        #[test]
        fn decimal_roundtrip_through_nexus_decimal() {
            let d = FixDecimal::parse(b"99.50").unwrap();
            let dec: Decimal<i64, 8> = d.try_into().unwrap();
            let back: FixDecimal = dec.into();
            assert_eq!(back.mantissa, 9_950_000_000);
            assert_eq!(back.scale, 8);
            let f1: f64 = d.into();
            let f2: f64 = back.into();
            assert!((f1 - f2).abs() < 1e-10);
        }
    }
}
