use crate::math::MulAdd;

/// Jitter tracker — smoothed absolute deviation between consecutive samples.
///
/// Internally tracks an EMA of absolute consecutive deltas and an EMA of
/// values for computing the jitter ratio.
///
/// # Use Cases
/// - Network jitter (variation in inter-packet delay)
/// - Latency jitter (variation in response times)
/// - Clock stability monitoring
#[derive(Debug, Clone)]
pub struct JitterF64 {
    alpha: f64,
    one_minus_alpha: f64,
    jitter: f64,
    mean: f64,
    last_sample: f64,
    last_deviation: f64,
    count: u64,
    min_samples: u64,
}

/// Builder for [`JitterF64`].
#[derive(Debug, Clone)]
pub struct JitterF64Builder {
    alpha: Option<f64>,
    min_samples: u64,
    seed_value: Option<f64>,
    seed_jitter: Option<f64>,
}

impl JitterF64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> JitterF64Builder {
        JitterF64Builder {
            alpha: None,
            min_samples: 2,
            seed_value: None,
            seed_jitter: None,
        }
    }

    /// Feeds a sample. Returns smoothed jitter once primed.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<Option<f64>, crate::DataError> {
        check_finite!(sample);
        self.count += 1;

        if self.count == 1 {
            self.last_sample = sample;
            self.mean = sample;
            return Ok(None);
        }

        let abs_delta = (sample - self.last_sample).abs();
        self.last_deviation = abs_delta;
        self.last_sample = sample;

        if self.count == 2 {
            self.jitter = abs_delta;
        } else {
            self.jitter = self
                .alpha
                .fma(abs_delta, self.one_minus_alpha * self.jitter);
        }
        self.mean = self.alpha.fma(sample, self.one_minus_alpha * self.mean);

        if self.count >= self.min_samples {
            Ok(Some(self.jitter))
        } else {
            Ok(None)
        }
    }

    /// Current smoothed jitter (absolute deviation), or `None` if not primed.
    #[inline]
    #[must_use]
    pub fn jitter(&self) -> Option<f64> {
        if self.count >= self.min_samples {
            Some(self.jitter)
        } else {
            None
        }
    }

    /// Jitter as a fraction of the smoothed mean, or `None` if not primed
    /// or mean is near zero (absolute value < epsilon).
    #[inline]
    #[must_use]
    pub fn jitter_ratio(&self) -> Option<f64> {
        if self.count >= self.min_samples && self.mean.abs() > f64::EPSILON {
            Some(self.jitter / self.mean)
        } else {
            None
        }
    }

    /// Raw absolute deviation of the last two samples, or `None` if < 2 samples.
    #[inline]
    #[must_use]
    pub fn last_deviation(&self) -> Option<f64> {
        if self.count >= 2 {
            Some(self.last_deviation)
        } else {
            None
        }
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether enough data has been collected.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count >= self.min_samples
    }

    /// Resets to empty state. Parameters unchanged.
    #[inline]
    pub fn reset(&mut self) {
        self.jitter = 0.0;
        self.mean = 0.0;
        self.last_sample = 0.0;
        self.last_deviation = 0.0;
        self.count = 0;
    }
}

impl JitterF64Builder {
    /// Smoothing factor for jitter EMA.
    #[inline]
    #[must_use]
    pub fn alpha(mut self, alpha: f64) -> Self {
        self.alpha = Some(alpha);
        self
    }

    /// Halflife for jitter smoothing.
    #[inline]
    #[must_use]
    #[cfg(any(feature = "std", feature = "libm"))]
    pub fn halflife(mut self, halflife: f64) -> Self {
        let ln2 = core::f64::consts::LN_2;
        self.alpha = Some(1.0 - crate::math::exp(-ln2 / halflife));
        self
    }

    /// Span for jitter smoothing.
    #[inline]
    #[must_use]
    pub fn span(mut self, n: u64) -> Self {
        self.alpha = Some(2.0 / (n as f64 + 1.0));
        self
    }

