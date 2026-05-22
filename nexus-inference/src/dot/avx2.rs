#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

use super::scalar;

#[inline]
#[cfg(target_arch = "x86_64")]
pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
    let len = a.len();
    let mut i = 0;

    // SAFETY: AVX2+FMA guaranteed by target_feature cfg on parent module.
    // All pointer offsets satisfy i + N <= len before access.
    let sum = unsafe {
        let mut acc0 = _mm256_setzero_pd();
        let mut acc1 = _mm256_setzero_pd();
        let mut acc2 = _mm256_setzero_pd();
        let mut acc3 = _mm256_setzero_pd();

        while i + 16 <= len {
            let a0 = _mm256_loadu_pd(a.as_ptr().add(i));
            let b0 = _mm256_loadu_pd(b.as_ptr().add(i));
            let a1 = _mm256_loadu_pd(a.as_ptr().add(i + 4));
            let b1 = _mm256_loadu_pd(b.as_ptr().add(i + 4));
            let a2 = _mm256_loadu_pd(a.as_ptr().add(i + 8));
            let b2 = _mm256_loadu_pd(b.as_ptr().add(i + 8));
            let a3 = _mm256_loadu_pd(a.as_ptr().add(i + 12));
            let b3 = _mm256_loadu_pd(b.as_ptr().add(i + 12));
            acc0 = _mm256_fmadd_pd(a0, b0, acc0);
            acc1 = _mm256_fmadd_pd(a1, b1, acc1);
            acc2 = _mm256_fmadd_pd(a2, b2, acc2);
            acc3 = _mm256_fmadd_pd(a3, b3, acc3);
            i += 16;
        }

        while i + 4 <= len {
            let av = _mm256_loadu_pd(a.as_ptr().add(i));
            let bv = _mm256_loadu_pd(b.as_ptr().add(i));
            acc0 = _mm256_fmadd_pd(av, bv, acc0);
            i += 4;
        }

        acc0 = _mm256_add_pd(
            _mm256_add_pd(acc0, acc1),
            _mm256_add_pd(acc2, acc3),
        );

        let hi = _mm256_extractf128_pd(acc0, 1);
        let lo = _mm256_castpd256_pd128(acc0);
        let pair = _mm_add_pd(lo, hi);
        let high_lane = _mm_unpackhi_pd(pair, pair);
        _mm_cvtsd_f64(_mm_add_sd(pair, high_lane))
    };

    sum + scalar::dot_f64(&a[i..], &b[i..])
}

#[inline]
#[cfg(target_arch = "x86_64")]
pub fn dot_f32(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len();
    let mut i = 0;

    // SAFETY: AVX2+FMA guaranteed by target_feature cfg on parent module.
    // All pointer offsets satisfy i + N <= len before access.
    let sum = unsafe {
        let mut acc0 = _mm256_setzero_ps();
        let mut acc1 = _mm256_setzero_ps();
        let mut acc2 = _mm256_setzero_ps();
        let mut acc3 = _mm256_setzero_ps();

        while i + 32 <= len {
            let a0 = _mm256_loadu_ps(a.as_ptr().add(i));
            let b0 = _mm256_loadu_ps(b.as_ptr().add(i));
            let a1 = _mm256_loadu_ps(a.as_ptr().add(i + 8));
            let b1 = _mm256_loadu_ps(b.as_ptr().add(i + 8));
            let a2 = _mm256_loadu_ps(a.as_ptr().add(i + 16));
            let b2 = _mm256_loadu_ps(b.as_ptr().add(i + 16));
            let a3 = _mm256_loadu_ps(a.as_ptr().add(i + 24));
            let b3 = _mm256_loadu_ps(b.as_ptr().add(i + 24));
            acc0 = _mm256_fmadd_ps(a0, b0, acc0);
            acc1 = _mm256_fmadd_ps(a1, b1, acc1);
            acc2 = _mm256_fmadd_ps(a2, b2, acc2);
            acc3 = _mm256_fmadd_ps(a3, b3, acc3);
            i += 32;
        }

        while i + 8 <= len {
            let av = _mm256_loadu_ps(a.as_ptr().add(i));
            let bv = _mm256_loadu_ps(b.as_ptr().add(i));
            acc0 = _mm256_fmadd_ps(av, bv, acc0);
            i += 8;
        }

        acc0 = _mm256_add_ps(
            _mm256_add_ps(acc0, acc1),
            _mm256_add_ps(acc2, acc3),
        );

        let hi = _mm256_extractf128_ps(acc0, 1);
        let lo = _mm256_castps256_ps128(acc0);
        let sum128 = _mm_add_ps(lo, hi);
        let shuf = _mm_movehdup_ps(sum128);
        let sums = _mm_add_ps(sum128, shuf);
        let shuf2 = _mm_movehl_ps(sums, sums);
        _mm_cvtss_f32(_mm_add_ss(sums, shuf2))
    };

    sum + scalar::dot_f32(&a[i..], &b[i..])
}
