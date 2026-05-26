# BNN — Binary Neural Network

**Feedforward network whose hidden layers use ±1 weights.** The
hidden-to-hidden matrix multiply becomes XNOR + popcount instead of
multiply-add, so the inner layers are integer-only and ~32x smaller in
memory. The input and output layers stay fp32 for precision where it
matters.

| Property | Value |
|----------|-------|
| Prediction cost | 83 ns (0 binary layers) to 666 ns (8→128, 2 layers) |
| Memory | 8 B per 64 binary weights (32x less than fp32) + fp32 in/out layers |
| Type | `Bnn` (stateless) |
| Construction | `from_parts()` (fp32 in/out weights + pre-packed binary hidden weights) |
| Output | Single scalar, or multi-output via `predict_into` |

## What It Does

```
  input (fp32)
       │
       ▼
  ┌──────────────┐   W_input @ x + b   →  sign(·)   binarize to ±1
  │ input layer  │   fp32 projection, then threshold to bits
  └──────┬───────┘
         ▼  packed bits (u64 words)
  ┌──────────────┐
  │ binary layer │   XNOR + popcount  (replaces multiply-add)
  │   × N        │   compare to integer threshold → next bits
  └──────┬───────┘
         ▼  bits
  ┌──────────────┐   weighted sum read directly from bits
  │ output layer │   fp32, no unpacking
  └──────┬───────┘
         ▼
      score (fp32)
```

The fp32 **input layer** projects features to the hidden width and
binarizes (`sign(W @ x + b)`), folding any batch-norm into the bias. Each
**binary layer** holds ±1 weights packed into `u64` words; a neuron's
pre-activation is `XNOR(weights, inputs)` then `popcount` — the number of
agreeing bits — compared against an integer threshold derived from the
bias (`ceil((H − bias) / 2)`). The fp32 **output layer** reads a weighted
sum straight from the bits without unpacking.

## Why XNOR + popcount

With weights and activations both in {−1, +1}, a dot product is just
"how many positions agree minus how many disagree." Encode +1 as bit 1
and −1 as bit 0, and agreement is `XNOR`; summing agreements is `popcount`.
A 64-wide multiply-accumulate collapses to one XNOR and one popcount per
64-bit word — no multiplier, no FPU. The binary core is a tight `count_ones`
loop the compiler can vectorize; the fp32 input projection and output readout
use the crate's hand-written AVX2 kernels.

`hidden_size` must be a multiple of 64 (one bit per neuron, packed into
`u64` words). Binary weights use **32x less memory** than fp32: for H=64,
512 bytes vs 16 KB per layer.

## When to Use It

**Use BNN when:**
- You need MLP-style nonlinearity at **GBDT-beating latency** (16-37%
  faster than a comparable [GBDT](gbdt.md) at 1-2 binary layers).
- The model is **memory-constrained** — binary layers are 32x smaller, so
  the working set stays in L1.
- You're targeting (or prototyping for) **FPGA / fixed-point fabric** —
  XNOR+popcount is the most hardware-native neural primitive.
- You have a **binarized network trained in Python** (binary-aware
  training, e.g. straight-through estimator).

**Don't use BNN when:**
- You need full fp32 precision in the hidden layers (use [MLP](mlp.md) or
  [QuantizedMlp](mlp.md)).
- Inputs are tabular with missing values (use [GBDT](gbdt.md) — NaN routing).
- The model is tiny anyway — a small [MLP](mlp.md) is simpler and the
  binarization buys little.
- You don't have a binary-trained model — naively binarizing fp32 weights
  destroys accuracy; BNNs require binary-aware training.

## Output Interpretation

`predict()` returns the **raw output-layer score** (fp32) — not a
probability. For binary classification, apply a sigmoid. The binary
layers are internal; only the output is a real number.

## Code Example

```rust
use nexus_inference::Bnn;

// 2 inputs → 64 hidden → 1 output, with no extra binary hidden layers
// (just the fp32 input projection + fp32 output readout).
let h = 64;
let w_input  = vec![0.1_f32; h * 2];   // [H, I] row-major
let b_input  = vec![0.0_f32; h];       // [H]
let w_output = vec![0.1_f32; 1 * h];   // [O, H] row-major
let b_output = vec![0.0_f32; 1];       // [O]

let bnn = Bnn::from_parts(
    &w_input, &b_input,
    &[], &[],              // packed binary hidden layers (u64 words) + biases
    &w_output, &b_output,
    1,                     // output_size
).unwrap();

let score = bnn.predict(&[1.0, 1.0]);

// Add binary hidden layers by passing packed ±1 weights:
//   binary_weights: &[&[u64]]  each [H * H/64] words (bit 1 = +1, bit 0 = −1)
//   binary_biases:  &[&[f32]]  each [H], converted to integer thresholds
```

## Complexity

| Operation | Time | Space |
|-----------|------|-------|
| Construction (`from_parts`) | O(total_weights) | O(total_weights) |
| `predict` | O(H×I + N×H²/64 + O×H) | O(1) after construction |

The `N×H²/64` term is the binary core: N hidden layers, each H² weight
bits processed 64 at a time. Typical configurations:

| Shape | Binary layers | Latency | Note |
|-------|---------------|---------|------|
| 8→64→1 | 0 | 83 ns | fp32 in/out only |
| 8→64→1 | 1 | 195 ns | 16% faster than GBDT 50×6 |
| 8→64→1 | 2 | 309 ns | 37% faster than GBDT 100×6 |
| 8→128→1 | 2 | 666 ns | wider hidden |
