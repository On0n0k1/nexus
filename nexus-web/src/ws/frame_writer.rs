use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

use super::frame::Role;

/// Error from WebSocket frame encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// Control frame payload exceeds 125 bytes (RFC 6455 §5.5).
    ControlPayloadTooLarge(usize),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ControlPayloadTooLarge(n) => {
                write!(f, "control frame payload too large: {n} bytes (max 125)")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

/// Frame header bytes (stack-allocated, max 14 bytes).
pub struct FrameHeader {
    bytes: [u8; 14],
    len: u8,
}

impl FrameHeader {
    /// The header bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Header length in bytes.
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether the header is empty (shouldn't happen in practice).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// WebSocket frame encoder.
///
/// Encodes messages into RFC 6455 wire format. If the role is Client,
/// frames are masked with a random 4-byte key. If Server, no masking.
///
/// Owns its output buffer — call [`data`](Self::data) after any `encode_*`
/// call to get the pending bytes, and [`advance`](Self::advance) after a
/// partial write. For a zero-copy raw-slice path use the `encode_*_raw`
/// variants instead.
///
/// # Usage
///
/// ```
/// use nexus_web::ws::{FrameWriter, Role};
///
/// let mut writer = FrameWriter::new(Role::Server, 65_536);
/// writer.encode_text(b"Hello");
/// assert_eq!(&writer.data()[2..], b"Hello");
/// ```
pub struct FrameWriter {
    role: Role,
    /// PRNG for mask key generation (client only). Seeded lazily from
    /// OS randomness on first use, then produces mask keys at ~1 cycle
    /// instead of ~50-200 cycles per getrandom syscall.
    mask_rng: Option<ChaCha8Rng>,
    write_buf: nexus_net::buf::WriteBuf,
}

impl FrameWriter {
    /// Create a writer for the given role with the given write buffer capacity.
    #[must_use]
    pub fn new(role: Role, capacity: usize) -> Self {
        Self {
            role,
            mask_rng: None,
            write_buf: nexus_net::buf::WriteBuf::new(capacity, 14),
        }
    }

    /// Encoded frame bytes, ready to write to the socket.
    ///
    /// Valid after any `encode_*` call. Consumed by [`advance`](Self::advance).
    #[inline]
    pub fn data(&self) -> &[u8] {
        self.write_buf.data()
    }

    /// Consume `n` bytes from the front after a partial write.
    ///
    /// On full drain, the buffer resets automatically — no `clear()` needed
    /// between frames.
    #[inline]
    pub fn advance(&mut self, n: usize) {
        self.write_buf.advance(n);
    }

    /// Encode a text message frame into the internal buffer.
    pub fn encode_text(&mut self, payload: &[u8]) {
        self.encode_into(0x81, payload);
    }

    /// Encode a binary message frame into the internal buffer.
    pub fn encode_binary(&mut self, payload: &[u8]) {
        self.encode_into(0x82, payload);
    }

    /// Encode a ping control frame into the internal buffer.
    ///
    /// Returns `Err` if payload exceeds 125 bytes (RFC 6455 §5.5).
    pub fn encode_ping(&mut self, payload: &[u8]) -> Result<(), EncodeError> {
        if payload.len() > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload.len()));
        }
        self.encode_into(0x89, payload);
        Ok(())
    }

    /// Encode a pong control frame into the internal buffer.
    ///
    /// Returns `Err` if payload exceeds 125 bytes (RFC 6455 §5.5).
    pub fn encode_pong(&mut self, payload: &[u8]) -> Result<(), EncodeError> {
        if payload.len() > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload.len()));
        }
        self.encode_into(0x8A, payload);
        Ok(())
    }

    /// Encode a close frame into the internal buffer.
    ///
    /// Returns `Err` if code + reason exceeds 125 bytes.
    pub fn encode_close(&mut self, code: u16, reason: &[u8]) -> Result<(), EncodeError> {
        let payload_len = 2 + reason.len();
        if payload_len > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload_len));
        }
        self.write_buf.clear();
        self.write_buf.append(&code.to_be_bytes());
        self.write_buf.append(reason);
        let (hdr, mask_key) = self.build_header(0x88, payload_len);
        if let Some(mask) = mask_key {
            super::mask::apply_mask(self.write_buf.data_mut(), mask);
        }
        self.write_buf.prepend(hdr.as_bytes());
        Ok(())
    }

    /// Encode an empty close frame into the internal buffer.
    ///
    /// Used when `CloseCode::NoStatus` is intended — RFC 6455 §7.4.1
    /// reserves code 1005 from appearing in close frame payloads.
    pub fn encode_empty_close(&mut self) {
        self.encode_into(0x88, &[]);
    }

    /// Encode a close frame with structured [`CloseCode`](super::CloseCode) and UTF-8 reason.
    ///
    /// # Panics
    /// Panics if `code` is `CloseCode::NoStatus` (RFC 6455 reserves 1005
    /// from appearing on the wire — use [`encode_empty_close`](Self::encode_empty_close)).
    /// Panics if 2 + reason.len() exceeds 125 bytes.
    pub fn encode_close_code(
        &mut self,
        code: super::message::CloseCode,
        reason: &str,
    ) -> Result<(), EncodeError> {
        assert!(
            code != super::message::CloseCode::NoStatus,
            "CloseCode::NoStatus cannot be sent on the wire — use encode_empty_close()"
        );
        self.encode_close(code.as_u16(), reason.as_bytes())
    }

    /// Encode a text frame, writing the payload via a closure.
    ///
    /// The closure writes directly into the internal buffer — no intermediate
    /// allocation. The WS frame header (including payload length) is
    /// prepended after the closure returns.
    ///
    /// ```ignore
    /// writer.encode_text_writer(|w| {
    ///     use std::io::Write;
    ///     serde_json::to_writer(w, &msg)
    /// })?;
    /// ```
    pub fn encode_text_writer<F, E>(&mut self, f: F) -> Result<(), E>
    where
        F: FnOnce(&mut nexus_net::buf::WriteBufWriter<'_>) -> Result<(), E>,
    {
        self.encode_writer_into(0x81, f)
    }

    /// Encode a binary frame, writing the payload via a closure.
    pub fn encode_binary_writer<F, E>(&mut self, f: F) -> Result<(), E>
    where
        F: FnOnce(&mut nexus_net::buf::WriteBufWriter<'_>) -> Result<(), E>,
    {
        self.encode_writer_into(0x82, f)
    }

    /// Encode a text frame with a fixed-size payload via closure.
    ///
    /// The closure receives `&mut [u8]` of exactly `len` bytes.
    pub fn encode_text_fixed(&mut self, len: usize, f: impl FnOnce(&mut [u8])) {
        self.encode_fixed_into(0x81, len, f);
    }

    /// Encode a binary frame with a fixed-size payload via closure.
    pub fn encode_binary_fixed(&mut self, len: usize, f: impl FnOnce(&mut [u8])) {
        self.encode_fixed_into(0x82, len, f);
    }

    /// Maximum encoded size for a given payload length.
    /// Accounts for header (2-10 bytes) + optional mask (4 bytes).
    #[must_use]
    pub fn max_encoded_len(&self, payload_len: usize) -> usize {
        let header = if payload_len <= 125 {
            2
        } else if payload_len <= 65535 {
            4
        } else {
            10
        };
        let mask = if self.role == Role::Client { 4 } else { 0 };
        header + mask + payload_len
    }

    /// Encode a text message frame into a raw byte slice. Returns bytes written.
    ///
    /// Zero-copy path — no internal buffer used. Use this when you already
    /// own a pre-allocated slice. Prefer [`encode_text`](Self::encode_text)
    /// for the common case.
    ///
    /// # Panics
    /// Panics if `dst` is too small. Use [`max_encoded_len`](Self::max_encoded_len).
    pub fn encode_text_raw(&mut self, payload: &[u8], dst: &mut [u8]) -> usize {
        self.encode_slice(0x81, payload, dst)
    }

    /// Encode a binary message frame into a raw byte slice. Returns bytes written.
    pub fn encode_binary_raw(&mut self, payload: &[u8], dst: &mut [u8]) -> usize {
        self.encode_slice(0x82, payload, dst)
    }

    /// Encode a ping control frame into a raw byte slice. Returns bytes written.
    ///
    /// Returns `Err` if payload exceeds 125 bytes (RFC 6455 §5.5).
    pub fn encode_ping_raw(
        &mut self,
        payload: &[u8],
        dst: &mut [u8],
    ) -> Result<usize, EncodeError> {
        if payload.len() > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload.len()));
        }
        Ok(self.encode_slice(0x89, payload, dst))
    }

    /// Encode a pong control frame into a raw byte slice. Returns bytes written.
    ///
    /// Returns `Err` if payload exceeds 125 bytes (RFC 6455 §5.5).
    pub fn encode_pong_raw(
        &mut self,
        payload: &[u8],
        dst: &mut [u8],
    ) -> Result<usize, EncodeError> {
        if payload.len() > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload.len()));
        }
        Ok(self.encode_slice(0x8A, payload, dst))
    }

    /// Encode a close frame into a raw byte slice. Returns bytes written.
    pub fn encode_close_raw(
        &mut self,
        code: u16,
        reason: &[u8],
        dst: &mut [u8],
    ) -> Result<usize, EncodeError> {
        let payload_len = 2 + reason.len();
        if payload_len > 125 {
            return Err(EncodeError::ControlPayloadTooLarge(payload_len));
        }
        let mut close_payload = [0u8; 125];
        close_payload[..2].copy_from_slice(&code.to_be_bytes());
        close_payload[2..payload_len].copy_from_slice(reason);
        Ok(self.encode_slice(0x88, &close_payload[..payload_len], dst))
    }

    /// Build just the frame header. Returns (header_bytes, length, optional mask_key).
    ///
    /// For use with WriteBuf: append payload, apply mask if Some, prepend header.
    pub fn build_header(
        &mut self,
        byte0: u8,
        payload_len: usize,
    ) -> (FrameHeader, Option<[u8; 4]>) {
        let mask_bit: u8 = if self.role == Role::Client { 0x80 } else { 0 };
        let mut hdr = FrameHeader {
            bytes: [0; 14],
            len: 0,
        };

        hdr.bytes[0] = byte0;
        hdr.len = 1;

        if payload_len <= 125 {
            hdr.bytes[1] = mask_bit | (payload_len as u8);
            hdr.len = 2;
        } else if payload_len <= 65535 {
            hdr.bytes[1] = mask_bit | 0x7E;
            hdr.bytes[2..4].copy_from_slice(&(payload_len as u16).to_be_bytes());
            hdr.len = 4;
        } else {
            hdr.bytes[1] = mask_bit | 0x7F;
            hdr.bytes[2..10].copy_from_slice(&(payload_len as u64).to_be_bytes());
            hdr.len = 10;
        }

        let mask_key = if self.role == Role::Client {
            let mask = self.generate_mask();
            hdr.bytes[hdr.len as usize..hdr.len as usize + 4].copy_from_slice(&mask);
            hdr.len += 4;
            Some(mask)
        } else {
            None
        };

        (hdr, mask_key)
    }

    // =========================================================================
    // Internal
    // =========================================================================

    fn encode_into(&mut self, byte0: u8, payload: &[u8]) {
        self.write_buf.clear();
        self.write_buf.append(payload);
        let (hdr, mask_key) = self.build_header(byte0, payload.len());
        if let Some(mask) = mask_key {
            super::mask::apply_mask(self.write_buf.data_mut(), mask);
        }
        self.write_buf.prepend(hdr.as_bytes());
    }

    fn encode_writer_into<F, E>(&mut self, byte0: u8, f: F) -> Result<(), E>
    where
        F: FnOnce(&mut nexus_net::buf::WriteBufWriter<'_>) -> Result<(), E>,
    {
        self.write_buf.clear();
        let payload_len = {
            let mut bw = nexus_net::buf::WriteBufWriter::new(&mut self.write_buf);
            f(&mut bw)?;
            bw.written()
        };
        let (hdr, mask_key) = self.build_header(byte0, payload_len);
        if let Some(mask) = mask_key {
            super::mask::apply_mask(self.write_buf.data_mut(), mask);
        }
        self.write_buf.prepend(hdr.as_bytes());
        Ok(())
    }

    fn encode_fixed_into(&mut self, byte0: u8, len: usize, f: impl FnOnce(&mut [u8])) {
        self.write_buf.clear();
        self.write_buf.extend_zeroed(len);
        f(self.write_buf.data_mut());
        let (hdr, mask_key) = self.build_header(byte0, len);
        if let Some(mask) = mask_key {
            super::mask::apply_mask(self.write_buf.data_mut(), mask);
        }
        self.write_buf.prepend(hdr.as_bytes());
    }

    /// Generate a 4-byte mask key from the internal PRNG.
    ///
    /// The PRNG is seeded from OS randomness on first use, then produces
    /// mask keys without syscalls. RFC 6455 §10.3 requires unpredictable
    /// masking keys — ChaCha8 satisfies this.
    fn generate_mask(&mut self) -> [u8; 4] {
        let rng = self.mask_rng.get_or_insert_with(|| {
            let mut seed = [0u8; 32];
            getrandom::fill(&mut seed).expect("OS randomness unavailable");
            ChaCha8Rng::from_seed(seed)
        });
        let mut mask = [0u8; 4];
        rng.fill_bytes(&mut mask);
        mask
    }

    fn encode_slice(&mut self, byte0: u8, payload: &[u8], dst: &mut [u8]) -> usize {
        let mask_bit: u8 = if self.role == Role::Client { 0x80 } else { 0 };
        let payload_len = payload.len();

        let mut offset = 0;

        dst[offset] = byte0;
        offset += 1;

        if payload_len <= 125 {
            dst[offset] = mask_bit | (payload_len as u8);
            offset += 1;
        } else if payload_len <= 65535 {
            dst[offset] = mask_bit | 0x7E;
            offset += 1;
            dst[offset..offset + 2].copy_from_slice(&(payload_len as u16).to_be_bytes());
            offset += 2;
        } else {
            dst[offset] = mask_bit | 0x7F;
            offset += 1;
            dst[offset..offset + 8].copy_from_slice(&(payload_len as u64).to_be_bytes());
            offset += 8;
        }

        if self.role == Role::Client {
            let mask = self.generate_mask();
            dst[offset..offset + 4].copy_from_slice(&mask);
            offset += 4;
            dst[offset..offset + payload_len].copy_from_slice(payload);
            super::mask::apply_mask(&mut dst[offset..offset + payload_len], mask);
        } else {
            dst[offset..offset + payload_len].copy_from_slice(payload);
        }

        offset + payload_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_text_server() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_text(b"Hello");
        let d = writer.data();
        assert_eq!(d[0], 0x81); // FIN + Text
        assert_eq!(d[1], 0x05); // no mask, len=5
        assert_eq!(&d[2..], b"Hello");
    }

    #[test]
    fn encode_binary_server() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_binary(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let d = writer.data();
        assert_eq!(d[0], 0x82); // FIN + Binary
        assert_eq!(&d[2..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn encode_close_server() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_close(1000, b"goodbye").unwrap();
        let d = writer.data();
        assert_eq!(d[0], 0x88); // FIN + Close
        assert_eq!(&d[2..4], &1000u16.to_be_bytes());
        assert_eq!(&d[4..], b"goodbye");
    }

    #[test]
    fn encode_ping_server() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_ping(b"ping").unwrap();
        let d = writer.data();
        assert_eq!(d[0], 0x89); // FIN + Ping
        assert_eq!(&d[2..], b"ping");
    }

    #[test]
    fn encode_pong_server() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_pong(b"pong").unwrap();
        let d = writer.data();
        assert_eq!(d[0], 0x8A); // FIN + Pong
        assert_eq!(&d[2..], b"pong");
    }

    #[test]
    fn encode_client_is_masked() {
        let mut writer = FrameWriter::new(Role::Client, 65_536);
        writer.encode_text(b"Hello");
        let d = writer.data();
        assert_eq!(d.len(), 11); // 2 header + 4 mask + 5 payload
        assert_eq!(d[0], 0x81); // FIN + Text
        assert_eq!(d[1] & 0x80, 0x80); // mask bit set
        assert_eq!(d[1] & 0x7F, 5); // len=5
        assert_ne!(&d[6..11], b"Hello"); // masked
    }

    #[test]
    fn encode_16bit_length() {
        let mut writer = FrameWriter::new(Role::Server, 1 << 18);
        let payload = vec![0x42; 256];
        writer.encode_binary(&payload);
        let d = writer.data();
        assert_eq!(d.len(), 4 + 256); // 2 + 2 (16-bit len) + 256
        assert_eq!(d[1] & 0x7F, 126); // extended 16-bit
        let len = u16::from_be_bytes([d[2], d[3]]);
        assert_eq!(len, 256);
    }

    #[test]
    fn max_encoded_len_small() {
        let server = FrameWriter::new(Role::Server, 65_536);
        assert_eq!(server.max_encoded_len(0), 2);
        assert_eq!(server.max_encoded_len(125), 2 + 125);
        assert_eq!(server.max_encoded_len(126), 4 + 126);

        let client = FrameWriter::new(Role::Client, 65_536);
        assert_eq!(client.max_encoded_len(0), 2 + 4);
        assert_eq!(client.max_encoded_len(125), 2 + 4 + 125);
    }

    #[test]
    fn round_trip_server() {
        use crate::ws::{FrameReader, Message};
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_text(b"Hello");

        let mut reader = FrameReader::builder().role(Role::Client).build();
        reader.read(writer.data()).unwrap();
        assert!(matches!(
            reader.next().unwrap().unwrap(),
            Message::Text("Hello")
        ));
    }

    #[test]
    fn round_trip_client() {
        use crate::ws::{FrameReader, Message};
        let mut writer = FrameWriter::new(Role::Client, 65_536);
        writer.encode_text(b"Hello");

        let mut reader = FrameReader::builder().role(Role::Server).build();
        reader.read(writer.data()).unwrap();
        assert!(matches!(
            reader.next().unwrap().unwrap(),
            Message::Text("Hello")
        ));
    }

    #[test]
    fn encode_close_code_round_trip() {
        use crate::ws::{CloseCode, FrameReader, Message};
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer
            .encode_close_code(CloseCode::Normal, "goodbye")
            .unwrap();

        let mut reader = FrameReader::builder().role(Role::Client).build();
        reader.read(writer.data()).unwrap();
        match reader.next().unwrap().unwrap() {
            Message::Close(cf) => {
                assert_eq!(cf.code, CloseCode::Normal);
                assert_eq!(cf.reason, "goodbye");
            }
            other => panic!("expected Close, got {other:?}"),
        }
    }

    #[test]
    fn ping_too_large_returns_err() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        assert!(matches!(
            writer.encode_ping(&[0; 126]),
            Err(super::EncodeError::ControlPayloadTooLarge(126))
        ));
    }

    #[test]
    fn encode_text_writer_round_trip() {
        use crate::ws::{FrameReader, Message};

        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer
            .encode_text_writer(|w| {
                use std::io::Write;
                w.write_all(b"test message")
            })
            .unwrap();

        let mut reader = FrameReader::builder().role(Role::Client).build();
        reader.read(writer.data()).unwrap();
        assert!(matches!(
            reader.next().unwrap().unwrap(),
            Message::Text("test message")
        ));
    }

    #[test]
    fn encode_text_writer_matches_fixed() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        let payload = b"Hello, world!";

        writer
            .encode_text_writer(|w| {
                use std::io::Write;
                w.write_all(payload)
            })
            .unwrap();
        let via_writer = writer.data().to_vec();

        writer.encode_text_fixed(payload.len(), |buf| {
            buf.copy_from_slice(payload);
        });
        let via_fixed = writer.data().to_vec();

        assert_eq!(via_writer, via_fixed);
    }

    #[test]
    fn advance_consumes_bytes() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        writer.encode_text(b"Hello");
        let total = writer.data().len();
        writer.advance(2);
        assert_eq!(writer.data().len(), total - 2);
    }

    #[test]
    fn encode_text_raw_matches_internal() {
        let mut writer = FrameWriter::new(Role::Server, 65_536);
        let payload = b"Hi";

        writer.encode_text(payload);
        let via_internal = writer.data().to_vec();

        let mut dst = vec![0u8; writer.max_encoded_len(payload.len())];
        let n = writer.encode_text_raw(payload, &mut dst);
        assert_eq!(&dst[..n], via_internal.as_slice());
    }
}
