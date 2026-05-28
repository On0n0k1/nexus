use crate::Condition;
use crate::math::MulAdd;

/// Saturation detector — smoothed utilization with threshold.
///
/// Internally uses an EMA to smooth the utilization signal and
/// compares against a configured threshold.
///
/// # Use Cases
/// - CPU/memory utilization monitoring
/// - Queue fill level monitoring
/// - Bandwidth saturation detection
#[derive(Debug, Clone)]
pub struct SaturationF64 {
    alpha: f64,
    one_minus_alpha: f64,
    value: f64,
    threshold: f64,
    count: u64,
    min_samples: u64,
}

/// Builder for [`SaturationF64`].
#[derive(Debug, Clone)]
pub struct SaturationF64Builder {
    alpha: Option<f64>,
    threshold: Option<f64>,
    min_samples: u64,
}

impl SaturationF64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> SaturationF64Builder {
        SaturationF64Builder {
            alpha: None,
            threshold: None,
            min_samples: 1,
        }
    }

    /// Feeds a utilization sample. Returns pressure state once primed.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, utilization: f64) -> Result<Option<Condition>, crate::DataError> {
        check_finite!(utilization);
        self.count += 1;

        if self.count == 1 {
            self.value = utilization;
        } else {
            self.value = self
                .alpha
                .fma(utilization, self.one_minus_alpha * self.value);
        }

        if self.count < self.min_samples {
            return Ok(None);
        }

        Ok(if self.value > self.threshold {
            Some(Condition::Degraded)
        } else {
            Some(Condition::Normal)
        })
    }

    /// Current smoothed utilization, or `None` if not primed.
    #[inline]
    #[must_use]
    pub fn utilization(&self) -> Option<f64> {
        if self.count >= self.min_samples {
            Some(self.value)
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
        self.value = 0.0;
        self.count = 0;
    }

    /// Updates the saturation threshold without resetting state.
    #[inline]
    pub fn reconfigure_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }
}

impl SaturationF64Builder {
    /// Smoothing factor.
    #[inline]
    #[must_use]
    pub fn alpha(mut self, alpha: f64) -> Self {
        self.alpha = Some(alpha);
        self
    }

    /// Halflife for smoothing.
    #[inline]
    #[must_use]
    #[cfg(any(feature = "std", feature = "libm"))]
    pub fn halflife(mut self, halflife: f64) -> Self {
        let ln2 = core::f64::consts::LN_2;
        self.alpha = Some(1.0 - crate::math::exp(-ln2 / halflife));
        self
    }

    /// Span for smoothing.
    #[inline]
    #[must_use]
    pub fn span(mut self, n: u64) -> Self {
        self.alpha = Some(2.0 / (n as f64 + 1.0));
        self
    }

    /// Saturation threshold. Default must be set.
    #[inline]
    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Minimum samples before detection activates. Default: 1.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Builds the saturation detector.
    ///
    /// # Errors
    ///
    /// - Alpha and threshold must have been set.
    /// - Alpha must be in (0, 1) exclusive.
    #[inline]
    pub fn build(self) -> Result<SaturationF64, crate::ConfigError> {
        let alpha = self.alpha.ok_or(crate::ConfigError::Missing("alpha"))?;
        let threshold = self
            .threshold
            .ok_or(crate::ConfigError::Missing("threshold"))?;
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(crate::ConfigError::Invalid("alpha must be in (0, 1)"));
        }

        Ok(SaturationF64 {
            alpha,
            one_minus_alpha: 1.0 - alpha,
            value: 0.0,
            threshold,
            count: 0,
            min_samples: self.min_samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_threshold_is_normal() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .build()
            .unwrap();

        for _ in 0..50 {
            assert_eq!(s.update(0.5).unwrap(), Some(Condition::Normal));
        }
    }

    #[test]
    fn above_threshold_is_saturated() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .build()
            .unwrap();

        for _ in 0..50 {
            let _ = s.update(0.95);
        }
        assert_eq!(s.update(0.95).unwrap(), Some(Condition::Degraded));
    }

    #[test]
    fn crosses_back() {
        let mut s = SaturationF64::builder()
            .alpha(0.5)
            .threshold(0.8)
            .build()
            .unwrap();

        // Drive up
        for _ in 0..50 {
            let _ = s.update(0.95);
        }
        assert_eq!(s.update(0.95).unwrap(), Some(Condition::Degraded));

        // Drive down
        for _ in 0..50 {
            let _ = s.update(0.3);
        }
        assert_eq!(s.update(0.3).unwrap(), Some(Condition::Normal));
    }

    #[test]
    fn priming() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .min_samples(5)
            .build()
            .unwrap();

        for _ in 0..4 {
            assert!(s.update(0.95).unwrap().is_none());
        }
        assert!(s.update(0.95).unwrap().is_some());
    }

    #[test]
    fn reset() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .build()
            .unwrap();

        for _ in 0..10 {
            let _ = s.update(0.95);
        }
        s.reset();
        assert_eq!(s.count(), 0);
        assert!(s.utilization().is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn reconfigure_threshold_changes_behavior() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .build()
            .unwrap();

        for _ in 0..50 {
            let _ = s.update(0.75);
        }
        assert_eq!(s.update(0.75).unwrap(), Some(Condition::Normal));

        // Lower the threshold — same value should now be degraded
        s.reconfigure_threshold(0.7);
        assert_eq!(s.update(0.75).unwrap(), Some(Condition::Degraded));
    }

    #[test]
    fn errors_without_threshold() {
        let result = SaturationF64::builder().alpha(0.3).build();
        assert!(matches!(
            result,
            Err(crate::ConfigError::Missing("threshold"))
        ));
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut s = SaturationF64::builder()
            .alpha(0.3)
            .threshold(0.8)
            .build()
            .unwrap();

        assert_eq!(
            s.update(f64::NAN).unwrap_err(),
            crate::DataError::NotANumber
        );
        assert_eq!(
            s.update(f64::INFINITY).unwrap_err(),
            crate::DataError::Infinite
        );
        assert_eq!(
            s.update(f64::NEG_INFINITY).unwrap_err(),
            crate::DataError::Infinite
        );
        assert_eq!(s.count(), 0);
    }
}
