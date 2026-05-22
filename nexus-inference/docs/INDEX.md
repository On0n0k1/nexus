# nexus-inference

ML inference engine for pre-trained models. Low-latency prediction
on the hot path — no training, no Python, no allocation after setup.

Models are trained externally (LightGBM, PyTorch, scikit-learn, etc.),
loaded once at startup via `from_parts()`, and served immutably with
`&self` prediction methods.

## Model Types

### Stateless (single prediction)

| Type | What it is | Prediction cost | Use case |
|------|-----------|----------------|----------|
| [GBDT](algorithms/gbdt.md) | Gradient-boosted decision tree ensemble | ~5 cycles/node | Tabular features, risk signals |
| [MLP](algorithms/mlp.md) | Feedforward neural network | ~0.5 ns/FMA | Nonlinear combinations, embeddings |
| [LUT](algorithms/lut.md) | Discretized lookup table | ~5-8 ns total | Pre-computed surfaces, fast approximation |

### Stateful (streaming temporal)

| Type | What it is | Step cost | Use case |
|------|-----------|----------|----------|
| [LSTM](algorithms/lstm.md) | Long Short-Term Memory network | 105ns-1.3µs | Temporal patterns, long-range memory |
| [GRU](algorithms/gru.md) | Gated Recurrent Unit | 165ns-1.1µs | Temporal patterns, simpler/faster than LSTM |
| [Causal1dConv](algorithms/causal1d.md) | Streaming causal 1D convolution | 50ns-168ns | Short-range patterns, fixed receptive field |

## Guides

- [Quickstart](guides/quickstart.md) — Load a model, make predictions, handle errors
- [Choosing a Model Type](guides/choosing.md) — Decision tree: which model for your use case
- [NaN Handling](guides/nan-handling.md) — Checked vs unchecked contracts per type
- [no_std Support](guides/no-std.md) — Feature flags: `alloc`, `std`, `libm`
- [Exporting from Python](guides/python-export.md) — Get weights out of PyTorch/LightGBM into `from_parts()`

## Reference

- [Performance](reference/performance.md) — Benchmark results, complexity analysis

## Use Cases

- [Trading Systems](use-cases/trading.md) — Feature pipeline to inference to execution

## Crate Layout

```
src/
├── lib.rs              — Public API, re-exports
├── error.rs            — LoadError
├── gbdt.rs             — GbdtF64/F32, Node, RawNode, reorder_and_compact
├── mlp.rs              — MlpF64/F32, Activation
├── lut.rs              — LutF64/F32, checked_pow
├── dot/
│   └── mod.rs          — SIMD dot products, matvec_bias_f32, matvec_f32
├── rnn/
│   ├── mod.rs          — sigmoid_f32, tanh_f32 (Padé approximants)
│   ├── lstm.rs         — TinyLstmF32
│   ├── gru.rs          — TinyGruF32
│   └── avx2_gates.rs   — AVX2 vectorized gate activations
├── conv/
│   ├── mod.rs          — Module declaration
│   └── causal1d.rs     — Causal1dConvF32
└── loader/
    └── lightgbm.rs     — LightGBM text format parser
```

## Feature Flags

| Flag | Default | Enables |
|------|---------|---------|
| `std` | Yes | Standard library (implies `alloc`) |
| `alloc` | Via `std` | GBDT, MLP, LUT, Conv types (`Box`, `Vec`) |
| `libm` | No | LSTM/GRU in no_std, `Tanh`/`Sigmoid` activations (implies `alloc`) |
| `loader-lightgbm` | No | `GbdtF64::from_lightgbm()` parser (implies `alloc`) |
