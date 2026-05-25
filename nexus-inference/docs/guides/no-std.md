# no_std Support

nexus-inference supports `no_std` environments through feature flags.

## Feature Flag Hierarchy

```
  std (default)
   └── alloc
        ├── libm (optional)
        └── loader-lightgbm (optional)
```

| Flag | What it enables |
|------|----------------|
| `std` | Standard library, implies `alloc` |
| `alloc` | All model types (GBDT, MLP, LUT) — requires `Box`, `Vec` |
| `libm` | `Tanh`/`Sigmoid` activations without `std` (uses `libm` crate) |
| `loader-lightgbm` | `Gbdt::from_lightgbm()` text parser |

## Minimum: `alloc` only

```toml
[dependencies]
nexus-inference = { version = "0.1", default-features = false, features = ["alloc"] }
```

This gives you all three model types with `Relu`, `LeakyRelu`, and
`Identity` activations. `Tanh`, `Sigmoid`, `Elu`, `Gelu`, and `Swish`
are rejected at construction time (`from_parts` returns
`LoadError::Validation`).

## With transcendental activations

```toml
[dependencies]
nexus-inference = { version = "0.1", default-features = false, features = ["libm"] }
```

The `libm` feature implies `alloc` and adds the `libm` crate for
`tanh()` and `exp()` implementations. Same API, same results — just
a different math backend.

## Without `alloc`

```toml
[dependencies]
nexus-inference = { version = "0.1", default-features = false }
```

This compiles but provides no model types — only `LoadError`. Useful
if you need the error type in a shared crate that's `no_std` without
an allocator.

## What uses `alloc`

| Component | Allocation |
|-----------|-----------|
| Model structs | `Box<[T]>` for weights, biases, nodes, tables |
| `from_parts()` | Copies input slices into owned storage |
| `from_lightgbm()` | Parses text, builds node vectors |
| MLP scratch buffers | Pre-allocated in struct at construction |
| `LoadError` | No allocation (stack type) |

No per-prediction allocation. MLP scratch buffers are pre-allocated
in the struct. GBDT and LUT allocate nothing after construction.
