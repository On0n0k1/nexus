# nexus-fix-codegen

Dictionary-driven code generator for [`nexus-fix-codec`](../nexus-fix-codec).
Reads a QuickFIX XML dictionary and emits zero-copy, zero-alloc Rust decoders
and encoders that sit directly on the codec primitives.

## Output

Per dictionary, five files:

- `fields.rs` — `TAG_*` constants and typed enums. Constructors are infallible
  (`from_byte`/`from_bytes` return `Self`); values outside the dictionary become
  `Unknown(u8)` / `Unknown(&AsciiTextStr)`.
- `messages.rs` — per-`MsgType` flyweight decoders. A single forward pass over
  `FieldReader` dispatches each tag (via `match`) into a `FieldSpan` slot.
  Accessors are typed by FIX type: `&AsciiTextStr` for string-like fields,
  `i64`/`u32` for integers, `bool` for booleans, `&[u8]` for DATA. DATA is read
  length-delimited so an embedded `0x01` never mis-splits. `is_complete()`
  checks required fields are present.
- `groups.rs` — repeating-group iterators and per-entry decoders, recursive.
- `encoders.rs` — consume-self builders over `FieldWriter`. Repeating groups are
  not yet supported on the encode side.
- `mod.rs` — re-exports, `BEGIN_STRING`, and `MsgType` dispatch.

## CLI

The CLI is behind the `cli` feature (so `build.rs` users don't compile `clap`):

```bash
cargo run -p nexus-fix-codegen --features cli -- --dict dict/FIX44.xml --out src/generated/
```

## build.rs

```rust
nexus_fix_codegen::generate()
    .dictionary("dict/FIX44.xml")
    .out_dir(std::env::var("OUT_DIR").unwrap())
    .run()
    .unwrap();
```

```rust
pub mod fix {
    include!(concat!(env!("OUT_DIR"), "/mod.rs"));
}
```

DATA fields inside repeating groups are rejected at generation time
(`EmitError::DataInGroup`).
