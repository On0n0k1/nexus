# nexus-fix-codec — Flyweight Parser + Code Generation

Zero-copy FIX message parsing and compile-time code generation
from FIXML dictionaries.

## Crate Structure

```
nexus-fix-codec/       — Core library: flyweight runtime, scanning
                         primitives, FieldSpan, checksum, SIMD utils,
                         encoder primitives, error types. Ships as a
                         normal crate dependency.

nexus-fix-codegen/     — Standalone binary + library: reads
                         FIXML/QuickFIX XML dictionaries, writes
                         readable .rs files to a user-specified output
                         directory. Can be invoked as a CLI tool
                         (like protoc / SBE) or called as a library
                         from a build.rs.
```

### Codegen invocation

**CLI (recommended):**

```bash
# Generate codecs for a venue's dictionary
cargo run -p nexus-fix-codegen -- \
    --dict dictionaries/FIX44_coinbase.xml \
    --out src/generated/coinbase/

# Regenerate after dictionary update
cargo run -p nexus-fix-codegen -- \
    --dict dictionaries/FIX44_coinbase_v2.xml \
    --out src/generated/coinbase/

# git diff shows exactly what changed
```

Dictionary changes are deployment events, not runtime events. A
counterparty publishes a new spec weeks in advance. You test
against their cert environment and deploy. The old schema keeps
working during the transition. There is no hot-reload use case
that justifies runtime dictionary interpretation.

**build.rs (alternative):**

```rust
// build.rs
nexus_fix_codegen::generate(&["dictionaries/FIX44_coinbase.xml"])
    .out_dir(&std::env::var("OUT_DIR").unwrap())
    .run();
```

```rust
// src/generated.rs
include!(concat!(env!("OUT_DIR"), "/coinbase/mod.rs"));
```

This follows the prost/tonic pattern — zero friction, `cargo build`
just works. Tradeoff: generated code lives in `target/` and is not
directly navigable in IDE or reviewable in PR diffs. The CLI
approach is preferred for FIX codecs because generated output is
substantial (hundreds of message types, thousands of fields) and
benefits from being readable, greppable, and diffable.

The codegen crate exposes both:

```
nexus-fix-codegen/
  src/
    lib.rs    — generation logic (pub API)
    main.rs   — CLI wrapper (clap, calls lib)
```

---

## nexus-fix-codec: Core Library

### FieldSpan

The fundamental unit. A `(offset: u32, len: u32)` pair pointing
into the original message buffer. 8 bytes. All field access goes
through this — the accessor reads `buffer[span.offset..][..span.len]`.

`u32` length accommodates DATA-type fields (RawData, XmlData,
EncodedText) which can exceed 64KB. Normal fields are tiny (1-50
bytes), but the uniform type keeps the generated code simple —
no separate span type for DATA fields.

```rust
#[derive(Copy, Clone)]
pub struct FieldSpan {
    pub offset: u32,
    pub len: u32,
}

impl FieldSpan {
    pub const EMPTY: Self = Self { offset: 0, len: 0 };

    pub fn is_present(&self) -> bool {
        self.len > 0
    }

    pub fn slice<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.offset as usize..][..self.len as usize]
    }
}
```

### SOH Scanning (SIMD)

FIX messages are delimited by SOH (`\x01`). Every operation starts
with finding the next SOH. This is the innermost loop of the entire
parser and must be SIMD-accelerated.

**SSE2 (baseline x86-64):** `_mm_cmpeq_epi8` + `_mm_movemask_epi8`
to find SOH in 16-byte chunks. Same technique as memchr. Falls back
to scalar for the tail.

**AVX2:** 32 bytes per iteration. Same pattern, wider registers.

```rust
/// Find the next SOH byte starting from `pos`.
/// Returns the offset relative to `buf.as_ptr()`.
pub fn find_soh(buf: &[u8], pos: usize) -> Option<usize>;

/// Find the next '=' byte starting from `pos`.
/// Used for tag=value separation.
pub fn find_eq(buf: &[u8], pos: usize) -> Option<usize>;
```

These are the only two delimiter scan functions needed. Everything
else composes on top.

### DATA Fields (Length-Delimited)

FIX DATA-type fields (tag pairs 95/96 RawData, 90/91 SecureData,
212/213 XmlData, 348/349 EncodedText, 358/359 EncodedListStatusText)
can legally contain embedded SOH (`\x01`) and `=` bytes in the
value. A pure SOH scan would split the value at the first embedded
delimiter and desync the rest of the message.

The dictionary knows which fields are DATA type — each is preceded
by a LENGTH field that gives the byte count. The generated scanner
handles this: when the watermark scan encounters a LENGTH tag, it
parses the integer value, then consumes the next field as exactly
N raw bytes instead of scanning for SOH. This is a correctness
path, not an optimization.

