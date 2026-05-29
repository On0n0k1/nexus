# nexus-fix — FIX Protocol Toolkit

## Overview

A toolkit for building FIX engines, not a monolithic engine.
Composable primitives that users assemble to match their
architecture — single-process, custom IPC, etc.

Sans-IO throughout: pure state machines, no sockets, no async, no
runtime dependency. Time is injected. Works with mio, tokio,
io_uring, kernel bypass, or a plain blocking thread.

### Crate structure

```
nexus-fix/           -- FIX codec autogen from XML dictionaries
  nexus-fix-macros/  -- proc macro crate (if needed)
nexus-fix-engine/    -- Sans-IO session state machine, framing
```

`nexus-fix` generates the flyweight codecs. `nexus-fix-engine` is
the session layer that uses them.

**Depends on:** `nexus-shm` (ShmJournal for message persistence).

---

## nexus-fix: Codec Generation

### Approach

QuickFIX XML data dictionaries -> Rust flyweight codecs at compile
time.

```rust
// build.rs
fn main() {
    nexus_fix::compile(&[
        ("coinbase", "dictionaries/FIX44_coinbase.xml"),
        ("deribit", "dictionaries/FIX44_deribit.xml"),
    ]).unwrap();
}
```

**Generated per venue:**
- Flyweight decoders: zero-copy, single-pass. Hot fields pre-indexed
  (O(1) access), cold fields via linear scan.
- Encoders: direct buffer write, builder pattern. Caller provides
  the buffer.
- Enum types: `repr(u8)`, exhaustive matching, per-dictionary (no
  cross-venue sharing — if users need unified types, they build their
  own abstraction).

### Design question: codegen vs proc macro

**Path A — build.rs codegen (prost model):**
Generate to `OUT_DIR`, include via `include!`. Standard pattern.
Don't commit generated code.

**Path B — proc macro:**
```rust
#[derive(FixCodec)]
#[fix(dictionary = "FIX44_coinbase.xml")]
struct CoinbaseFix;
```

Pros: better IDE support, no build.rs complexity.
Cons: proc macros reading XML at compile time is unusual and may
have tooling issues.

**Recommendation:** Path A is proven and matches the ecosystem
convention. Path B can be explored later if there's demand.

### Flyweight decoder (zero allocation)

The codegen knows the full message structure from the XML dictionary
at compile time — which tags appear, their order, which start
repeating groups, what fields each group contains. This means the
generated decoder is a typed struct with named fields, not a generic
offset array. Single parse pass populates the field spans. All
access after that is O(1): struct field read + buffer index.

```rust
// Generated from FIX44_coinbase.xml — one struct per message type.
// Every field the dictionary defines becomes a struct member.
pub struct NewOrderSingleDecoder<'buf> {
    buffer: &'buf [u8],
    // Fixed fields — populated during single parse pass
    cl_ord_id: FieldSpan,
    symbol: FieldSpan,
    side: FieldSpan,
    order_qty: FieldSpan,
    price: FieldSpan,
    time_in_force: FieldSpan,
    // Repeating group — byte offset where group starts + count
    no_allocs_offset: u16,
    no_allocs_count: u8,
}

impl<'buf> NewOrderSingleDecoder<'buf> {
    /// Single-pass decode. Walks the buffer once, records all field
    /// positions as FieldSpans. No allocation.
    pub fn decode(buffer: &'buf [u8]) -> Result<Self, DecodeError>;

    /// O(1) — struct field read + buffer slice.
    pub fn cl_ord_id(&self) -> &'buf [u8] {
        self.buffer[self.cl_ord_id.range()]
    }

    /// Lazy parse — only converts when accessed.
    pub fn side(&self) -> Result<Side, DecodeError> {
        Side::from_byte(self.buffer[self.side.offset])
    }

    /// Returns a typed iterator over the group's entries.
    pub fn allocs(&self) -> AllocGroupIter<'buf> {
        AllocGroupIter {
            buffer: self.buffer,
            pos: self.no_allocs_offset as usize,
            remaining: self.no_allocs_count,
        }
    }
}
```

