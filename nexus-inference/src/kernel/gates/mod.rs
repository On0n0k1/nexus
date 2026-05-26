//! LSTM / GRU gate kernels.
//!
//! `apply_lstm_gates` / `apply_gru_gates` dispatch to the AVX-512, AVX2, or
//! scalar implementation at compile time. The SIMD backends live in the
//! sibling `avx2` / `avx512` modules.

use crate::kernel::activate::{sigmoid_f32, tanh_f32};

#[cfg(all(target_arch = "x86_64", target_feature = "avx512f"))]
mod avx512;

#[cfg(all(
    target_arch = "x86_64",
    target_feature = "avx2",
    target_feature = "fma",
    not(target_feature = "avx512f"),
))]
mod avx2;

#[inline(always)]
pub(crate) fn apply_lstm_gates(gates: &[f32], c: &mut [f32], h: &mut [f32], hidden_size: usize) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx512f"))]
    {
        avx512::lstm_gates_avx512(gates, c, h, hidden_size);
    }

    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx2",
        target_feature = "fma",
        not(target_feature = "avx512f"),
    ))]
    {
        avx2::lstm_gates_avx2(gates, c, h, hidden_size);
    }

    #[cfg(not(all(
        target_arch = "x86_64",
        any(
            target_feature = "avx512f",
            all(target_feature = "avx2", target_feature = "fma"),
        )
    )))]
    {
        for k in 0..hidden_size {
            let ig = sigmoid_f32(gates[k]);
            let fg = sigmoid_f32(gates[hidden_size + k]);
            let cg = tanh_f32(gates[2 * hidden_size + k]);
            let og = sigmoid_f32(gates[3 * hidden_size + k]);

            c[k] = fg.mul_add(c[k], ig * cg);
            h[k] = og * tanh_f32(c[k]);
        }
    }
}

#[inline(always)]
pub(crate) fn apply_gru_gates(
    ih_scratch: &[f32],
    hh_scratch: &[f32],
    bias_ih: &[f32],
    bias_hh: &[f32],
    h: &mut [f32],
    hidden_size: usize,
) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx512f"))]
    {
        avx512::gru_gates_avx512(ih_scratch, hh_scratch, bias_ih, bias_hh, h, hidden_size);
    }

    #[cfg(all(
        target_arch = "x86_64",
        target_feature = "avx2",
        target_feature = "fma",
        not(target_feature = "avx512f"),
    ))]
    {
        avx2::gru_gates_avx2(ih_scratch, hh_scratch, bias_ih, bias_hh, h, hidden_size);
    }

    #[cfg(not(all(
        target_arch = "x86_64",
        any(
            target_feature = "avx512f",
            all(target_feature = "avx2", target_feature = "fma"),
        )
    )))]
    {
        let hi = hidden_size;
        for k in 0..hi {
            let r = sigmoid_f32(ih_scratch[k] + bias_ih[k] + hh_scratch[k] + bias_hh[k]);
            let z = sigmoid_f32(
                ih_scratch[hi + k] + bias_ih[hi + k] + hh_scratch[hi + k] + bias_hh[hi + k],
            );
            let hh_candidate = hh_scratch[2 * hi + k] + bias_hh[2 * hi + k];
            let n = tanh_f32(r.mul_add(hh_candidate, ih_scratch[2 * hi + k] + bias_ih[2 * hi + k]));
            h[k] = (1.0 - z).mul_add(n, z * h[k]);
        }
    }
}
