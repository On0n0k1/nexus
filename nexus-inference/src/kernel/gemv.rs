//! Tiled GEMV: `weights × input + bias → activation`, 4/8 outputs at a time.
//!
//! The dense-layer forward shared by MLP (one call per layer) and the
//! convolution models (over an im2col-linearized window — the sliding-window
//! part stays in the models). For a linear output, pass `Activation::Identity`.

#[cfg(all(
    target_arch = "x86_64",
    any(
        target_feature = "avx512f",
        all(target_feature = "avx2", target_feature = "fma"),
    )
))]
use crate::Activation;

#[cfg(all(
    target_arch = "x86_64",
    any(
        target_feature = "avx512f",
        all(target_feature = "avx2", target_feature = "fma"),
    )
))]
#[inline(never)]
pub(crate) fn tiled_gemv(
    weights: &[f32],
    biases: &[f32],
    src: &[f32],
    dst: &mut [f32],
    in_size: usize,
    out_size_4: usize,
    activation: Activation,
) -> usize {
    use crate::kernel::activate::simd::{activate_4wide, activate_8wide};
    use crate::kernel::dot::{dot4_f32_m128, dot8_f32_m256};
    use core::arch::x86_64::*;
    let out_size_8 = out_size_4 & !7;
    let mut j = 0;

    // SAFETY: cfg-gated to x86_64 + (avx512f | avx2+fma), so the intrinsics and
    // dot kernels are available. The 8-wide loop holds `j + 8 <= out_size_8 <=
    // out_size_4` and the 4-wide tail `j + 4 <= out_size_4`; the caller sizes
    // `biases` and `dst` to at least `out_size_4`. Loads/stores are unaligned.
    unsafe {
        // 8-wide loop (requires in_size >= 32 to amortize dot8 overhead)
        if in_size >= 32 {
            while j < out_size_8 {
                let rows = &weights[j * in_size..(j + 8) * in_size];
                let dots = dot8_f32_m256(rows, src);
                let bias_v = _mm256_loadu_ps(biases.as_ptr().add(j));
                let with_bias = _mm256_add_ps(dots, bias_v);
                match activate_8wide(with_bias, activation) {
                    Some(activated) => _mm256_storeu_ps(dst.as_mut_ptr().add(j), activated),
                    None => return j,
                }
                j += 8;
            }
        }

        // 4-wide tail
        while j < out_size_4 {
            let rows = &weights[j * in_size..(j + 4) * in_size];
            let dots = dot4_f32_m128(rows, src);
            let bias_v = _mm_loadu_ps(biases.as_ptr().add(j));
            let with_bias = _mm_add_ps(dots, bias_v);
            match activate_4wide(with_bias, activation) {
                Some(activated) => _mm_storeu_ps(dst.as_mut_ptr().add(j), activated),
                None => return j,
            }
            j += 4;
        }
    }
    j
}