The decoder IS the index. After the parse pass, the struct is a
complete map of the message — pure flyweight over the original
buffer. `FieldSpan` is a `(u16, u16)` pair (offset + length),
so the struct is compact and cache-friendly.

### Repeating groups (zero allocation)

FIX repeating groups can appear anywhere in the message body, not
just at the end (unlike SBE). A message can have fixed fields, then
a group, then more fixed fields, then another group. Any field
after a group requires the parser to have walked through the group
to know where it starts.

Most FIX engines fall back to HashMap for groups and give back the
latency earned everywhere else. The flyweight approach avoids this
entirely — the codegen knows the group structure from the XML
dictionary and generates typed iterators and entry decoders.

**How it works:**

During the single parse pass, the decoder records where each group
starts (byte offset) and how many entries it has (from the count
tag). It does NOT parse individual group entries — it just notes
the region.

Group access is lazy: the generated iterator walks the buffer
region on demand, yielding typed entry decoders. Each entry decoder
is just a `(start, end)` byte range into the original buffer.

```rust
// Generated — one iterator + entry decoder per group definition.
pub struct AllocGroupIter<'buf> {
    buffer: &'buf [u8],
    pos: usize,       // current position in buffer
    remaining: u8,    // entries left
}

impl<'buf> Iterator for AllocGroupIter<'buf> {
    type Item = AllocEntryDecoder<'buf>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 { return None; }
        // Walk from `pos` to find entry boundaries.
        // The delimiter tag (first field in group definition)
        // signals the start of each entry.
        let (start, end) = find_entry_bounds(
            self.buffer, self.pos, /* delimiter tag from codegen */
        );
        self.pos = end;
        self.remaining -= 1;
        Some(AllocEntryDecoder {
            buffer: self.buffer,
            start,
            end,
        })
    }
}

// Generated — one accessor per field in the group definition.
pub struct AllocEntryDecoder<'buf> {
    buffer: &'buf [u8],
    start: usize,
    end: usize,
}

impl<'buf> AllocEntryDecoder<'buf> {
    /// Scan within [start..end] for tag 79.
    /// Entry is small (typically 50-100 bytes), scan is trivial.
    pub fn alloc_account(&self) -> Option<&'buf [u8]> {
        find_tag(self.buffer, self.start, self.end, 79)
    }

    pub fn alloc_qty(&self) -> Option<&'buf [u8]> {
        find_tag(self.buffer, self.start, self.end, 80)
    }
}
```

**Why this works without allocation:**

- The message decoder struct stores group metadata as struct
  fields (`offset: u16`, `count: u8`) — no arrays, no heap.
- The iterator walks the buffer directly. Its state is three words
  (pointer, position, remaining). Stack-allocated.
- Each entry decoder is two words (start, end) plus the buffer
  reference. Stack-allocated.
- Field access within an entry is a small linear scan. Entries are
  typically 5-10 fields / 50-100 bytes — trivial.
- Nested groups (rare, mostly allocation/settlement messages off
  the hot path) follow the same pattern recursively. The codegen
  generates nested iterator types from the XML.

**What the codegen provides vs what's shared:**

The `FieldSpan` type, `find_tag` / `find_entry_bounds` scanning
functions, and SOH-delimited parsing utilities live in the shared
`nexus-fix` library crate. The codegen generates only the typed
structs (decoders, encoders, group iterators, entry decoders, enum
types) per dictionary. No buffer wrapper types regenerated per
schema.

**Practical note:** Most hot fields in trading messages (ClOrdID,
Symbol, Side, Price, OrderQty) appear before any repeating groups.
The groups are typically legs, parties, or allocation entries —
important for downstream processing but not on the tightest
order-entry hot path. The lazy group iteration means you pay for
groups only when you access them.

