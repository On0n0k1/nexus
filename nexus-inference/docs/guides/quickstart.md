# Quickstart

## Add the dependency

```toml
[dependencies]
nexus-inference = { version = "0.1", features = ["loader-lightgbm"] }
```

## Load and predict with a GBDT

```rust
use nexus_inference::GbdtF64;

// Load from LightGBM text format (model_text.txt)
let bytes = std::fs::read("model_text.txt").unwrap();
let model = GbdtF64::from_lightgbm(&bytes).unwrap();

let features = vec![0.5, 1.2, -0.3, 0.8, 2.1, 0.0, -1.5, 3.3];
let score = model.predict(&features);

// NaN-aware routing (when features may contain NaN)
let score = model.predict_nan_aware(&features);
```

## Load and predict with an MLP

```rust
use nexus_inference::{MlpF64, Activation};

// Weights exported from PyTorch (see python-export.md)
let layer_sizes = &[4, 8, 1];  // 4 inputs → 8 hidden → 1 output
let weights: Vec<f64> = load_weights();  // 4*8 + 8*1 = 40 values
let biases: Vec<f64> = load_biases();    // 8 + 1 = 9 values

let mut model = MlpF64::from_parts(
    layer_sizes, &weights, &biases, Activation::Relu,
).unwrap();

let score = model.predict(&[0.5, 1.2, -0.3, 0.8]);
```

## Load and predict with a LUT

```rust
use nexus_inference::LutF64;

// Pre-computed table: 2 features, 10 bins each
let table: Vec<f64> = load_table();  // 100 values

let model = LutF64::from_parts(
    2,              // n_features
    10,             // n_bins
    &[0.0, 0.0],   // feature minimums
    &[1.0, 1.0],   // feature maximums
    &table,
).unwrap();

let value = model.predict(&[0.35, 0.72]);
```

## Multi-output MLP

```rust
use nexus_inference::{MlpF64, Activation};

// 4 inputs → 8 hidden → 3 outputs
let mut model = MlpF64::from_parts(
    &[4, 8, 3], &weights, &biases, Activation::Relu,
).unwrap();

// predict() panics for multi-output — use predict_into
let mut output = [0.0_f64; 3];
model.predict_into(&[0.5, 1.2, -0.3, 0.8], &mut output);
// output[0], output[1], output[2] now contain the three predictions
```

## Handling errors

```rust
use nexus_inference::{MlpF64, Activation, LoadError};

// Construction errors
let result = MlpF64::from_parts(&[2, 0, 1], &[], &[], Activation::Relu);
match result {
    Err(LoadError::Validation(msg)) => eprintln!("bad model: {msg}"),
    Err(LoadError::Parse(msg)) => eprintln!("parse error: {msg}"),
    Ok(model) => { /* use model */ }
}
```
