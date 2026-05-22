# NaN Handling

NaN validation is the caller's responsibility (standard ML
convention). All model types assume clean inputs on `predict()`.

## Per-Type NaN Behavior

| Type | `predict` | NaN-specific method |
|------|-----------|-------------------|
| GBDT | NaN routes right (`NaN <= threshold` is false) | `predict_nan_aware` — learned NaN routing |
| MLP | NaN propagates through computation | — |
| LUT | NaN maps to bin 0 (Rust saturating cast) | — |

**GBDT is the exception.** It can *handle* NaN via learned default
directions (LightGBM trains the optimal NaN routing). Use
`predict_nan_aware()` when features may contain missing values.
The cost is ~30% more cycles per node.

MLP and LUT have no learned NaN behavior. NaN in → garbage out.

## Where to Validate

NaN validation belongs in the **feature pipeline**, not at the
inference boundary:

```
  Feature producer  →  Validate/impute  →  Inference
  (may produce NaN)    (catch NaN here)    (clean inputs)
```

## MLP NaN Propagation

When NaN enters an MLP, it propagates through all operations:

- **Matmul**: `NaN * weight = NaN`, `NaN + bias = NaN`
- **Relu**: NaN passes through (IEEE 754: `NaN > 0.0` is false,
  `NaN <= 0.0` is false, third branch returns NaN)
- **LeakyRelu/Identity/Elu/Gelu/Swish**: all propagate NaN
- **Tanh/Sigmoid**: transcendentals propagate NaN

The output will be NaN, which the caller can detect.

## LUT NaN Behavior

Rust's saturating float-to-int cast maps `NaN as usize` to 0.
NaN features always index bin 0. The result is a valid number
from the table — but meaningless.

## Code Patterns

### Standard hot path

```rust
// Feature pipeline guarantees clean inputs
let score = model.predict(&features);
```

### GBDT with missing features

```rust
// GBDT routes NaN via learned default direction
let gbdt_score = gbdt_model.predict_nan_aware(&features);

// MLP requires clean inputs
let features_clean = impute_nan(&features);
let mlp_score = mlp_model.predict(&features_clean);
```
