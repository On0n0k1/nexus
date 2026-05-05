use std::io::{self, Read, Write};

use rustls::ClientConnection;
use rustls::pki_types::ServerName;

use super::{TlsConfig, TlsError};
use crate::ws::FrameReader;

/// Sans-IO TLS codec. Decrypts inbound bytes, encrypts outbound bytes.
///
/// Wraps a rustls `ClientConnection` with an API shaped for nexus-net:
/// feed raw TLS bytes in, get plaintext into a [`FrameReader`]; encrypt
/// plaintext from a [`FrameWriter`](crate::ws::FrameWriter) and flush to a socket.
///
/// # Usage
///
/// ```ignore
/// let config = TlsConfig::new()?;
/// let mut tls = TlsCodec::new(&config, "exchange.com")?;
///
/// // Handshake
/// while tls.is_handshaking() {
///     tls.write_tls_to(&mut socket)?;
///     tls.read_tls_from(&mut socket)?;
///     tls.process_new_packets()?;
/// }
///
/// // Steady state
/// tls.read_tls_from(&mut socket)?;
/// tls.process_into(&mut reader)?;
/// // ... reader.next() ...
/// ```
pub struct TlsCodec {
    inner: ClientConnection,
}

impl TlsCodec {
    /// Create a new TLS codec for the given hostname.
    ///
    /// The hostname is used for SNI (Server Name Indication) and
    /// certificate verification.
    pub fn new(config: &TlsConfig, hostname: &str) -> Result<Self, TlsError> {
        let server_name = ServerName::try_from(hostname.to_owned())
            .map_err(|_| TlsError::InvalidHostname(hostname.to_owned()))?;

        let conn = ClientConnection::new(config.inner.clone(), server_name)?;

        Ok(Self { inner: conn })
    }

    // =========================================================================
    // Inbound (socket → TLS → FrameReader)
    // =========================================================================

    /// Feed raw TLS bytes from a byte slice (sans-IO path).
    ///
    /// Returns the number of bytes consumed. **May be less than
    /// `src.len()`** — rustls's deframer can require a
    /// [`process_new_packets`](Self::process_new_packets) call before
    /// accepting more bytes. Most callers want
    /// [`read_and_process_tls`](Self::read_and_process_tls), which
    /// loops until the entire slice is consumed and is the correct
    /// primitive when bytes have already been read into a buffer
    /// (async paths, sans-IO pipelines).
    pub fn read_tls(&mut self, src: &[u8]) -> Result<usize, TlsError> {
        let mut cursor = io::Cursor::new(src);
        Ok(self.inner.read_tls(&mut cursor)?)
    }

