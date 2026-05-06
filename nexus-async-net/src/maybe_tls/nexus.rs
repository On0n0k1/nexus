//! MaybeTls — plain TCP or TLS, unified async I/O (nexus-async-rt backend).
//!
//! Unlike the tokio variant which delegates TLS to `tokio-rustls`, this
//! drives nexus-net's sans-IO [`TlsCodec`] at the poll level. The codec
//! handles encrypt/decrypt; we shuttle bytes between it and the TCP stream.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use nexus_async_rt::{AsyncRead, AsyncWrite, TcpStream};

/// Async stream that may or may not be TLS-wrapped.
///
/// Created by connection builders based on the URL scheme.
pub enum MaybeTls {
    /// Plain TCP (ws://, http://).
    Plain(TcpStream),
    /// TLS over TCP (wss://, https://).
    #[cfg(feature = "tls")]
    Tls(Box<TlsInner>),
}

/// TLS state: a TCP stream plus the sans-IO codec and a write staging buffer.
///
/// Opaque to users — fields are `pub(crate)`. Exposed only because
/// [`MaybeTls::Tls`] holds a `Box<TlsInner>`.
#[cfg(feature = "tls")]
pub struct TlsInner {
    pub(crate) stream: TcpStream,
    pub(crate) codec: nexus_net::tls::TlsCodec,
    /// Ciphertext read from the transport but not yet accepted by rustls.
    pending_read: Vec<u8>,
    /// Ciphertext waiting to be flushed to the transport.
    pending_write: Vec<u8>,
}

#[cfg(feature = "tls")]
impl TlsInner {
    pub(crate) fn new(stream: TcpStream, codec: nexus_net::tls::TlsCodec) -> Self {
        Self {
            stream,
            codec,
            pending_read: Vec::with_capacity(8192),
            pending_write: Vec::with_capacity(16_384),
        }
    }
}

impl MaybeTls {
    /// Whether this connection is TLS-wrapped.
    pub fn is_tls(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            #[cfg(feature = "tls")]
            Self::Tls(_) => true,
        }
    }
}

// =============================================================================
// AsyncRead
// =============================================================================

impl AsyncRead for MaybeTls {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            MaybeTls::Tls(inner) => {
                if buf.is_empty() {
                    return Poll::Ready(Ok(0));
                }

                let mut tmp = [0u8; 8192];

                loop {
                    // Try already-buffered plaintext first.
                    let n = inner.codec.read_plaintext(buf).map_err(tls_to_io)?;
                    if n > 0 {
                        return Poll::Ready(Ok(n));
                    }

                    if !inner.pending_read.is_empty() {
                        process_pending_tls(&mut inner.codec, &mut inner.pending_read)
                            .map_err(tls_to_io)?;
                        continue;
                    }

                    // Need more ciphertext from the transport.
                    match Pin::new(&mut inner.stream).poll_read(cx, &mut tmp) {
                        Poll::Ready(Ok(0)) => return Poll::Ready(Ok(0)), // EOF
                        Poll::Ready(Ok(n)) => {
                            feed_tls_input(&mut inner.codec, &mut inner.pending_read, &tmp[..n])
                                .map_err(tls_to_io)?;
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }
}

// =============================================================================
// AsyncWrite
// =============================================================================

impl AsyncWrite for MaybeTls {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            MaybeTls::Tls(inner) => {
                // Drain any pending ciphertext before encrypting more.
                drain_pending(inner, cx)?;
                if !inner.pending_write.is_empty() {
                    // Couldn't drain — backpressure.
                    return Poll::Pending;
                }

                // Encrypt plaintext through the codec.
                inner.codec.encrypt(buf).map_err(tls_to_io)?;

                // Collect resulting ciphertext into pending_write.
                inner
                    .codec
                    .write_tls_to(&mut inner.pending_write)
                    .map_err(io::Error::other)?;

                // Best-effort drain of what we just encrypted.
                drain_pending(inner, cx)?;

                Poll::Ready(Ok(buf.len()))
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "tls")]
            MaybeTls::Tls(inner) => {
                // Drain any codec ciphertext not yet staged.
                if inner.codec.wants_write() {
                    inner
                        .codec
                        .write_tls_to(&mut inner.pending_write)
                        .map_err(io::Error::other)?;
                }

                // Drain pending_write to the transport.
                drain_pending(inner, cx)?;
                if !inner.pending_write.is_empty() {
                    return Poll::Pending;
                }

                // Flush the underlying stream.
                Pin::new(&mut inner.stream).poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            MaybeTls::Plain(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            MaybeTls::Tls(inner) => Pin::new(&mut inner.stream).poll_shutdown(cx),
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

#[cfg(feature = "tls")]
fn feed_tls_input(
    codec: &mut nexus_net::tls::TlsCodec,
    pending_read: &mut Vec<u8>,
    input: &[u8],
) -> Result<(), nexus_net::tls::TlsError> {
    debug_assert!(pending_read.is_empty());

    let consumed = codec.read_tls(input)?;
    if consumed == 0 {
        return Err(nexus_net::tls::TlsError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "TLS codec stopped before consuming buffered input",
        )));
    }

    codec.process_new_packets()?;
    if consumed < input.len() {
        pending_read.extend_from_slice(&input[consumed..]);
    }

    Ok(())
}

#[cfg(feature = "tls")]
fn process_pending_tls(
    codec: &mut nexus_net::tls::TlsCodec,
    pending_read: &mut Vec<u8>,
) -> Result<(), nexus_net::tls::TlsError> {
    let consumed = codec.read_tls(pending_read)?;
    if consumed == 0 {
        return Err(nexus_net::tls::TlsError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "TLS codec stopped before consuming buffered input",
        )));
    }

