#![warn(missing_docs)]

//! Real-time CPU inference for small, pre-trained models.
//!
//! This crate runs tiny models on the CPU — the kind that sit in an
//! event loop and make decisions at wire speed. Models are trained
//! externally (LightGBM, PyTorch), loaded once via `from_parts`, and
//! called millions of times. Sub-microsecond prediction, zero allocation
//! after construction.
//!
//! # Stateless (single prediction)
//!
//! - [`Gbdt`] — Gradient-boosted decision tree ensemble
//! - [`Mlp`] — Feedforward neural network (multi-layer perceptron)
//! - [`Lut`] — Lookup table predictor (discretized features)
//! - [`Bnn`] — Binary neural network (XNOR+popcount inference)
//! - [`QuantizedMlp`] — Int8-quantized MLP (i8 matmul, f32 activations)
//!
//! # Stateful (streaming temporal)
//!
//! - [`TinyLstm`] — Single-layer LSTM
//! - [`TinyGru`] — Single-layer GRU, ~75% of LSTM cost
//! - [`StackedLstm`] — Multi-layer LSTM (PyTorch `nn.LSTM(num_layers=N)`)
//! - [`StackedGru`] — Multi-layer GRU (PyTorch `nn.GRU(num_layers=N)`)
//! - [`LinearSsm`] — Linear state-space model (S4/S4D), no transcendentals
//! - [`Causal1dConv`] — Causal 1D convolution over a sliding window
//! - [`TinyTcn`] — Temporal convolutional network (dilated causal conv stack)

use std::cell::UnsafeCell;

mod activation;
mod bnn;
mod conv;
mod dot;
mod error;
mod gbdt;
mod lut;
mod mlp;
mod quantized_mlp;
mod rnn;
mod ssm;
mod validate;

#[cfg(any(feature = "loader-lightgbm", feature = "safetensors"))]
mod loader;

pub use activation::Activation;
pub use bnn::Bnn;
pub use conv::{Causal1dConv, TinyTcn};
pub use error::LoadError;
pub use gbdt::Gbdt;
pub use lut::Lut;
pub use mlp::Mlp;
pub use quantized_mlp::QuantizedMlp;
pub use rnn::{StackedGru, StackedLstm, TinyGru, TinyLstm};
pub use ssm::LinearSsm;

/// Inference model with mutable access.
///
/// All model types implement this trait. Stateful models (LSTM, GRU, Conv, SSM)
/// carry hidden state between calls. Stateless models (GBDT, MLP, LUT) produce
/// the same output for the same input regardless of call history.
///
/// Use `&mut dyn Model` for dynamic dispatch over mixed model types.
pub trait Model {
    /// Single-output prediction.
    ///
    /// # Panics
    ///
    /// Panics if `n_outputs() != 1` or input length doesn't match.
    fn predict(&mut self, input: &[f32]) -> f32;

    /// Multi-output prediction.
    ///
    /// # Panics
    ///
    /// Panics if input or output length doesn't match.
    fn predict_into(&mut self, input: &[f32], output: &mut [f32]);

    /// Number of output values.
    fn n_outputs(&self) -> usize;
}

/// Marker: output depends only on input, not call history.
///
/// Stateless models (GBDT, MLP, LUT, BNN, QuantizedMLP) also provide
/// inherent `predict(&self)` methods for use without exclusive access.
pub trait StatelessModel: Model {}

/// Generate the `Model` trait impl for a model type by delegating to its
/// inherent `predict` / `predict_into` / `n_outputs` methods. Pass a trailing
/// `, stateless` to also mark the type [`StatelessModel`].
macro_rules! impl_model {
    ($ty:ty) => {
        impl $crate::Model for $ty {
            fn predict(&mut self, input: &[f32]) -> f32 {
                <$ty>::predict(self, input)
            }
            fn predict_into(&mut self, input: &[f32], output: &mut [f32]) {
                <$ty>::predict_into(self, input, output);
            }
            fn n_outputs(&self) -> usize {
                <$ty>::n_outputs(self)
            }
        }
    };
    ($ty:ty, stateless) => {
        $crate::impl_model!($ty);
        impl $crate::StatelessModel for $ty {}
    };
}
pub(crate) use impl_model;

/// Interior-mutable scratch buffer for stateless models.
///
/// Wraps `UnsafeCell` so that `predict(&self)` can mutate scratch memory
/// without requiring `&mut self`. The type is `!Sync` (cannot be shared
/// across threads), matching the intended single-threaded usage.
#[derive(Debug)]
pub(crate) struct Scratch<T>(UnsafeCell<T>);

impl<T: Clone> Clone for Scratch<T> {
    fn clone(&self) -> Self {
        // SAFETY: clone is not called during predict (single-threaded, not reentrant).
        Self(UnsafeCell::new(unsafe { &*self.0.get() }.clone()))
    }
}

impl<T> Scratch<T> {
    pub(crate) fn new(val: T) -> Self {
        Self(UnsafeCell::new(val))
    }

    /// # Safety
    ///
    /// Caller must ensure no other mutable or shared references to the
    /// inner value exist for the duration of the returned borrow.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn get_mut(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}