### Shared vs generated types

A key design concern with codegen is the boundary between shared
types and generated types. SBE's codegen, for example, generates
`ReadBuf`/`WriteBuf` wrappers per schema that really should be
shared infrastructure rather than regenerated each time. The codec
generation should keep the buffer access and field encoding
primitives in a shared library crate, generating only the
message-specific flyweight types per dictionary.

---

## nexus-fix-engine: Session Layer

### Sans-IO session state machine

Same pattern as nexus-net's WebSocket codec. Pure state machine
with poll-based event dispatch and caller-owned encode buffer.

```
handle_message(msg, now) -> pushes events to internal buffer
handle_timeout(now)      -> pushes timer events
poll_event()             -> drains events, caller processes
encode_pending(kind, buf)-> encodes into caller's buffer
```

**Key design:** Session never allocates. The caller provides a
"workhorse buffer" for encoding. Events are `Copy` enum variants
(no borrowed data).

### Session state machine

```
Disconnected -> LogonSent -> Active <-> Resending -> LogoutPending -> Disconnected
```

All admin messages (Logon, Logout, Heartbeat, TestRequest,
ResendRequest, SequenceReset, Reject) handled internally by the
state machine. Application messages emitted as events for user code.

### Persistence integration

Uses `nexus-shm`'s ShmJournal for durable message storage.

```
TCP read -> raw bytes
  -> journal.append(...)     <- mmap write, ~ns
  -> fix_codec.parse(bytes)  <- zero-copy from read buffer
  -> session.handle_message  <- state machine
  -> poll_event loop         <- dispatch

Resend request:
  -> journal.read_range(...) <- mmap read, zero-copy
  -> TCP write
```

### Trait-based persistence

Pluggable via traits so users can swap implementations:

```rust
pub trait MessageStore {
    type Error;
    fn store(&mut self, session_id: SessionId, direction: Direction,
             seq_num: u32, msg: &[u8]) -> Result<(), Self::Error>;
    fn retrieve(&self, session_id: SessionId, range: SeqRange)
             -> impl Iterator<Item = Result<StoredMessage<'_>, Self::Error>>;
}

pub trait SessionStore {
    type Error;
    fn load(&self, id: SessionId) -> Result<Option<SessionState>, Self::Error>;
    fn save(&mut self, id: SessionId, state: &SessionState) -> Result<(), Self::Error>;
}
```

Default implementations backed by ShmJournal. Users can implement
these traits for their own storage.

### Performance targets

| Operation | Target |
|-----------|--------|
| SOH scan + checksum | < 200ns |
| Session logic | < 100ns |
| Message store write | < 500ns |
| Full inbound path | < 1us |
| Outbound encode | < 300ns |

### Implementation order

1. **nexus-fix** — Codec generation (XML -> flyweight codecs)
2. **nexus-fix-engine** — Session state machine + framing
3. **Persistence integration** — Wire up ShmJournal via traits

nexus-shm is a prerequisite for step 3. Steps 1-2 can proceed
in parallel with shm work.

---

## FIX versions

Start with FIX 4.2 and 4.4 (dominant in crypto). Design the codegen
to be version-generic so 5.0/FIXT can be added later without
restructuring.

## References

- **SBE (Simple Binary Encoding)** — Flyweight codec generation
  over buffer segments. Good pattern for zero-copy field access.
  Design note: shared buffer primitives (ReadBuf/WriteBuf) should
  live in a common crate rather than being regenerated per schema.
- **prost** — build.rs codegen from schema files (protobuf). The
  `compile()` API pattern and `OUT_DIR` generation model.
- **Artio** — Adaptive's FIX engine. Engine/library separation.
  Session-layer design inspiration.
- **QuickFIX** — Canonical FIX engine. Dictionary XML format is
  the de facto standard for FIX message schemas.
- **nexus-net** — Existing sans-IO WebSocket implementation. Same
  architectural pattern for the session state machine.