    /// Feed buffered TLS bytes through rustls, looping until the entire
    /// slice is consumed.
    ///
    /// Use this anywhere code reads bytes into a buffer first (async
    /// paths, IO drivers that don't expose a `Read` trait, sans-IO
    /// pipelines) and then needs to push them into the codec. Sync paths
    /// reading directly from a [`Read`](std::io::Read) trait should use
    /// [`read_tls_from`](Self::read_tls_from) instead — rustls handles
    /// the consume-loop internally there.
    ///
    /// # Why a loop is required
    ///
    /// `rustls::Connection::read_tls` is not guaranteed to consume the
    /// full provided slice on a single call. It may consume part, return
    /// that count, and require [`process_new_packets`](Self::process_new_packets)
    /// before accepting more. Calling `read_tls(&buf)` once and ignoring
    /// the returned consumed count silently drops the unconsumed tail
    /// (issue #200 — a TLS handshake against a server that splits its
    /// response into multiple records inside a single TCP segment fails
    /// because the unconsumed bytes vanish).
    ///
    /// # Returns
    ///
    /// `Ok(src.len())` when the entire slice has been consumed and
    /// processed.
    ///
    /// # Errors
    ///
    /// - `TlsError::Io(InvalidData)` if rustls's deframer can't make
    ///   progress (returns 0 bytes consumed) despite the prior
    ///   `process_new_packets` call. Indicates a malformed or hostile
    ///   TLS stream.
    /// - Any error returned by [`read_tls`](Self::read_tls) or
    ///   [`process_new_packets`](Self::process_new_packets).
    pub fn read_and_process_tls(&mut self, src: &[u8]) -> Result<usize, TlsError> {
        let mut consumed = 0;
        while consumed < src.len() {
            let n = self.read_tls(&src[consumed..])?;
            if n == 0 {
                return Err(TlsError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "TLS codec stopped before consuming buffered input \
                     (rustls deframer cannot make progress)",
                )));
            }
            consumed += n;
            self.process_new_packets()?;
        }
        Ok(consumed)
    }

    /// Read raw TLS bytes from a socket.
    ///
    /// Returns the number of bytes read, or 0 on EOF.
    pub fn read_tls_from<R: Read>(&mut self, src: &mut R) -> io::Result<usize> {
        self.inner.read_tls(src)
    }

    /// Process buffered TLS records (decrypt).
    ///
    /// Call after [`read_tls`](Self::read_tls) or
    /// [`read_tls_from`](Self::read_tls_from) to decrypt any
    /// complete TLS records. This does not produce plaintext
    /// directly — call [`process_into`](Self::process_into) or
    /// [`read_plaintext`](Self::read_plaintext) afterwards.
    pub fn process_new_packets(&mut self) -> Result<(), TlsError> {
        self.inner.process_new_packets()?;
        Ok(())
    }

    /// Decrypt buffered TLS records and feed plaintext into a FrameReader.
    ///
    /// Combines [`process_new_packets`](Self::process_new_packets) and
    /// a read into the FrameReader in one call. Returns the number of
    /// plaintext bytes fed.
    pub fn process_into(&mut self, reader: &mut FrameReader) -> Result<usize, TlsError> {
        self.inner.process_new_packets()?;

        // Use BufRead::fill_buf to avoid ChunkVecBuffer::read overhead.
        // fill_buf returns a reference to buffered plaintext — one fewer
        // copy than Read::read which copies into an intermediate buffer.
        let mut rd = self.inner.reader();
        let chunk = match std::io::BufRead::fill_buf(&mut rd) {
            Ok(chunk) => chunk,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(0),
            Err(e) => return Err(TlsError::Io(e)),
        };
        if chunk.is_empty() {
            return Ok(0);
        }
        let n = chunk.len();
        if let Err(e) = reader.read(chunk) {
            return Err(TlsError::Io(io::Error::other(format!(
                "FrameReader buffer full: {e}"
            ))));
        }
        std::io::BufRead::consume(&mut rd, n);
        Ok(n)
    }

    /// Read decrypted plaintext into a buffer (sans-IO path).
    ///
    /// For users who want to feed bytes into FrameReader manually
    /// or use a different parser.
    pub fn read_plaintext(&mut self, dst: &mut [u8]) -> Result<usize, TlsError> {
        match self.inner.reader().read(dst) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(TlsError::Io(e)),
        }
    }

    // =========================================================================
    // Outbound (FrameWriter → TLS → socket)
    // =========================================================================

    /// Encrypt plaintext for sending.
    ///
    /// The encrypted bytes are buffered internally. Call
    /// [`write_tls_to`](Self::write_tls_to) to flush them to a socket.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<(), TlsError> {
        self.inner.writer().write_all(plaintext)?;
        Ok(())
    }

    /// Flush encrypted bytes to a socket.
    ///
    /// Returns the number of bytes written. Call in a loop or when
    /// [`wants_write`](Self::wants_write) returns true.
    pub fn write_tls_to<W: Write>(&mut self, dst: &mut W) -> io::Result<usize> {
        self.inner.write_tls(dst)
    }

    // =========================================================================
    // State
    // =========================================================================

    /// Whether the TLS handshake is still in progress.
    pub fn is_handshaking(&self) -> bool {
        self.inner.is_handshaking()
    }

    /// Whether the codec has buffered TLS data to read.
    pub fn wants_read(&self) -> bool {
        self.inner.wants_read()
    }

    /// Whether the codec has encrypted data to write.
    pub fn wants_write(&self) -> bool {
        self.inner.wants_write()
    }
}