    codec.process_new_packets()?;
    pending_read.drain(..consumed);
    Ok(())
}

/// Drain the `pending_write` buffer to the transport, writing as much as the
/// socket will accept without blocking.
#[cfg(feature = "tls")]
fn drain_pending(inner: &mut TlsInner, cx: &mut Context<'_>) -> io::Result<()> {
    while !inner.pending_write.is_empty() {
        match Pin::new(&mut inner.stream).poll_write(cx, &inner.pending_write) {
            Poll::Ready(Ok(0)) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "transport write returned 0",
                ));
            }
            Poll::Ready(Ok(n)) => {
                inner.pending_write.drain(..n);
            }
            Poll::Ready(Err(e)) => return Err(e),
            Poll::Pending => return Ok(()), // will retry on next poll
        }
    }
    Ok(())
}

/// Convert a [`TlsError`](nexus_net::tls::TlsError) into an [`io::Error`].
#[cfg(feature = "tls")]
fn tls_to_io(e: nexus_net::tls::TlsError) -> io::Error {
    match e {
        nexus_net::tls::TlsError::Io(io_err) => io_err,
        other => io::Error::other(other),
    }
}

#[cfg(all(test, feature = "tls"))]
mod tests {
    use std::io::{Cursor, Write};
    use std::sync::Arc;

    use nexus_net::tls::{TlsCodec, TlsConfig};

    use super::{feed_tls_input, process_pending_tls};

    fn generate_self_signed() -> (Vec<rustls::pki_types::CertificateDer<'static>>, Vec<u8>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("cert generation");
        (
            vec![rustls::pki_types::CertificateDer::from(
                cert.cert.der().to_vec(),
            )],
            cert.key_pair.serialize_der(),
        )
    }

    fn connected_pair() -> (TlsCodec, rustls::ServerConnection) {
        let (cert_chain, key_der) = generate_self_signed();
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

        let mut c2s = Vec::new();
        let mut s2c = Vec::new();

        for _ in 0..64 {
            while client.wants_write() {
                client.write_tls_to(&mut c2s).unwrap();
            }

            if !c2s.is_empty() {
                server.read_tls(&mut Cursor::new(&c2s)).unwrap();
                server.process_new_packets().unwrap();
                c2s.clear();
            }

            while server.wants_write() {
                server.write_tls(&mut s2c).unwrap();
            }

            if !s2c.is_empty() {
                client.read_and_process_tls(&s2c).unwrap();
                s2c.clear();
            }

            if !client.is_handshaking() && !server.is_handshaking() {
                return (client, server);
            }
        }

        panic!("TLS handshake did not complete");
    }

    fn encrypt_server_payload(server: &mut rustls::ServerConnection, payload: &[u8]) -> Vec<u8> {
        server.writer().write_all(payload).unwrap();

        let mut ciphertext = Vec::new();
        while server.wants_write() {
            server.write_tls(&mut ciphertext).unwrap();
        }
        ciphertext
    }

    #[test]
    fn full_slice_tls_processing_can_hit_plaintext_backpressure() {
        let (mut client, mut server) = connected_pair();
        let payload = vec![b'x'; 64 * 1024];
        let ciphertext = encrypt_server_payload(&mut server, &payload);

        let error = client
            .read_and_process_tls(&ciphertext)
            .expect_err("full-slice processing should overfill rustls plaintext");

        assert!(
            error.to_string().contains("received plaintext buffer full"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn pending_read_flow_drains_plaintext_before_more_ciphertext() {
        let (mut client, mut server) = connected_pair();
        let payload = vec![b'x'; 64 * 1024];
        let ciphertext = encrypt_server_payload(&mut server, &payload);

        let mut pending_read = Vec::new();
        let mut plaintext = Vec::with_capacity(payload.len());
        let mut offset = 0;
        let mut dst = [0u8; 1024];

        for _ in 0..100_000 {
            let n = client.read_plaintext(&mut dst).unwrap();
            if n > 0 {
                plaintext.extend_from_slice(&dst[..n]);
                if plaintext.len() == payload.len() {
                    break;
                }
                continue;
            }

            if !pending_read.is_empty() {
                process_pending_tls(&mut client, &mut pending_read).unwrap();
                continue;
            }

            if offset < ciphertext.len() {
                let end = (offset + 8192).min(ciphertext.len());
                feed_tls_input(&mut client, &mut pending_read, &ciphertext[offset..end]).unwrap();
                offset = end;
                continue;
            }

            break;
        }

        assert_eq!(plaintext, payload);
        assert_eq!(offset, ciphertext.len());
        assert!(pending_read.is_empty());
    }
}
