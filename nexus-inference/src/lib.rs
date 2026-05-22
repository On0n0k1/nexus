#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

//! ML inference engine for pre-trained models.
//!
//! This crate provides low-latency inference for models trained in
//! external frameworks (LightGBM, PyTorch, etc.). No training — just
//! fast prediction on the hot path.
//!
//! # Available Types
//!
//! - [`GbdtF64`] / [`GbdtF32`] — Gradient-boosted decision tree ensemble
//! - [`MlpF64`] / [`MlpF32`] — Feedforward neural network (multi-layer perceptron)
//! - [`LutF64`] / [`LutF32`] — Lookup table predictor (discretized features)

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
mod dot;
mod error;
mod gbdt;
mod lut;
mod mlp;

#[cfg(any(feature = "std", feature = "libm"))]
mod rnn;

#[cfg(feature = "alloc")]
mod conv;

#[cfg(feature = "loader-lightgbm")]
mod loader;

#[cfg(feature = "alloc")]
pub use conv::Causal1dConvF32;
pub use error::LoadError;
#[cfg(feature = "alloc")]
pub use gbdt::{GbdtF32, GbdtF64};
#[cfg(feature = "alloc")]
pub use lut::{LutF32, LutF64};
#[cfg(feature = "alloc")]
pub use mlp::{Activation, MlpF32, MlpF64};
#[cfg(any(feature = "std", feature = "libm"))]
pub use rnn::{TinyGruF32, TinyLstmF32};
