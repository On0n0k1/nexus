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
/// // Defaults: 8 KiB read chunk, 64 KiB pending_write.
/// let _ = TlsBufferCapacities::default();
///
/// // Override one knob:
/// let _ = TlsBufferCapacities::builder()
///     .pending_write(16 * 1024)
///     .build();
///
/// // Both:
/// let _ = TlsBufferCapacities::builder()
///     .read_chunk(4 * 1024)
///     .pending_write(8 * 1024)
///     .build();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TlsBufferCapacities {
    read_chunk: usize,
    pending_write: usize,
}

impl TlsBufferCapacities {
    /// Builder with sane defaults (8 KiB read chunk, 64 KiB pending_write).
    pub const fn builder() -> TlsBufferCapacitiesBuilder {
        TlsBufferCapacitiesBuilder {
            read_chunk: 8 * 1024,
            pending_write: 64 * 1024,
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
}

impl TlsBufferCapacitiesBuilder {
    /// Inbound `pending_read` buffer capacity. Doubles as the
    /// transport read chunk size — the adapter reads directly into
    /// `pending_read.spare()`. Default 8 KiB suffices for any
    /// well-formed TLS stream: rustls's deframer accumulates state
    /// across calls, so a single TLS record larger than `read_chunk`
    /// (max plaintext record = 16 KiB) is consumed across multiple
    /// transport reads without overflowing the buffer. Larger
    /// `read_chunk` reduces syscall count for bulk-transfer workloads
    /// at the cost of per-connection memory.
    pub const fn read_chunk(mut self, bytes: usize) -> Self {
        self.read_chunk = bytes;
        self
    }

    /// Outbound `pending_write` buffer capacity. 64 KiB matches
    /// rustls's `DEFAULT_BUFFER_LIMIT`. Trading workloads with small
    /// messages can drop this to 8–16 KiB to reduce per-connection
    /// footprint.
    pub const fn pending_write(mut self, bytes: usize) -> Self {
        self.pending_write = bytes;
        self
    }

    pub const fn build(self) -> TlsBufferCapacities {
        TlsBufferCapacities {
            read_chunk: self.read_chunk,
            pending_write: self.pending_write,
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
        assert_eq!(a.read_chunk(), 8 * 1024);
        assert_eq!(a.pending_write(), 64 * 1024);
    }

    #[test]
    fn builder_overrides() {
        let c = TlsBufferCapacities::builder()
            .read_chunk(4096)
            .pending_write(32 * 1024)
            .build();
        assert_eq!(c.read_chunk(), 4096);
        assert_eq!(c.pending_write(), 32 * 1024);
    }
}
