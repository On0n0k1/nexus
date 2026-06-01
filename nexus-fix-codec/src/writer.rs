//! FIX field writer for encoding `tag=value\x01` fields into a buffer.
//!
//! Provides [`FieldWriter`], a cursor that writes FIX fields into a
//! caller-provided `&mut [u8]`. Generated encoders (from `nexus-fix-codegen`)
//! compose this with framing logic for complete message construction.
//!
//! Also provides [`encode_field`] as a standalone function for cases
//! where the struct overhead isn't needed.

/// FIX field writer.
///
/// Wraps a `&mut [u8]` buffer and tracks the write position as fields
/// are appended. Symmetric with [`FieldReader`](crate::FieldReader)
/// on the read side.
///
/// # Example
///
/// ```
/// use nexus_fix_codec::writer::FieldWriter;
///
/// let mut buf = [0u8; 64];
/// let mut w = FieldWriter::wrap(&mut buf);
/// w.field(35, b"D");
/// w.field(49, b"SENDER");
/// w.field(55, b"BTC-USD");
/// assert_eq!(w.data(), b"35=D\x0149=SENDER\x0155=BTC-USD\x01");
/// ```
pub struct FieldWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> FieldWriter<'a> {
    /// Wrap a mutable buffer for writing FIX fields.
    #[inline]
    pub fn wrap(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Wrap a mutable buffer, starting writes at `offset`.
    #[inline]
    pub fn wrap_at(buf: &'a mut [u8], offset: usize) -> Self {
        Self { buf, pos: offset }
    }

    /// Write a `tag=value\x01` field. Advances position.
    #[inline]
    pub fn field(&mut self, tag: u32, value: &[u8]) {
        self.pos = encode_field(self.buf, self.pos, tag, value);
    }

    /// Current write position (bytes written so far).
    #[inline]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// The written portion of the buffer.
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
}

/// Write a `tag=value\x01` field into `buf` at `pos`. Returns new position.
///
/// This is the standalone version of [`FieldWriter::field`] for use
/// without the struct wrapper.
#[inline]
pub fn encode_field(buf: &mut [u8], pos: usize, tag: u32, value: &[u8]) -> usize {
    let mut p = pos;
    p = write_tag(buf, p, tag);
    buf[p] = b'=';
    p += 1;
    buf[p..p + value.len()].copy_from_slice(value);
    p += value.len();
    buf[p] = 0x01;
    p + 1
}

/// Format a checksum value as 3 zero-padded ASCII digits.
///
/// FIX tag 10 is always a 3-character zero-padded decimal value.
///
/// # Example
///
/// ```
/// use nexus_fix_codec::writer::format_checksum;
///
/// assert_eq!(&format_checksum(42), b"042");
/// assert_eq!(&format_checksum(178), b"178");
/// ```
#[inline]
pub fn format_checksum(sum: u8) -> [u8; 3] {
    [sum / 100 + b'0', (sum / 10) % 10 + b'0', sum % 10 + b'0']
}

// =============================================================================
// Internal: tag number → ASCII digits
// =============================================================================