    /// Minimum samples before jitter is valid. Default: 2.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Pre-loads the last sample value and smoothed jitter from calibration data.
    ///
    /// When seeded, `is_primed()` returns true immediately and the
    /// next `update()` computes a deviation against `value`.
    #[inline]
    #[must_use]
    pub fn seed(mut self, value: f64, jitter: f64) -> Self {
        self.seed_value = Some(value);
        self.seed_jitter = Some(jitter);
        self
    }

    /// Builds the jitter tracker.
    ///
    /// # Errors
    ///
    /// - Alpha must have been set.
    /// - Alpha must be in (0, 1) exclusive.
    #[inline]
    pub fn build(self) -> Result<JitterF64, crate::ConfigError> {
        let alpha = self.alpha.ok_or(crate::ConfigError::Missing("alpha"))?;
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(crate::ConfigError::Invalid(
                "Jitter alpha must be in (0, 1)",
            ));
        }

        let (last_sample, jitter, mean, count) = match (self.seed_value, self.seed_jitter) {
            (Some(v), Some(j)) => (v, j, v, self.min_samples),
            _ => (0.0, 0.0, 0.0, 0),
        };

        Ok(JitterF64 {
            alpha,
            one_minus_alpha: 1.0 - alpha,
            jitter,
            mean,
            last_sample,
            last_deviation: 0.0,
            count,
            min_samples: self.min_samples,
        })
    }
}

/// Jitter tracker (integer variant) — fixed-point EMA of absolute deltas.
///
/// Uses kernel-style bit-shift arithmetic. `jitter_ratio()` is not
/// available on integer types (integer division loses too much precision).
#[derive(Debug, Clone)]
pub struct JitterI64 {
    acc: i128,
    shift: u32,
    span: u64,
    last_sample: i64,
    last_deviation: i64,
    count: u64,
    min_samples: u64,
    initialized: bool,
}

/// Builder for [`JitterI64`].
#[derive(Debug, Clone)]
pub struct JitterI64Builder {
    span: Option<u64>,
    min_samples: u64,
}

impl JitterI64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> JitterI64Builder {
        JitterI64Builder {
            span: None,
            min_samples: 2,
        }
    }

    /// Feeds a sample. Returns smoothed jitter once primed.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> Option<i64> {
        self.count += 1;

        if self.count == 1 {
            self.last_sample = sample;
            return None;
        }

        let abs_delta = (sample - self.last_sample).abs();
        self.last_deviation = abs_delta;
        self.last_sample = sample;

        if self.initialized {
            let delta_shifted = (abs_delta as i128) << self.shift;
            self.acc += (delta_shifted - self.acc) >> self.shift;
        } else {
            self.acc = (abs_delta as i128) << self.shift;
            self.initialized = true;
        }

        if self.count >= self.min_samples {
            Some((self.acc >> self.shift) as i64)
        } else {
            None
        }
    }

    /// Current smoothed jitter, or `None` if not primed.
    #[inline]
    #[must_use]
    pub fn jitter(&self) -> Option<i64> {
        if self.count >= self.min_samples && self.initialized {
            Some((self.acc >> self.shift) as i64)
        } else {
            None
        }
    }

    /// Raw absolute deviation of the last two samples, or `None` if < 2.
    #[inline]
    #[must_use]
    pub fn last_deviation(&self) -> Option<i64> {
        if self.count >= 2 {
            Some(self.last_deviation)
        } else {
            None
        }
    }

    /// Effective span after rounding.
    #[inline]
    #[must_use]
    pub fn effective_span(&self) -> u64 {
        self.span
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether enough data has been collected.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count >= self.min_samples
    }

    /// Resets to empty state.
    #[inline]
    pub fn reset(&mut self) {
        self.acc = 0;
        self.last_sample = 0;
        self.last_deviation = 0;
        self.count = 0;
        self.initialized = false;
    }
}

impl JitterI64Builder {
    /// Smoothing span. Rounded up to next `2^k - 1`.
    #[inline]
    #[must_use]
    pub fn span(mut self, n: u64) -> Self {
        self.span = Some(n);
        self
    }

