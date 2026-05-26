//! Numerical compute kernels for the model hot paths.
//!
//! Pure functions — slices in, slices out, no model state. Each model file at
//! the crate root owns its struct, construction, and predict orchestration; the
//! heavy SIMD / integer compute lives here, so the model surface stays readable
//! and the perf-critical code sits in one place.
//!
//! SIMD kernels are compile-time gated on `target_feature`; a default build
//! uses the scalar fallbacks. See the README's "Build flags for SIMD".

pub(crate) mod activate;
pub(crate) mod binary;
pub(crate) mod dot;
pub(crate) mod gates;
pub(crate) mod gemv;
pub(crate) mod mlp;
pub(crate) mod quantized;