#[inline]
fn write_tag(buf: &mut [u8], pos: usize, tag: u32) -> usize {
    if tag >= 10000 {
        buf[pos] = (tag / 10000) as u8 + b'0';
        buf[pos + 1] = ((tag / 1000) % 10) as u8 + b'0';
        buf[pos + 2] = ((tag / 100) % 10) as u8 + b'0';
        buf[pos + 3] = ((tag / 10) % 10) as u8 + b'0';
        buf[pos + 4] = (tag % 10) as u8 + b'0';
        pos + 5
    } else if tag >= 1000 {
        buf[pos] = (tag / 1000) as u8 + b'0';
        buf[pos + 1] = ((tag / 100) % 10) as u8 + b'0';
        buf[pos + 2] = ((tag / 10) % 10) as u8 + b'0';
        buf[pos + 3] = (tag % 10) as u8 + b'0';
        pos + 4
    } else if tag >= 100 {
        buf[pos] = (tag / 100) as u8 + b'0';
        buf[pos + 1] = ((tag / 10) % 10) as u8 + b'0';
        buf[pos + 2] = (tag % 10) as u8 + b'0';
        pos + 3
    } else if tag >= 10 {
        buf[pos] = (tag / 10) as u8 + b'0';
        buf[pos + 1] = (tag % 10) as u8 + b'0';
        pos + 2
    } else {
        buf[pos] = tag as u8 + b'0';
        pos + 1
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_field() {
        let mut buf = [0u8; 32];
        let end = encode_field(&mut buf, 0, 35, b"D");
        assert_eq!(&buf[..end], b"35=D\x01");
    }

    #[test]
    fn multiple_fields() {
        let mut buf = [0u8; 64];
        let mut pos = 0;
        pos = encode_field(&mut buf, pos, 8, b"FIX.4.4");
        pos = encode_field(&mut buf, pos, 35, b"D");
        pos = encode_field(&mut buf, pos, 49, b"SENDER");
        assert_eq!(&buf[..pos], b"8=FIX.4.4\x0135=D\x0149=SENDER\x01");
    }

    #[test]
    fn all_tag_widths() {
        let cases: &[(u32, &[u8])] = &[
            (8, b"8=v\x01"),
            (35, b"35=v\x01"),
            (150, b"150=v\x01"),
            (5592, b"5592=v\x01"),
            (10000, b"10000=v\x01"),
        ];
        for &(tag, expected) in cases {
            let mut buf = [0u8; 16];
            let end = encode_field(&mut buf, 0, tag, b"v");
            assert_eq!(&buf[..end], expected, "tag={}", tag);
        }
    }

    #[test]
    fn empty_value() {
        let mut buf = [0u8; 16];
        let end = encode_field(&mut buf, 0, 35, b"");
        assert_eq!(&buf[..end], b"35=\x01");
    }

    #[test]
    fn encode_from_offset() {
        let mut buf = [0u8; 32];
        buf[0..5].copy_from_slice(b"XXXXX");
        let end = encode_field(&mut buf, 5, 35, b"D");
        assert_eq!(&buf[5..end], b"35=D\x01");
    }

    #[test]
    fn format_checksum_values() {
        assert_eq!(&format_checksum(0), b"000");
        assert_eq!(&format_checksum(42), b"042");
        assert_eq!(&format_checksum(178), b"178");
        assert_eq!(&format_checksum(255), b"255");
    }

    #[test]
    fn writer_basic() {
        let mut buf = [0u8; 64];
        let mut w = FieldWriter::wrap(&mut buf);
        w.field(35, b"D");
        w.field(49, b"SENDER");
        assert_eq!(w.pos(), 15);
        assert_eq!(w.data(), b"35=D\x0149=SENDER\x01");
    }

    #[test]
    fn writer_wrap_at() {
        let mut buf = [0u8; 64];
        let mut w = FieldWriter::wrap_at(&mut buf, 10);
        w.field(35, b"D");
        assert_eq!(w.pos(), 15);
        assert_eq!(&buf[10..15], b"35=D\x01");
    }

    #[test]
    fn roundtrip_read_write() {
        let mut buf = [0u8; 128];
        let mut w = FieldWriter::wrap(&mut buf);
        w.field(8, b"FIX.4.4");
        w.field(35, b"D");
        w.field(49, b"SENDER");
        w.field(55, b"BTC-USD");
        let written = w.pos();

        let mut reader = crate::FieldReader::new(&buf[..written], 0);
        let fields: Vec<_> = reader.by_ref().collect();

        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].tag, 8);
        assert_eq!(fields[0].value.slice(&buf), b"FIX.4.4");
        assert_eq!(fields[1].tag, 35);
        assert_eq!(fields[1].value.slice(&buf), b"D");
        assert_eq!(fields[2].tag, 49);
        assert_eq!(fields[2].value.slice(&buf), b"SENDER");
        assert_eq!(fields[3].tag, 55);
        assert_eq!(fields[3].value.slice(&buf), b"BTC-USD");
    }

    #[test]
    fn writer_with_checksum() {
        let mut buf = [0u8; 128];
        let body_end;
        {
            let mut w = FieldWriter::wrap(&mut buf);
            w.field(35, b"D");
            w.field(49, b"SENDER");
            body_end = w.pos();
        }

        let sum = crate::checksum(&buf[..body_end]);
        let msg_end = encode_field(&mut buf, body_end, 10, &format_checksum(sum));

        assert!(buf[body_end..msg_end].starts_with(b"10="));
        assert_eq!(buf[msg_end - 1], 0x01);
    }
}
