# Changelog

All notable changes to nexus-net are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.6.2] — 2026-05-07

The "TLS plaintext-backpressure + steady-state hardening" release.
Closes [#205](https://github.com/Abso1ut3Zer0/nexus/pull/205) — birch
diagnosed that even after the 0.6.1 handshake byte-loss fix, steady-
state TLS app-data >16 KiB could still overflow rustls's internal
plaintext buffer (`received plaintext buffer full`) because the
helper kept feeding ciphertext without giving the caller a chance to
drain plaintext. This release closes that bug surface plus a chain of
related issues surfaced across three audit passes (code review +
hot-path + deep audit).

### Added

- `TlsCodec::read_tls_step(&[u8]) -> Result<usize, TlsError>` — single
  packet-step primitive (`read_tls` + `process_new_packets` once,
  returns bytes consumed). Use for streaming app-data adapters where
  the caller alternates ciphertext input with plaintext output. Avoids
  overflowing rustls's plaintext queue.
- `TlsCodec::try_encrypt(&[u8]) -> Result<usize, TlsError>` — chunked
  variant of `encrypt`. Returns the number of plaintext bytes accepted
  (which may be less than `plaintext.len()`). Implements the proper
  `AsyncWrite::poll_write` contract for plaintexts that exceed
  rustls's outbound queue cap.
- `TlsCodec::set_buffer_limit(Option<usize>)` — pass-through to
  rustls's outbound plaintext queue limit (`DEFAULT_BUFFER_LIMIT =
  64 KiB`). `None` for unlimited.
- `TlsCodec::send_close_notify()` — idempotent rustls wrapper. Used
  by `TlsStream::poll_shutdown` to send a TLS close_notify alert
  before TCP FIN.
- `WriteBuf::spare(&mut self) -> &mut [u8]` and
  `WriteBuf::filled(&mut self, n: usize)` — symmetric with
  `ReadBuf::spare`/`filled`. Enables cursor-FIFO usage where a sans-IO
  codec writes directly into the buffer's tail and commits with
  `filled(n)`.
- `TlsStream::with_capacities(stream, codec, pending_read_cap,
  pending_write_cap)` — explicit buffer capacity tuning. Default
  `new()` uses 8 KiB / 64 KiB.
- `TlsStream::TMP_SIZE` (8 KiB) and
  `TlsStream::DEFAULT_PENDING_WRITE_CAPACITY` (64 KiB) — public
  constants documenting the default sizing and the lower bound for
  `pending_read_cap` (must be ≥ `TMP_SIZE` else `with_capacities`
  panics).
- `TlsStream::set_buffer_limit(Option<usize>)` — convenience
  pass-through to the inner codec.
- Module-level "Choosing an input primitive" decision matrix in
  `tls/mod.rs` covering when to use `read_tls_step` vs
  `read_and_process_tls` vs (deprecated) `read_tls`.

### Changed

- `WriteBuf::advance(n)` now auto-resets `head`/`tail` to
  `reset_offset` when the buffer becomes empty post-advance — matches
  `ReadBuf::advance` semantics. Backwards-compatible: existing callers
  that follow `advance` with `clear()` continue to work; the `clear()`
  is now redundant.

### Deprecated

- `TlsCodec::read_tls(&[u8]) -> Result<usize, TlsError>` — direct
  rustls wrapper that doesn't encode partial-consumption semantics.
  Migrate to `read_tls_step` (streaming) or `read_and_process_tls`
  (bounded handshake input). The bare primitive remains for advanced
  use; its docs now warn against direct use.
- `TlsCodec::encrypt(&[u8]) -> Result<(), TlsError>` — all-or-nothing
  shape that errors with `WriteZero` when plaintext exceeds rustls's
  outbound queue cap. Migrate to `try_encrypt` for chunked semantics.

### Fixed

- `TlsStream::poll_read` (`tls/stream.rs:207`) — steady-state app-data
  bursts ≥16 KiB no longer error with `received plaintext buffer
  full`. Adapter now uses `read_tls_step` + a `pending_read: ReadBuf`
  spillover, drains plaintext between packet steps. Symmetric fix
  for `nexus-async-net::MaybeTls::poll_read` shipped in
  nexus-async-net 0.6.2.
- `TlsStream::poll_write` correctly chunks plaintexts larger than
  rustls's outbound queue cap (default 64 KiB). Previously, a single
  `write_all(&[u8; 100_000])` would surface a confusing `WriteZero`
  error from rustls's writer; now the adapter uses `try_encrypt` and
  returns `Ok(N)` where N may be less than the input length, deferring
  to the standard `AsyncWrite` retry contract.
- `TlsStream::poll_shutdown` now queues a TLS `close_notify` alert,
  flushes the resulting ciphertext, then closes the transport.
  Pre-fix: only TCP FIN was sent, peer treated EOF as a truncation
  signal and errored mid-stream when reading the last bytes (matches
  rustls's defensive behavior). Doc-comment updated to reflect actual
  semantics.

### Internal

- `pending_read: ReadBuf` and `pending_write: WriteBuf` migrated from
  `Vec<u8>` (which used `drain(..n)` — an O(n) memmove on every TLS
  packet step under partial socket reads/writes). Cursor-based buffers
  give O(1) advance with auto-reset to start when fully drained.
- Per-poll `tmp: Box<[u8; 8192]>` hoisted into the struct from a
  per-poll stack alloca + memset. Eliminates ~256 cycles + L1
  pollution per `poll_read` (Casey-audit confirmed via cargo-asm).
- `#[inline]` on `TlsCodec::{read_tls_step, read_plaintext, encrypt,
  is_handshaking, wants_read, wants_write}` — eliminates cross-crate
  function calls per packet step under default codegen-units=16.
- New tokio integration tests in
  `tests/tls_stream_async_backpressure.rs`: oversize app-data burst
  (256 KiB), large write chunking (256 KiB), tiny pending_write
  capacity drain-and-refill, drop-mid-poll regression. Side-by-side
  codec-level demonstrators
  (`adapter_pattern_with_read_and_process_tls_overflows_on_oversize_chunks`
  + `adapter_pattern_with_read_tls_step_handles_oversize_chunks`) pin
  the bug at 32 KiB chunks: identical adapter loop, identical input,
  only the helper differs.
- `const_assert!(TMP_SIZE <= 16 * 1024)` guards the latent
  handshake-piggyback overflow (TLS 1.3 servers can piggyback app-data
  in the same TCP segment as ServerFinished). The architectural fix
  for this — hoisting handshake into `TlsInner` so `pending_read` is
  reachable for direct stash without an intermediate allocation — is
  filed as a 0.7.0 follow-up.

### Migration notes

For most users, `cargo update -p nexus-net` is sufficient. Public API
behavior of high-level types (`TlsStream`, builders) is unchanged.

Direct callers of `TlsCodec::read_tls` or `TlsCodec::encrypt` will see
deprecation warnings — migrate to `read_tls_step` (streaming) or
`try_encrypt` (chunked) per the module-level decision matrix in
`tls/mod.rs`. The deprecated primitives continue to work in 0.6.x;
removal is planned for 0.7.0 alongside the architectural refactor.

## [0.6.1] — 2026-05-05

The "TLS handshake byte-loss" release. Closes
[#200](https://github.com/Abso1ut3Zer0/nexus/issues/200) — TLS
handshakes against servers that emit multi-record handshake bursts
exceeding rustls's per-call read cap (e.g., long cert chains, OCSP
stapling, large certs) could silently drop unconsumed bytes,
stalling the handshake and producing
`Io(UnexpectedEof, "closed during TLS handshake")` after the
server's timeout.

### Added

- `TlsCodec::read_and_process_tls(&[u8]) -> Result<usize, TlsError>` —
  helper that loops `read_tls` + `process_new_packets` until the
  entire input slice is consumed. Use anywhere code reads ciphertext
  bytes into a buffer first (async paths, sans-IO pipelines, IO
  drivers without a `Read` trait) and then needs to push them into
  the codec. Returns `Ok(src.len())` on success or
  `TlsError::Io(InvalidData)` if rustls's deframer can't make
  progress despite intervening `process_new_packets` calls.

### Fixed

- `TlsStream::handshake_async` (line 166) and `poll_read` (line 207)
  now use `read_and_process_tls` instead of the bare
  `read_tls` + `process_new_packets` pair. Pre-fix,
  `rustls::Connection::read_tls(&mut Cursor)` could consume only part
  of the slice and the unconsumed tail was silently lost when `tmp`
  got overwritten on the next loop iteration. This was the bug birch
  reported in #200 against `wss://ws-subscriptions-clob.polymarket.com`.

### Internal

- New regression tests: `tls::codec::tests::read_and_process_tls_handles_oversize_burst`
  (in-process, deterministic; uses 10-cert ECDSA-P256 chain to push
  the burst past rustls's `READ_SIZE = 4096`) and
  `wss_echo::local_wss_echo_with_oversize_handshake_burst` (hermetic
  localhost TLS+WS+frame-echo). Plus
  `tls::codec::tests::bare_read_tls_partially_consumes_large_slice`
  documents rustls's contract directly.
- `read_tls`'s doc-comment now points readers at
  `read_and_process_tls` for the common case (callers who already
  have bytes in a buffer).

### Migration notes

`cargo update -p nexus-net` is the only change required for most
users. The bare `read_tls` API is unchanged — anyone implementing
their own consume loop on top of it continues to work. New code
that pre-buffers bytes should reach for `read_and_process_tls`
instead.

## [0.6.0] — 2026-04-15

Earlier 0.6.x and prior versions are not documented in this
CHANGELOG. See git history and GitHub release notes for details.