```
95=5\x01 96=AB\x01CD\x01
         ^^^^^^^^^^
         5 raw bytes — the embedded \x01 is data, not a delimiter
```

The codegen emits the LENGTH/DATA association from the dictionary.
The core `find_soh` scanner is never called within a DATA field.

### Checksum (SIMD)

FIX checksum is the sum of all bytes (excluding the checksum field
itself) mod 256. This is a byte accumulation — SIMD `PSADBW`
(packed sum of absolute differences) against a zero vector gives
horizontal byte sums in 8-byte chunks. Accumulate the partial sums
and take mod 256 at the end.

```rust
/// Compute FIX checksum over `buf[start..end]`.
/// Uses SIMD PSADBW when available.
pub fn checksum(buf: &[u8], start: usize, end: usize) -> u8;

/// Validate checksum of a complete FIX message.
/// Finds tag 10, computes expected checksum, compares.
pub fn validate_checksum(msg: &[u8]) -> Result<(), ChecksumError>;
```

Note: FIX checksum is NOT CRC32 — there is no dedicated hardware
instruction. PSADBW is the right SIMD approach for byte summation.

### Tag Number Parsing

Tag numbers are 1-5 digit ASCII integers. Parsing them is on the
hot path (every field access starts with reading the tag number).
Options:

- **Scalar:** Simple multiply-accumulate loop. 4-8 cycles for
  typical 2-3 digit tags.
- **SWAR:** Parallel digit extraction for known-length tags.
  Useful if we can predict tag length (generated code knows this).
- **Lookup table:** For the most common tags (35, 49, 56, etc.),
  the generated code can match on the first byte to skip parsing
  entirely.

```rust
/// Parse ASCII tag number. Returns (tag, bytes_consumed).
pub fn parse_tag(buf: &[u8]) -> (u32, usize);
```

### Progressive Scan (Watermark)

The flyweight does not build a complete index upfront. Instead it
scans forward on demand and caches every field it passes. A
watermark (`scanned_to`) tracks how far into the buffer the
scanner has progressed.

**Three cases on field access:**

1. **Offset already cached** → direct return, zero scanning.
2. **Scanned to end-of-message and field not cached** → field
   not present in message.
3. **Watermark hasn't reached end-of-message** → scan forward
   from watermark, caching every tag encountered, until the
   target tag is found or end-of-message.

FIX only guarantees field order within the header and within
repeating groups — body field order is not significant on the
wire. A field cannot be declared absent until the entire message
has been scanned. Most counterparties send canonical order, so
accessing fields in order does short forward scans in practice,
but the correctness invariant is: "not present" means "scanned
to end and not found."

This means:
- Accessing the first field is a short scan of the header.
- Accessing fields in message order (the common case) does
  short forward scans, each continuing where the last left off.
- Accessing the last field worst-case scans the whole message
  once. But that scan also caches every other field, so
  subsequent accesses are free.
- If you only access 3 fields out of 40, you only scan the
  bytes up to and including the last field you need.
- If a field is truly absent, one full scan confirms it and
  caches everything else — all future accesses are free.

```
Buffer:
┌─────────────────────────────────────────────────┐
│ 8=FIX.4.4│35=D│49=SENDER│56=TARGET│44=123.45│..│
└─────────────────────────────────────────────────┘
                              ▲
                              scanned_to

Access price (tag 44):
  → scan from scanned_to, pass tag 56 (cache it), find tag 44
  → cache tag 44, advance scanned_to past it
  → return &buf[offset..][..len] for tag 44
```

### Interior Mutability (Cell)

The watermark and cached field offsets are internal scanner state,
not user-visible mutation. Using `Cell<FieldSpan>` and `Cell<u32>`
lets accessors take `&self` instead of `&mut self`. This is
critical for ergonomic use — with `&mut self`, the borrow checker
prevents holding a returned `&'buf str` while calling another
accessor:

```rust
// With &mut self — does NOT compile:
let symbol = decoder.symbol();   // borrows &mut decoder
let side = decoder.side();       // ERROR: already borrowed

// With &self + Cell — compiles fine:
let symbol = decoder.symbol();   // borrows &decoder
let side = decoder.side();       // another & borrow, OK
let allocs = decoder.allocs();   // iterator also borrows, OK
```

`Cell` is the right tool: zero-cost (no atomic, no runtime check),
`Copy` types only (`FieldSpan` and `u32` are `Copy`),
single-threaded (FIX decoders are not shared across threads). The
scanner state is an implementation detail — `Cell` hides it behind
a clean `&self` API.

### Writer Primitives

Encoding is simpler than decoding — the caller knows what fields
they're writing. The core library provides field-level primitives:

```rust
/// Write a tag=value\x01 field into `buf` at `pos`.
/// Returns new position after the field.
pub fn encode_field(buf: &mut [u8], pos: usize, tag: u32,
                    value: &[u8]) -> usize;

/// Format checksum as 3 zero-padded ASCII digits for tag 10.
pub fn format_checksum(sum: u8) -> [u8; 3];

/// Compute FIX checksum: byte sum mod 256.
pub fn checksum(data: &[u8]) -> u8;
```

`FieldWriter` wraps `&mut [u8]` with a position cursor, symmetric
with `FieldReader` on the read side.

**Framing (tags 8, 9, 10) is NOT in the codec.** Message framing
lives in the generated encoder layer, which has dictionary knowledge
of which tags are header/trailer. The codec provides the building
blocks; codegen orchestrates them:

```rust
// Generated encoder usage with WriteBuf (nexus-net):
//
// 1. Write body fields into wb.spare() via FieldWriter
// 2. Compute checksum over body bytes
// 3. wb.prepend("9=<body_len>\x01")
// 4. wb.prepend("8=<begin_string>\x01")
// 5. wb.append("10=<checksum>\x01")  via encode_field + format_checksum
//
// Body is written in-place (zero-copy). Header/trailer are
// prepended/appended — ~25 bytes of copies, negligible.
```

Generated encoders use these to build typed builder APIs per
message type.

---

## nexus-fix-codegen: Code Generator

### Input

QuickFIX XML data dictionaries. This is the de facto standard
format — every exchange that supports FIX publishes one, and
custom venues extend it with their own fields/messages.

The generator parses:
- `<fields>` — tag number, name, type, enum values
- `<messages>` — message type (tag 35 value), required/optional
  fields, component refs
- `<components>` — reusable field groups (e.g., Instrument,
  Parties)
- `<groups>` — repeating group definitions with delimiter tag

### Output (per dictionary)

The codegen writes readable `.rs` files to the output directory:

**`fields.rs`** — Tag number constants and typed enum types.

```rust
// Generated from FIX44_coinbase.xml

pub const TAG_CL_ORD_ID: u32 = 11;
pub const TAG_SYMBOL: u32 = 55;
pub const TAG_SIDE: u32 = 54;
pub const TAG_PRICE: u32 = 44;
pub const TAG_ORDER_QTY: u32 = 38;
// ...

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Side {
    Buy = b'1',
    Sell = b'2',
    // ...
}

impl Side {
    pub fn from_byte(b: u8) -> Option<Self> { /* ... */ }
    pub fn as_byte(self) -> u8 { self as u8 }
}
```

**`messages.rs`** — Per-message-type flyweight decoder.

```rust
use std::cell::Cell;

// Generated — one decoder per message type.
pub struct NewOrderSingleDecoder<'buf> {
    buf: &'buf [u8],
    scanned_to: Cell<u32>,
    // Slot per field defined in dictionary for this message.
    // EMPTY until the field is encountered during scan.
    cl_ord_id: Cell<FieldSpan>,
    symbol: Cell<FieldSpan>,
    side: Cell<FieldSpan>,
    order_qty: Cell<FieldSpan>,
    price: Cell<FieldSpan>,
    time_in_force: Cell<FieldSpan>,
    // Groups store offset + count. Entries parsed lazily.
    no_allocs: Cell<GroupSpan>,
}

impl<'buf> NewOrderSingleDecoder<'buf> {
    /// Wrap a raw FIX message body (after header validation).
    pub fn wrap(buf: &'buf [u8]) -> Self { /* ... */ }

    /// Zero-copy field access. Scans forward if needed.
    pub fn cl_ord_id(&self) -> Option<&'buf [u8]> { /* ... */ }
    pub fn symbol(&self) -> Option<&'buf str> { /* ... */ }
    pub fn side(&self) -> Option<Side> { /* ... */ }
    pub fn price(&self) -> Option<&'buf [u8]> { /* ... */ }

    /// Repeating group — returns typed iterator.
    pub fn allocs(&self) -> AllocGroupIter<'buf> { /* ... */ }
}
```

**`groups.rs`** — Repeating group iterators and entry decoders.

```rust
pub struct AllocGroupIter<'buf> {
    buf: &'buf [u8],
    pos: usize,
    remaining: u16,
}

impl<'buf> Iterator for AllocGroupIter<'buf> {
    type Item = AllocEntry<'buf>;
    // ...
}

pub struct AllocEntry<'buf> {
    buf: &'buf [u8],
    start: usize,
    end: usize,
}

impl<'buf> AllocEntry<'buf> {
    pub fn alloc_account(&self) -> Option<&'buf [u8]> {
        nexus_fix_codec::find_tag(self.buf, self.start, self.end, 79)
    }
    pub fn alloc_qty(&self) -> Option<&'buf [u8]> {
        nexus_fix_codec::find_tag(self.buf, self.start, self.end, 80)
    }
}
```

