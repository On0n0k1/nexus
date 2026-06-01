# nexus-fix-codec

Zero-copy FIX protocol primitives with SIMD acceleration.

## Overview

Core building blocks for reading and writing FIX `tag=value\x01` fields.
Generated FIX codecs (from `nexus-fix-codegen`) depend on these primitives.
No allocation, no dependencies beyond `std`.

## Modules

### `scan` — SIMD Delimiter Scanning

Find SOH (`\x01`) and `=` delimiters with automatic dispatch to the
best available instruction set at compile time:

| Tier | Width | Availability |
|------|-------|-------------|
| AVX-512 | 64 bytes/iter | `target_feature = "avx512bw"` |
| AVX2 | 32 bytes/iter | `target_feature = "avx2"` |
| SSE2 | 16 bytes/iter | Baseline x86_64 |
| SWAR | 8 bytes/iter | All platforms |
| Scalar | 1 byte/iter | Tail bytes |

Two API styles:

- `find_soh(buf, pos)` / `find_eq(buf, pos)` — single-result lookup
- `soh_iter(buf, pos)` / `eq_iter(buf, pos)` — iterator with SIMD mask
  caching (multiple matches per chunk drained without re-scanning)

### `reader` — FIX Field Reader

`FieldReader` iterates over `tag=value\x01` fields, yielding `RawField`
(tag number + value `FieldSpan`). Fuses SOH scanning with PSADBW
checksum accumulation in a single pass — one SIMD load produces both
the delimiter mask and the byte sum.

```rust
use nexus_fix_codec::reader::FieldReader;

let msg = b"8=FIX.4.4\x0135=D\x0149=SENDER\x01";
let mut reader = FieldReader::new(msg, 0);

while let Some(field) = reader.next_field() {
    println!("tag={} value={:?}", field.tag, field.value.slice(msg));
}

let checksum = reader.checksum(); // byte sum mod 256, excludes tag 10
```

Standalone helpers: `parse_tag`, `find_tag`, `checksum`, `validate_checksum`.

### `writer` — FIX Field Writer

`FieldWriter` writes `tag=value\x01` fields into a `&mut [u8]` buffer.
Symmetric with `FieldReader` on the read side.

```rust
use nexus_fix_codec::writer::FieldWriter;

let mut buf = [0u8; 128];
let mut w = FieldWriter::wrap(&mut buf);
w.field(35, b"D");
w.field(49, b"SENDER");
w.field(55, b"BTC-USD");
assert_eq!(w.data(), b"35=D\x0149=SENDER\x0155=BTC-USD\x01");
```

Standalone helpers: `encode_field`, `format_checksum`.

### `span` — Zero-Copy Field References

- `FieldSpan { offset: u32, len: u32 }` — 8-byte pointer into the
  message buffer. `u32` length accommodates DATA-type fields that can
  exceed 64KB.
- `GroupSpan { offset: u32, count: u16 }` — repeating group location
  and entry count.

### `error` — Error Types

`DecodeError` and `ChecksumError`. Manual `Display` + `Error` impls
(no `thiserror` — workspace convention).

## Framing

Message framing (tags 8, 9, 10) is intentionally **not** in this crate.
The codec provides field-level read/write primitives. Framing logic —
which requires knowledge of FIX message structure — lives in the
generated encoder layer (`nexus-fix-codegen`).

The generated encoder composes these primitives with `WriteBuf`
(from `nexus-net`) for zero-copy outbound encoding:

1. Write body fields into `WriteBuf::spare()` via `FieldWriter`
2. Compute checksum over body bytes
3. Prepend `9=<body_length>\x01` and `8=<begin_string>\x01` via `WriteBuf::prepend`
4. Append `10=<checksum>\x01` via `encode_field` + `format_checksum`

Body is written in-place. Header and trailer are ~25 bytes of copies.

## Performance

Benchmarked on Intel Core Ultra 7 155H, SSE2, pinned with `taskset -c 0`:

| Operation | p50 | Notes |
|-----------|-----|-------|
| `find_soh` (128B, target at end) | ~20 cycles | Full scan worst case |
| `soh_iter` (15-field NewOrderSingle) | ~28 cycles | All SOH positions |
| `FieldReader` (15-field NewOrderSingle) | ~188 cycles | Fused scan + tag parse + checksum |

Run benchmarks:

```bash
cargo build --release --example perf_scan -p nexus-fix-codec
taskset -c 0 ./target/release/examples/perf_scan
```

For AVX2:

```bash
RUSTFLAGS="-C target-feature=+avx2" cargo build --release --example perf_scan -p nexus-fix-codec
taskset -c 0 ./target/release/examples/perf_scan
```

## License

MIT OR Apache-2.0
