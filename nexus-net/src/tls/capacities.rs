//! Per-connection TLS buffer sizing. Construct via [`TlsBufferCapacities::builder`]
//! (or use [`TlsBufferCapacities::default`] for the standard sizes).

/// Per-connection TLS buffer sizing.
///
/// Construct via [`builder()`](Self::builder) — the builder applies
/// sane defaults so callers only specify what they want to change.
/// [`Default::default()`](Self::default) gives the same result as
/// `builder().build()`.
///
/// ```
/// use nexus_net::tls::TlsBufferCapacities;
///
/// // Defaults: 18 KiB inbound, 16 KiB outbound, rustls default queue.
/// let _ = TlsBufferCapacities::default();
///
/// // Override outbound:
/// let _ = TlsBufferCapacities::builder()
///     .pending_write(64 * 1024)
///     .build();
///
/// // Tight memory for many connections:
/// let _ = TlsBufferCapacities::builder()
///     .pending_write(8 * 1024)
///     .rustls_plaintext_limit(Some(16 * 1024))
///     .build();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TlsBufferCapacities {
    read_chunk: usize,
    pending_write: usize,
    rustls_plaintext_limit: Option<usize>,
}

impl TlsBufferCapacities {
    /// Builder with sane defaults: 18 KiB inbound, 16 KiB outbound,
    /// rustls's plaintext queue limit unchanged (rustls default 64 KiB).
    ///
    /// **Inbound (`read_chunk`):** 18 KiB covers rustls's maximum
    /// plaintext record size (16,384 bytes) plus its 5-byte header
    /// and AEAD auth tag (up to 256 bytes for some ciphers) with
    /// slack. A peer producing a max-sized TLS 1.3 record fits in one
    /// transport read; smaller defaults can fail with `pending_read
    /// full but rustls cannot decode a record` against such peers.
    ///
    /// **Outbound (`pending_write`):** 16 KiB covers typical
    /// small-message workloads (order entry, market-data
    /// subscriptions). Asymmetry vs inbound is intentional —
    /// outbound has no rustls record-size constraint and most
    /// trading writes are <2 KiB.
    ///
    /// **Total resident: ~34 KiB per connection** (inbound +
    /// outbound + rustls state). Worst-case under bursty inbound
    /// can reach ~98 KiB if rustls's outbound plaintext queue fills
    /// to its 64 KiB cap — see `rustls_plaintext_limit`.
    pub const fn builder() -> TlsBufferCapacitiesBuilder {
        TlsBufferCapacitiesBuilder {
            read_chunk: 18 * 1024,
            pending_write: 16 * 1024,
            rustls_plaintext_limit: None,
        }
    }

    /// Inbound `pending_read` buffer capacity. Doubles as the
    /// transport read chunk size — the adapter reads directly into
    /// `pending_read.spare()`.
    #[inline]
    pub const fn read_chunk(&self) -> usize {
        self.read_chunk
    }

    /// Outbound `pending_write` buffer capacity.
    #[inline]
    pub const fn pending_write(&self) -> usize {
        self.pending_write
    }

    /// Override rustls's outbound plaintext queue limit. `None` keeps
    /// rustls's default (64 KiB). `Some(n)` calls `set_buffer_limit`
    /// on the codec immediately after construction. Trading workloads
    /// with strict per-connection memory budgets can drop this to
    /// 8–16 KiB; bulk-transfer workloads can raise it.
    #[inline]
    pub const fn rustls_plaintext_limit(&self) -> Option<usize> {
        self.rustls_plaintext_limit
    }
}

