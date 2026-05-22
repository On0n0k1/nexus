mod gru;
mod lstm;

pub use gru::TinyGruF32;
pub use lstm::TinyLstmF32;

#[inline(always)]
pub(crate) fn sigmoid_f32(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        (1.0_f64 / (1.0_f64 + (-(x as f64)).exp())) as f32
    }
    #[cfg(all(not(feature = "std"), feature = "libm"))]
    {
        (1.0_f64 / (1.0_f64 + libm::exp(-(x as f64)))) as f32
    }
}

#[inline(always)]
pub(crate) fn tanh_f32(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        (x as f64).tanh() as f32
    }
    #[cfg(all(not(feature = "std"), feature = "libm"))]
    {
        libm::tanh(x as f64) as f32
    }
}
