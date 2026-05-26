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
| [BNN](algorithms/bnn.md) | Binary neural net (±1 weights, XNOR+popcount) | 83-666 ns | GBDT-beating latency, memory-constrained, FPGA target |
| [QuantizedMlp](algorithms/mlp.md) | Int8-quantized MLP (i8 matmul, f32 activations) | 113-511 ns | Bandwidth-bound MLPs (large layers / L2 spill) |

### Stateful (streaming temporal)

| Type | What it is | Step cost | Use case |
|------|-----------|----------|----------|
| [LSTM](algorithms/lstm.md) | Long Short-Term Memory network | 105ns-1.3µs | Temporal patterns, long-range memory |
| [GRU](algorithms/gru.md) | Gated Recurrent Unit | 165ns-1.1µs | Temporal patterns, simpler/faster than LSTM |
| [Causal1dConv](algorithms/causal1d.md) | Streaming causal 1D convolution | 50ns-168ns | Short-range patterns, fixed receptive field |
| [TCN](algorithms/tcn.md) | Dilated causal conv stack | ~100-350 ns | Fixed-window medium/long range, exponential reach |
| [SSM](algorithms/ssm.md) | Linear state-space model (diagonal recurrence) | 42-131 ns | Long-range memory, fastest temporal, regime detection |

Multi-layer variants `StackedLstm` / `StackedGru` (PyTorch `num_layers=N`) are
documented in the [LSTM](algorithms/lstm.md) and [GRU](algorithms/gru.md) docs.

## Guides

- [Quickstart](guides/quickstart.md) — Load a model, make predictions, handle errors
- [Choosing a Model Type](guides/choosing.md) — Decision tree: which model for your use case
- [NaN Handling](guides/nan-handling.md) — Checked vs unchecked contracts per type
- [Exporting from Python](guides/python-export.md) — Get weights out of PyTorch/LightGBM into `from_parts()`

## Reference

- [Performance](reference/performance.md) — Benchmark results, complexity analysis

## Use Cases

- [Trading Systems](use-cases/trading.md) — Feature pipeline to inference to execution

## Crate Layout

```
src/
├── lib.rs              — Public API, Model/StatelessModel traits, re-exports
├── error.rs            — LoadError
├── activation.rs       — Activation enum
├── validate.rs         — construction-time validation helpers
├── gbdt.rs             — Gbdt (false-branch-next tree layout)
├── mlp.rs              — Mlp
├── quantized_mlp.rs    — QuantizedMlp (int8 matmul, f32 activations)
├── bnn.rs              — Bnn (XNOR+popcount binary layers)
├── lut.rs              — Lut
├── ssm.rs              — LinearSsm (diagonal linear state-space)
├── causal1d.rs         — Causal1dConv
├── tcn.rs              — TinyTcn (dilated causal conv stack)
├── lstm.rs             — TinyLstm (+ shared LSTM gate-weight fusion)
├── gru.rs              — TinyGru
├── stacked_lstm.rs     — StackedLstm
├── stacked_gru.rs      — StackedGru
├── kernel/             — numerical compute kernels (slices in/out, no model state)
│   ├── dot/            — SIMD f32 dot / matvec (scalar, avx2, avx512)
│   ├── activate.rs     — scalar + SIMD activations (Padé [7,6] sigmoid/tanh)
│   ├── gates/          — LSTM/GRU gate kernels (scalar, avx2, avx512)
│   ├── gemv.rs         — tiled GEMV + bias + activation (shared by MLP and conv)
│   ├── mlp.rs          — LayerNorm + fast rsqrt
│   ├── quantized.rs    — i8 GEMV (maddubs) + f32→i8 quantize
│   └── binary.rs       — binary input / hidden (XNOR+popcount) / output kernels
└── loader/
    ├── mod.rs          — loader dispatch
    ├── lightgbm.rs     — LightGBM text format parser
    └── safetensors.rs  — safetensors weight loader
```

## Feature Flags

| Flag | Default | Enables |
|------|---------|---------|
| `loader-lightgbm` | Yes | `Gbdt::from_lightgbm()` text-format parser |
| `safetensors` | Yes | safetensors weight loading for NN models (see [Exporting from Python](guides/python-export.md)) |