impl std::fmt::Debug for TlsCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsCodec")
            .field("handshaking", &self.inner.is_handshaking())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;

    use super::*;

    // -------------------------------------------------------------------------
    // In-memory handshake scaffolding (lifted from examples/perf_tls.rs).
    // -------------------------------------------------------------------------

    fn generate_self_signed() -> (Vec<rustls::pki_types::CertificateDer<'static>>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("cert generation");
        (
            vec![rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec())],
            cert.key_pair.serialize_der(),
        )
    }

    /// Generate a 3-cert chain (root → intermediate → leaf) with RSA 4096
    /// keys. Real production-shape chain — chain has 3 certs each ~1.5KB
    /// of DER, so the TLS 1.3 server's Certificate message alone exceeds
    /// 4500 bytes. Combined with ServerHello + EncryptedExtensions +
    /// CertVerify (RSA 4096 signature is 512 bytes) + Finished, the
    /// server's first burst easily clears rustls's `READ_SIZE = 4096`
    /// per-call deframer cap — exactly the partial-consumption surface
    /// birch hit against polymarket.
    fn generate_rsa_4096_chain() -> (Vec<rustls::pki_types::CertificateDer<'static>>, Vec<u8>) {
        use rcgen::{
            BasicConstraints, CertificateParams, IsCa, KeyPair, RsaKeySize,
            PKCS_RSA_SHA256,
        };

        // Root CA (RSA 4096, self-signed, CA-flagged).
        let root_key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_4096)
            .expect("root key");
        let mut root_params = CertificateParams::new(Vec::<String>::new()).expect("root params");
        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let root_cert = root_params.self_signed(&root_key).expect("root self-sign");

        // Intermediate (RSA 4096, signed by root, CA-flagged).
        let int_key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_4096)
            .expect("intermediate key");
        let mut int_params = CertificateParams::new(Vec::<String>::new()).expect("intermediate params");
        int_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let int_cert = int_params
            .signed_by(&int_key, &root_cert, &root_key)
            .expect("intermediate signed by root");

        // Leaf (RSA 4096, signed by intermediate, SAN=localhost).
        let leaf_key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_4096)
            .expect("leaf key");
        let leaf_params = CertificateParams::new(vec!["localhost".to_string()])
            .expect("leaf params");
        let leaf_cert = leaf_params
            .signed_by(&leaf_key, &int_cert, &int_key)
            .expect("leaf signed by intermediate");

        // Server sends [leaf, intermediate, root] in the Certificate
        // message. Three RSA 4096 certs ≈ 4.5KB chain.
        let chain = vec![
            rustls::pki_types::CertificateDer::from(leaf_cert.der().to_vec()),
            rustls::pki_types::CertificateDer::from(int_cert.der().to_vec()),
            rustls::pki_types::CertificateDer::from(root_cert.der().to_vec()),
        ];
        (chain, leaf_key.serialize_der())
    }

    /// In-memory pipe for handshake bytes.
    struct MemPipe {
        buf: Vec<u8>,
    }

    impl MemPipe {
        fn new() -> Self {
            Self { buf: Vec::new() }
        }

        fn write_to(&mut self, data: &[u8]) {
            self.buf.extend_from_slice(data);
        }

        fn read_from(&mut self, dst: &mut [u8]) -> usize {
            let n = dst.len().min(self.buf.len());
            dst[..n].copy_from_slice(&self.buf[..n]);
            self.buf.drain(..n);
            n
        }

        fn len(&self) -> usize {
            self.buf.len()
        }
    }

    /// Build the server side and capture its first multi-record handshake
    /// burst (ServerHello + EncryptedExtensions + Certificate + CertVerify +
    /// Finished under TLS 1.3 — several records pushed back-to-back). The
    /// returned `server_out` is the slice we feed to the client `TlsCodec`
    /// to exercise the partial-consumption surface.
    fn setup_and_capture_server_burst(
        cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
        key_der: Vec<u8>,
    ) -> (TlsCodec, rustls::ServerConnection, Vec<u8>) {
        let key = rustls::pki_types::PrivateKeyDer::try_from(key_der).unwrap();
        let server_config = Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(cert_chain, key)
                .unwrap(),
        );
        let mut server = rustls::ServerConnection::new(server_config).unwrap();

        let client_config = TlsConfig::builder().danger_no_verify().build().unwrap();
        let mut client = TlsCodec::new(&client_config, "localhost").unwrap();

        let mut c2s = MemPipe::new();
        let mut s2c = MemPipe::new();

        // Client writes ClientHello.
        // Loop `while wants_write()` (mirroring the server side below)
        // for defense-in-depth — if a future rustls or cert config splits
        // the ClientHello across multiple write batches, a single
        // write_tls_to call would leave bytes pending in the codec.
        while client.wants_write() {
            let mut cursor = Cursor::new(Vec::new());
            client.write_tls_to(&mut cursor).unwrap();
            c2s.write_to(cursor.get_ref());
        }

        // Server consumes ClientHello.
        let mut tmp = vec![0u8; 16384];
        let n = c2s.read_from(&mut tmp);
        server
            .read_tls(&mut Cursor::new(&tmp[..n]))
            .expect("server reads ClientHello");
        server.process_new_packets().unwrap();

        // Server writes its multi-record burst.
        while server.wants_write() {
            let mut cursor = Cursor::new(Vec::new());
            server.write_tls(&mut cursor).unwrap();
            s2c.write_to(cursor.get_ref());
        }

        let mut server_out = vec![0u8; s2c.len()];
        let n = s2c.read_from(&mut server_out);
        assert!(n > 0, "server should have produced handshake bytes");
        server_out.truncate(n);

        (client, server, server_out)
    }

    // -------------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------------

    /// Regression test for issue #200.
    ///
    /// Pre-fix: `read_tls(&buf)` may consume only part of `buf`. Calling
    /// code in nexus-async-net + nexus-net's tls/stream.rs ignored the
    /// returned consumed count, dropping the unconsumed tail and stalling
    /// the TLS handshake. Post-fix: `read_and_process_tls` loops until the
    /// entire slice is consumed.
    #[test]
    fn read_and_process_tls_consumes_full_slice() {
        let (chain, key) = generate_self_signed();
        let (mut client, _server, server_out) = setup_and_capture_server_burst(chain, key);

        let consumed = client
            .read_and_process_tls(&server_out)
            .expect("helper must consume the full slice");

        assert_eq!(
            consumed,
            server_out.len(),
            "helper must consume every byte (issue #200)"
        );
        assert!(
            client.wants_write(),
            "client should have produced its handshake response"
        );
    }

    /// Stricter exercise: feed the captured server bytes one byte per
    /// `read_and_process_tls` call. Catches a class of bugs where the
    /// helper itself drops bytes between calls or skips the
    /// `process_new_packets` step in some iterations.
    #[test]
    fn read_and_process_tls_byte_at_a_time() {
        let (chain, key) = generate_self_signed();
        let (mut client, _server, server_out) = setup_and_capture_server_burst(chain, key);

        for byte in &server_out {
            client
                .read_and_process_tls(std::slice::from_ref(byte))
                .expect("byte-at-a-time must succeed");
        }

        assert!(
            client.wants_write(),
            "client should have produced its handshake response \
             after byte-at-a-time consumption"
        );
    }

    /// **The actual end-to-end regression test for issue #200.**
    ///
    /// The other tests in this module either don't exercise the helper's
    /// multi-iteration loop (`read_and_process_tls_consumes_full_slice`
    /// uses a small burst that consumes in one inner iteration;
    /// `read_and_process_tls_byte_at_a_time` invokes the helper many times
    /// with 1-byte slices but each invocation has a 1-iteration loop),
    /// or test only rustls's contract without exercising our helper
    /// (`bare_read_tls_partially_consumes_large_slice`).
    ///
    /// This test uses a 3-cert RSA 4096 chain to push the server's first
    /// handshake burst past rustls's `READ_SIZE = 4096` per-call cap. The
    /// helper is fed the whole burst in ONE call; its internal loop must
    /// iterate multiple times to consume everything. This is exactly the
    /// shape birch hit against polymarket.
    #[test]
    fn read_and_process_tls_handles_oversize_burst() {
        let (chain, key) = generate_rsa_4096_chain();
        let (mut client, _server, server_out) = setup_and_capture_server_burst(chain, key);

        // Confirm the test is actually exercising the partial-consumption
        // path. If this assertion fails, future contributors investigating
        // know the burst-size assumption broke (e.g., rustls raised
        // READ_SIZE, or the cert chain shrank). Bump the chain size or
        // the key size in `generate_rsa_4096_chain` to restore.
        assert!(
            server_out.len() > 4096,
            "burst must exceed READ_SIZE to exercise multi-iteration loop, \
             got {} bytes — bump cert chain in generate_rsa_4096_chain",
            server_out.len()
        );

        let consumed = client
            .read_and_process_tls(&server_out)
            .expect("helper must consume the full slice across multiple iterations");

        assert_eq!(
            consumed,
            server_out.len(),
            "helper must consume every byte across the multi-iteration loop \
             (issue #200 — the actual partial-consumption surface)"
        );
        assert!(
            client.wants_write(),
            "client should have produced its handshake response after \
             consuming the oversize burst"
        );
    }

    /// Demonstrates the contract difference between `read_tls` and
    /// `read_and_process_tls` (issue #200).
    ///
    /// rustls 0.23 clamps each `read_tls` call to a 4096-byte chunk per
    /// the deframer's internal `READ_SIZE` (see
    /// `rustls::msgs::deframer::buffers::DeframerVecBuffer::prepare_read`).
    /// Any slice larger than that is partially consumed in one call —
    /// the buggy pattern `codec.read_tls(&buf)?; process_new_packets()?;`
    /// silently drops everything past byte 4096 because the call site
    /// ignores the returned count.
    ///
    /// In the real-world failure (Polymarket's WSS endpoint) the server
    /// emits a multi-record TLS 1.3 handshake burst (ServerHello +
    /// EncryptedExtensions + Certificate + CertVerify + Finished) that
    /// can easily exceed 4096 bytes when the cert chain is non-trivial,
    /// or arrive concatenated inside a single TCP segment. The server
    /// times out after ~15s waiting for the client's Finished record
    /// that never comes, because the client never decrypted past the
    /// 4096th byte.
    ///
    /// The 4096-byte cap is rustls-internal and may change in future
    /// versions. If it does, this assertion needs adjusting (raise the
    /// input size above the new cap), but the helper's loop remains
    /// correct — partial consumption is the documented contract of
    /// `Connection::read_tls`, not an implementation accident.
    #[test]
    fn bare_read_tls_partially_consumes_large_slice() {
        let client_config = TlsConfig::builder().danger_no_verify().build().unwrap();
        let mut client = TlsCodec::new(&client_config, "localhost").unwrap();

        // Larger than rustls's READ_SIZE (4096) per-call cap. Contents
        // don't need to be valid TLS — `read_tls` only buffers; it does
        // not validate. (Validation happens in `process_new_packets`,
        // which we do not call.)
        let oversize = vec![0u8; 8192];

        let consumed = client
            .read_tls(&oversize)
            .expect("read_tls buffers without validating");

        assert!(
            consumed < oversize.len(),
            "expected partial consumption (issue #200 surface): \
             rustls should clamp to its per-call READ_SIZE cap, but \
             consumed {consumed} of {} bytes in one call",
            oversize.len(),
        );
    }
}