impl Default for TlsBufferCapacities {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Builder for [`TlsBufferCapacities`].
#[derive(Debug, Clone, Copy)]
pub struct TlsBufferCapacitiesBuilder {
    read_chunk: usize,
    pending_write: usize,
    rustls_plaintext_limit: Option<usize>,
}

impl TlsBufferCapacitiesBuilder {
    /// Inbound `pending_read` buffer capacity. Doubles as the
    /// transport read chunk size — the adapter reads directly into
    /// `pending_read.spare()`. Default 18 KiB covers rustls's max
    /// plaintext record (16,384 bytes) plus header and AEAD overhead.
    /// Smaller values still work for typical peers — rustls's deframer
    /// accumulates state across calls — but a peer producing a
    /// max-sized TLS 1.3 record can fail against `read_chunk < 16,640`.
    /// Larger values reduce syscall count for bulk-transfer workloads
    /// at the cost of per-connection memory.
    pub const fn read_chunk(mut self, bytes: usize) -> Self {
        self.read_chunk = bytes;
        self
    }

    /// Outbound `pending_write` buffer capacity. Default 16 KiB
    /// suffices for typical small-message workloads (order entry,
    /// market-data subscriptions, control messages). Bulk-transfer
    /// workloads (large snapshots, file uploads over TLS) may benefit
    /// from raising this to 32–64 KiB to reduce drain/refill cycles
    /// in [`encrypt`](super::TlsCodec::encrypt).
    pub const fn pending_write(mut self, bytes: usize) -> Self {
        self.pending_write = bytes;
        self
    }

    /// Cap rustls's outbound plaintext queue at `bytes`. `None`
    /// (default) keeps rustls's built-in default of 64 KiB. Lower
    /// values reduce per-connection worst-case memory under bursty
    /// inbound; raising it benefits bulk-transfer workloads that
    /// `encrypt()` large buffers in one call. Applied via
    /// [`TlsCodec::set_buffer_limit`](super::TlsCodec::set_buffer_limit)
    /// immediately after construction.
    pub const fn rustls_plaintext_limit(mut self, bytes: Option<usize>) -> Self {
        self.rustls_plaintext_limit = bytes;
        self
    }

    /// Materialize the capacities. Panics if either capacity is 0 —
    /// `pending_read` and `pending_write` must hold actual bytes.
    /// Catches `tls_buffer_capacities(builder.read_chunk(0).build())`
    /// at the call site rather than as a cryptic crash deep in
    /// connection setup (`WriteBuf::new(0, 0)` panics on `headroom >=
    /// capacity`; `ReadBuf::with_capacity(0)` produces a buffer that
    /// fails the next `poll_read`).
    pub const fn build(self) -> TlsBufferCapacities {
        assert!(
            self.read_chunk > 0,
            "TlsBufferCapacities::read_chunk must be > 0"
        );
        assert!(
            self.pending_write > 0,
            "TlsBufferCapacities::pending_write must be > 0"
        );
        TlsBufferCapacities {
            read_chunk: self.read_chunk,
            pending_write: self.pending_write,
            rustls_plaintext_limit: self.rustls_plaintext_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_builder_no_overrides() {
        let a = TlsBufferCapacities::default();
        let b = TlsBufferCapacities::builder().build();
        assert_eq!(a.read_chunk(), b.read_chunk());
        assert_eq!(a.pending_write(), b.pending_write());
        assert_eq!(a.rustls_plaintext_limit(), b.rustls_plaintext_limit());
        assert_eq!(a.read_chunk(), 18 * 1024);
        assert_eq!(a.pending_write(), 16 * 1024);
        assert!(a.rustls_plaintext_limit().is_none());
    }

    #[test]
    fn builder_overrides() {
        let c = TlsBufferCapacities::builder()
            .read_chunk(4096)
            .pending_write(32 * 1024)
            .rustls_plaintext_limit(Some(16 * 1024))
            .build();
        assert_eq!(c.read_chunk(), 4096);
        assert_eq!(c.pending_write(), 32 * 1024);
        assert_eq!(c.rustls_plaintext_limit(), Some(16 * 1024));
    }

    #[test]
    #[should_panic(expected = "read_chunk must be > 0")]
    fn build_panics_on_zero_read_chunk() {
        let _ = TlsBufferCapacities::builder().read_chunk(0).build();
    }

    #[test]
    #[should_panic(expected = "pending_write must be > 0")]
    fn build_panics_on_zero_pending_write() {
        let _ = TlsBufferCapacities::builder().pending_write(0).build();
    }
}
