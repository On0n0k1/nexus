# SSM — Linear State-Space Model

**A linear recurrence with a diagonal state transition.** Each hidden
dimension is an independent first-order filter with its own decay rate.
No gates, no transcendentals — just multiply-add — which makes it the
**fastest temporal model in the crate** and a clean fit for long-range
memory where LSTM forget gates leak signal.

| Property | Value |
|----------|-------|
| Prediction cost | 42 ns (4→8→1) to 131 ns (16→64→1) — 2.5x-8x faster than LSTM |
| Memory | 4 B × (H + H×I + O×H + O×I); ~56 B for a tiny model |
| Type | `LinearSsm` (stateful — carries hidden state) |
| Construction | `from_parts()` (pre-discretized A_d, B, C, D) |
| Output | Single scalar, or multi-output via `predict_into` |

## What It Does

One timestep applies a pre-discretized linear system:

```
  h_t = A ⊙ h_{t-1} + B @ u_t       state update  (A is diagonal → element-wise)
  y_t = C @ h_t     + D @ u_t       output         (D is the optional skip)

       u_t ──► B @ ──┐
                     ▼
   h_{t-1} ─► A ⊙ ──(+)──► h_t ──► C @ ──(+)──► y_t
                                            ▲
       u_t ──────────────── D @ ────────────┘
```

Because `A` is **diagonal**, the state transition is an element-wise
multiply, not a matrix multiply: each state dimension `h[i]` is its own
first-order recurrence `h[i] ← a[i]·h[i] + (B u)[i]` with its own decay
`a[i]`. A dimension with `a[i] ≈ 0.999` remembers for thousands of steps;
one with `a[i] ≈ 0.5` forgets in a handful. The model is a bank of
independent leaky integrators read out by `C`.

## Why linear + diagonal is the point

- **No transcendentals.** Unlike LSTM/GRU (sigmoid/tanh gates), every
  operation is a multiply-add. The `A ⊙ h` recurrence is a scalar loop that
  auto-vectorizes; the `B @ u` and `C @ h` products reuse the crate's shared
  SIMD dot kernels. No gate approximations on the hot path — deterministic
  latency.
- **Long memory without leakage.** An LSTM's forget gate is a learned
  scalar in (0,1) applied every step; over long horizons it bleeds the
  signal away. A diagonal SSM with `a[i]` near 1 holds state for very long
  ranges by construction — ideal for regime memory (vol over hours,
  correlation over days).
- **Linear, deliberately.** This is an S4/S4D-style *linear* SSM. The
  expressiveness comes from many decay rates and the readout, not from a
  per-step nonlinearity. If you need input-dependent gating, that's an
  LSTM/GRU.

You train in Python (S4, S4D, or a custom SSM), **discretize** to obtain
`A_d, B_d, C, D`, and export the matrices via safetensors. A missing `D`
is treated as zeros (no skip connection).

## When to Use It

**Use SSM when:**
- You need **long-range temporal memory** and an [LSTM](lstm.md)'s gates
  forget too fast (regime detection over hours/days).
- You want the **lowest-latency temporal model** — 2.5x-8x faster than LSTM.
- Determinism matters — no transcendental approximations on the hot path.
- You trained an **S4/S4D** model in Python and discretized it.

**Don't use SSM when:**
- The pattern is **short-range** — a [Causal1dConv](causal1d.md) or
  [TCN](tcn.md) over a fixed window is cheaper and simpler.
- You need **input-dependent gating** or per-step nonlinearity — use
  [LSTM](lstm.md) / [GRU](gru.md).
- The dynamics are **strongly nonlinear** — this model is linear by design.

## Output Interpretation

`predict()` advances the state one timestep and returns `y_t` (fp32).
State persists across calls; call `reset()` at a sequence boundary (new
instrument, gap, session start) to clear it.

## Code Example

```rust
use nexus_inference::LinearSsm;

// 2 inputs → 4-dim state → 1 output. Parameters are PRE-discretized.
let a_diag = vec![0.9_f32; 4];      // [H] diagonal of A (per-dim decay)
let b      = vec![0.1_f32; 4 * 2];  // [H, I] input-to-state, row-major
let c      = vec![0.1_f32; 1 * 4];  // [O, H] state-to-output, row-major
let d      = vec![0.0_f32; 1 * 2];  // [O, I] skip; all-zeros = no skip

let mut ssm = LinearSsm::from_parts(&a_diag, &b, &c, &d, 1).unwrap();

let y0 = ssm.predict(&[0.5, 1.0]);   // advances state
let y1 = ssm.predict(&[0.6, 0.9]);   // remembers y0's state

ssm.reset();                          // clear state at a sequence boundary
```

## Complexity

| Operation | Time | Space |
|-----------|------|-------|
| Construction (`from_parts`) | O(total_weights) | O(total_weights) |
| `predict` (one step) | O(H×I + H + H×O + I×O) | O(H) state |

| Shape | Latency | vs LSTM |
|-------|---------|---------|
| 4→8→1 | 42 ns | 2.5x faster |
| 8→16→1 | 55 ns | 2.5x faster |
| 8→32→1 | 74 ns | 4x faster |
| 16→64→1 | 131 ns | 8x faster |

Memory is tiny — a `[H + H×I + O×H + O×I]` block of fp32. An I=2, H=2,
O=1 model is 56 bytes; weights live in L1 indefinitely.
