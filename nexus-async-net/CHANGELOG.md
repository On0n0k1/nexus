# Changelog

All notable changes to nexus-async-net are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.6.2] — 2026-05-07

The "TLS plaintext-backpressure + steady-state hardening" release.
Picks up [nexus-net 0.6.2](../nexus-net/CHANGELOG.md) and applies the
matching fix to the nexus-async-rt backend (`MaybeTls`). Closes
[#205](https://github.com/Abso1ut3Zer0/nexus/pull/205).

### Added

- `WsStreamBuilder::tls_buffer_capacities(read_cap, write_cap)` —
  tune the TLS adapter's `pending_read` (default 8 KiB) and
  `pending_write` (default 64 KiB) buffer sizes per connection.
  Trading workloads with small frequent messages can reduce
  `pending_write` to 8–16 KiB to lower per-connection memory
  footprint (~81 KiB → ~33 KiB at 16 KiB write cap).
- `HttpConnectionBuilder::tls_buffer_capacities(read_cap, write_cap)`
  — same plumbing for the REST connection builder.
- `MaybeTls::TlsInner::TMP_SIZE` (8 KiB) and
  `DEFAULT_PENDING_WRITE_CAPACITY` (64 KiB) crate-internal constants
  documenting the default sizing. `pending_read_cap` must be at least
  `TMP_SIZE` (constructor panics otherwise).

### Changed

- Dependency declaration: `nexus-net` 0.6.1 → 0.6.2. Pulls in the new
  `read_tls_step`, `try_encrypt`, `set_buffer_limit`, and
  `send_close_notify` primitives + the cursor-FIFO `WriteBuf` API.

### Fixed

- `MaybeTls::poll_read` (`maybe_tls/nexus.rs:85` for the
  `feature = "nexus"` backend) — same plaintext-buffer-full bug as
  nexus-net's `TlsStream::poll_read`. Steady-state app-data bursts
  ≥16 KiB no longer error with `received plaintext buffer full`.
  Adapter now uses `read_tls_step` + `pending_read: ReadBuf`
  spillover.
- `MaybeTls::poll_write` correctly chunks plaintexts larger than
  rustls's outbound plaintext queue cap (default 64 KiB) via
  `try_encrypt`, returning `Ok(N)` where N may be less than the
  input length per the `AsyncWrite` retry contract.
- `MaybeTls::poll_shutdown` queues a TLS `close_notify` alert before
  closing the transport — peers no longer see EOF-without-close_notify
  truncation alerts on graceful disconnect.

### Internal

- `pending_read` and `pending_write` migrated from `Vec<u8>` to
  cursor-FIFO `ReadBuf` / `WriteBuf` (no per-write memmove; auto-reset
  on full drain).
- Per-poll `tmp: Box<[u8; 8192]>` hoisted into `TlsInner` from
  per-poll stack alloca + memset (matches the nexus-net change).
- New integration test
  `tests/maybe_tls_nexus_backpressure.rs` — 3 tests mirroring the
  tokio-side suite (oversize app-data burst, large write chunking,
  oversize write with tiny `pending_write_cap`). Driven through
  `nexus_async_rt::Runtime` + sync server thread; activated under
  `--features nexus,tls`.

### Migration notes

`cargo update -p nexus-async-net` is the only change required for
most users. Public API of `WsClient` / `HttpClient` and their
builders is backwards-compatible; the new
`tls_buffer_capacities(...)` setters are optional.

## [0.6.1] — 2026-05-05

The "TLS handshake byte-loss" release. Picks up the
[nexus-net 0.6.1](../nexus-net/CHANGELOG.md) fix at all three async
TLS call sites and bumps the `nexus-async-rt` dependency declaration
to 0.5.0 (the hardening-series release that landed earlier today).

### Fixed

- `ws::nexus::stream::handshake_tls` (line 71),
  `rest::nexus::connection` (line 71), and
  `maybe_tls::nexus::TlsInner::poll_read` (line 83) now use
  `TlsCodec::read_and_process_tls` instead of the bare
  `read_tls` + `process_new_packets` pair. Closes the production
  bug birch reported in
  [#200](https://github.com/Abso1ut3Zer0/nexus/issues/200) — the
  async client could silently drop unconsumed handshake bytes when
  the server sent a multi-record burst exceeding rustls's per-call
  read cap (long cert chains, large certs, OCSP stapling). The
  observed symptom was
  `Io(UnexpectedEof, "closed during TLS handshake")` after the
  server's timeout (~15-30s).

### Changed

- Dependency declaration: `nexus-async-rt` 0.4.0 → 0.5.0. The 0.5.0
  release was the production-hardening version of nexus-async-rt
  (see its own CHANGELOG for details). nexus-async-net's `nexus`
  backend benefits from those fixes transitively (TaskRef-based
  refcount discipline, `dispose_terminal` routing, intrusive
  cancellation list, `shutdown_quiesce` API).

### Internal

- New end-to-end regression test:
  `tests/ws_nexus_tls_loopback.rs::nexus_async_wss_echo_with_oversize_handshake_burst`
  drives a real wss:// connect through the async client against a
  localhost TLS+WS echo server, with a 10-cert ECDSA-P256 chain
  forcing the handshake burst past rustls's 4096-byte per-call
  deframer cap. Pre-fix this test reproduces birch's exact symptom
  (`Io(UnexpectedEof, "closed during TLS handshake")`); post-fix it
  passes. Hermetic, no network access required.

### Migration notes

`cargo update -p nexus-async-net` is the only change required.
Public API is unchanged.

## [0.6.0] — 2026-04-15

Earlier 0.6.x and prior versions are not documented in this
CHANGELOG. See git history and GitHub release notes for details.