Group entries are small (5-10 fields, 50-100 bytes). Linear
scan within an entry is trivial — no indexing needed.

**`encoders.rs`** — Builder-pattern encoders per message type.

```rust
pub struct NewOrderSingleEncoder<'buf> {
    buf: &'buf mut [u8],
    pos: usize,
}

impl<'buf> NewOrderSingleEncoder<'buf> {
    pub fn wrap(buf: &'buf mut [u8]) -> Self { /* ... */ }
    pub fn cl_ord_id(mut self, val: &[u8]) -> Self { /* ... */ }
    pub fn symbol(mut self, val: &str) -> Self { /* ... */ }
    pub fn side(mut self, val: Side) -> Self { /* ... */ }
    pub fn price(mut self, val: &[u8]) -> Self { /* ... */ }
    pub fn finish(self) -> usize { /* returns bytes written */ }
}
```

### Repeating Groups

FIX repeating groups are positional structure in a flat byte
stream. A group starts with a count tag (e.g., tag 268 =
NoMDEntries), followed by N entries. Each entry begins with the
group's delimiter tag (the first field defined in the group).
The group ends when a tag outside the group definition appears.

**The dictionary is required to parse groups.** Without it, you
can't know which tag starts a group, which tags belong to it, or
where it ends. This is why compile-time codegen is the right
approach — the generated scanner has const knowledge of group
boundaries.

During the watermark scan, when the scanner encounters a count
tag, it records a `GroupSpan { offset: u32, count: u16 }` and
must walk through the group entries to find where the group ends
(so it can continue scanning fields after the group). This is
the one place where scanning is unavoidable — you can't skip
a group without knowing its extent.

Nested groups (rare — mostly allocation/settlement messages off
the hot path) follow the same pattern recursively. The codegen
generates nested iterator types from the XML structure.

### What's Shared vs Generated

**nexus-fix-codec (shared library):**
- `FieldSpan`, `GroupSpan` types
- `find_soh`, `find_eq` SIMD scanners
- `checksum` SIMD computation
- `parse_tag` number parser
- `encode_field`, `frame_message` encoding primitives
- `find_tag` linear scan within a range
- `DecodeError`, `ChecksumError` error types

**nexus-fix-codegen (generated per dictionary):**
- `fields.rs` — tag constants, enum types
- `messages.rs` — per-message flyweight decoders
- `groups.rs` — group iterators and entry decoders
- `encoders.rs` — per-message builder encoders
- `mod.rs` — re-exports, MsgType dispatch

The generated code depends on `nexus-fix-codec` for primitives.
No buffer wrapper types or scan functions are regenerated per
schema.

---

## Performance Targets

| Operation | Target |
|-----------|--------|
| SOH scan + checksum (SIMD) | < 100ns |
| Field access (cached) | < 10ns |
| Field access (watermark scan) | < 50ns |
| Outbound encode | < 300ns |

---

## Design Decisions

**Tag dispatch in the watermark scanner:** The generated scan loop
uses a `match` on the tag number to route each discovered field
into its `Cell<FieldSpan>` slot. The compiler picks the optimal
dispatch strategy (jump table for dense ranges, binary search for
sparse). No perfect hash or direct-indexed array needed.

```rust
// Generated per message type — inside the scan loop:
match tag {
    11 => self.cl_ord_id.set(span),
    44 => self.price.set(span),
    54 => self.side.set(span),
    55 => self.symbol.set(span),
    _  => { /* skip: not in dictionary */ }
}
```

**Encoder validation:** None. The encoder wraps a buffer and
writes fields — same model as SBE. Required-field validation is
a business logic concern that belongs in the session engine or
application layer, not in the byte-level codec.

**Unknown tags:** Silent skip. When the scanner hits a tag not
in the dictionary, it advances the watermark past it and
continues. Exchanges routinely add undocumented tags — erroring
on a valid message because of an extra tag is a production outage
waiting to happen.

---

## References

- **SBE (Simple Binary Encoding)** — Flyweight codec generation
  over buffer segments. Good pattern for zero-copy field access.
  Design note: shared buffer primitives should live in a common
  crate rather than being regenerated per schema.
- **Artio** — Adaptive's FIX engine. Dictionary-driven, flyweight
  decoders. Validates the approach. Their runtime dictionary
  interpretation trades performance for hot-reload — we chose
  compile-time codegen instead.
- **QuickFIX** — Canonical FIX engine. Dictionary XML format is
  the de facto standard. Slow (HashMap-heavy) but the schema
  format is right.