    /// Minimum samples before jitter is valid. Default: 2.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Builds the jitter tracker.
    ///
    /// # Errors
    ///
    /// - Span must have been set and >= 1.
    #[inline]
    pub fn build(self) -> Result<JitterI64, crate::ConfigError> {
        let requested = self.span.ok_or(crate::ConfigError::Missing("span"))?;
        if requested < 1 {
            return Err(crate::ConfigError::Invalid("Jitter span must be >= 1"));
        }

        let effective = crate::smoothing::ema::next_power_of_two_minus_one(requested);
        let shift = crate::smoothing::ema::log2_of_span_plus_one(effective);

        Ok(JitterI64 {
            acc: 0,
            shift,
            span: effective,
            last_sample: 0,
            last_deviation: 0,
            count: 0,
            min_samples: self.min_samples,
            initialized: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn constant_input_zero_jitter() {
        let mut j = JitterF64::builder().alpha(0.3).build().unwrap();
        for _ in 0..100 {
            let _ = j.update(100.0).unwrap();
        }
        let jitter = j.jitter().unwrap();
        assert!(
            jitter.abs() < 1e-10,
            "constant input should have ~zero jitter, got {jitter}"
        );
    }

    #[test]
    fn alternating_input_high_jitter() {
        let mut j = JitterF64::builder().alpha(0.5).build().unwrap();
        for i in 0..50 {
            let _ = j.update(if i % 2 == 0 { 100.0 } else { 200.0 }).unwrap();
        }
        let jitter = j.jitter().unwrap();
        assert!(
            jitter > 50.0,
            "alternating input should have high jitter, got {jitter}"
        );
    }

    #[test]
    fn jitter_ratio_correctness() {
        let mut j = JitterF64::builder().alpha(0.3).build().unwrap();
        for i in 0..100 {
            let _ = j.update(100.0 + (i % 10) as f64).unwrap();
        }
        let ratio = j.jitter_ratio().unwrap();
        assert!(
            ratio > 0.0 && ratio < 1.0,
            "ratio should be reasonable, got {ratio}"
        );
    }

    #[test]
    fn priming() {
        let mut j = JitterF64::builder()
            .alpha(0.3)
            .min_samples(5)
            .build()
            .unwrap();
        for _ in 0..4 {
            assert!(j.update(100.0).unwrap().is_none());
        }
        assert!(j.update(100.0).unwrap().is_some());
    }

    #[test]
    fn reset() {
        let mut j = JitterF64::builder().alpha(0.3).build().unwrap();
        for _ in 0..10 {
            let _ = j.update(100.0).unwrap();
        }
        j.reset();
        assert_eq!(j.count(), 0);
        assert!(j.jitter().is_none());
    }

    #[test]
    fn i64_basic() {
        let mut j = JitterI64::builder().span(7).build().unwrap();
        let _ = j.update(100);
        let _ = j.update(110);
        let _ = j.update(105);
        assert!(j.jitter().is_some());
    }

    #[test]
    fn seeded_is_primed() {
        let j = JitterF64::builder()
            .alpha(0.3)
            .seed(100.0, 5.0)
            .build()
            .unwrap();

        assert!(j.is_primed());
        assert!((j.jitter().unwrap() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn seeded_next_update_uses_seed_value() {
        let mut j = JitterF64::builder()
            .alpha(0.3)
            .seed(100.0, 5.0)
            .build()
            .unwrap();

        // Next update should compute deviation from seeded last_sample=100
        let result = j.update(110.0).unwrap();
        assert!(result.is_some());
        // Deviation is |110-100|=10, smoothed jitter = 0.3*10 + 0.7*5 = 6.5
        let jitter = result.unwrap();
        assert!((jitter - 6.5).abs() < 1e-10);
    }

    #[test]
    fn errors_without_alpha() {
        let result = JitterF64::builder().build();
        assert!(matches!(result, Err(crate::ConfigError::Missing("alpha"))));
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut j = JitterF64::builder().alpha(0.3).build().unwrap();
        assert!(matches!(
            j.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            j.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
        assert!(matches!(
            j.update(f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
