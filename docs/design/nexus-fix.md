# nexus-fix — FIX Protocol Toolkit

## Overview

A toolkit for building FIX engines, not a monolithic engine.
Composable primitives that users assemble to match their
architecture — single-process, custom IPC, etc.

Sans-IO throughout: pure state machines, no sockets, no async, no
runtime dependency. Time is injected. Works with mio, tokio,
io_uring, kernel bypass, or a plain blocking thread.

**Design principles:**

- **Dictionary-driven, not version-driven.** There is no
  "FIX 4.2 mode" or "FIX 4.4 mode." A counterparty's FIXML
  dictionary defines the schema.
- **Compile-time codegen, not runtime interpretation.** The
  dictionary is parsed once by an explicit tool. All message
  structures, field layouts, group boundaries, and enum types
  are generated as readable Rust source.
- **Zero-copy flyweight with progressive scan.** The decoded
  message IS the buffer. No intermediate data structures, no
  allocation.
- **SIMD-accelerated primitives.** SOH delimiter scanning,
  checksum computation, and tag number parsing use SIMD where
  available.

---

## Crate Structure

```
nexus-fix-codec/       — Core library: flyweight runtime, scanning
                         primitives, FieldSpan, checksum, SIMD utils,
                         encoder primitives, error types.

nexus-fix-codegen/     — Standalone binary + library: reads
                         FIXML/QuickFIX XML dictionaries, writes
                         readable .rs files.

nexus-fix-engine/      — Sans-IO session state machine, framing,
                         sequence management, persistence traits.
```

## Detailed Design

- **Codec + codegen:** [nexus-fix-codec.md](nexus-fix-codec.md) —
  flyweight parsing, progressive watermark scan, SIMD primitives,
  generated decoder/encoder structure, repeating groups.
- **Session engine:** [nexus-fix-engine.md](nexus-fix-engine.md) —
  sans-IO session state machine, TCP framing, persistence traits,
  ShmJournal integration.

## Implementation Order

1. **nexus-fix-codec** — Core scanning primitives, SIMD, FieldSpan
2. **nexus-fix-codegen** — XML parser, code generator
3. **nexus-fix-engine** — Session state machine + framing
4. **Persistence integration** — Wire up ShmJournal via traits

nexus-shm is a prerequisite for step 4. Steps 1-3 can proceed
in parallel with shm work.

---

## References

- **SBE (Simple Binary Encoding)** — Flyweight codec generation
  over buffer segments. Zero-copy field access pattern.
- **Artio** — Adaptive's FIX engine. Dictionary-driven, flyweight
  decoders, session/engine separation.
- **QuickFIX** — Canonical FIX engine. Dictionary XML format is
  the de facto standard.
- **nexus-net** — Existing sans-IO WebSocket implementation. Same
  architectural pattern for the session state machine.
