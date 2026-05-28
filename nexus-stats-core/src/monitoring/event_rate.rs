use crate::math::MulAdd;

/// Smoothed event rate tracker.
///
/// Uses an EMA of inter-arrival times, inverted on query to produce
/// a rate (events per unit time). The rate adapts smoothly to changes
/// in event frequency.
///
/// # Use Cases
/// - Message throughput monitoring
/// - Order rate tracking
/// - Adaptive rate limiting input
#[derive(Debug, Clone)]
pub struct EventRateF64 {
    alpha: f64,
    one_minus_alpha: f64,
    interval: f64,
    last_timestamp: f64,
    count: u64,
    min_samples: u64,
}

/// Builder for [`EventRateF64`].
#[derive(Debug, Clone)]
pub struct EventRateF64Builder {
    alpha: Option<f64>,
    min_samples: u64,
}

impl EventRateF64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> EventRateF64Builder {
        EventRateF64Builder {
            alpha: None,
            min_samples: 2,
        }
    }

    /// Updates with an event at the given timestamp.
    ///
    /// If two events share a timestamp, the interval is zero and
    /// `rate()` returns `None` until a non-zero interval is observed.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the timestamp is NaN, or
    /// `DataError::Infinite` if the timestamp is infinite.
    #[inline]
    pub fn update(&mut self, timestamp: f64) -> Result<(), crate::DataError> {
        check_finite!(timestamp);
        self.count += 1;

        if self.count == 1 {
            self.last_timestamp = timestamp;
            return Ok(());
        }

        let dt = timestamp - self.last_timestamp;
        self.last_timestamp = timestamp;

        if self.count == 2 {
            self.interval = dt;
        } else {
            self.interval = self.alpha.fma(dt, self.one_minus_alpha * self.interval);
        }
        Ok(())
    }

    /// Current smoothed event rate (events per unit time).
    ///
    /// Returns `None` if not primed or if interval is zero.
    #[inline]
    #[must_use]
    pub fn rate(&self) -> Option<f64> {
        if self.count < self.min_samples || self.interval <= 0.0 {
            None
        } else {
            Some(1.0 / self.interval)
        }
    }

    /// Current smoothed inter-event interval, or `None` if < 2 events.
    #[inline]
    #[must_use]
    pub fn interval(&self) -> Option<f64> {
        if self.count >= 2 {
            Some(self.interval)
        } else {
            None
        }
    }

    /// Number of events recorded.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether the tracker has reached `min_samples`.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count >= self.min_samples
    }

    /// Resets to uninitialized state.
    #[inline]
    pub fn reset(&mut self) {
        self.interval = 0.0;
        self.last_timestamp = 0.0;
        self.count = 0;
    }
}

impl EventRateF64Builder {
    /// Direct smoothing factor for interval EMA.
    #[inline]
    #[must_use]
    pub fn alpha(mut self, alpha: f64) -> Self {
        self.alpha = Some(alpha);
        self
    }

    /// Halflife for interval smoothing.
    #[inline]
    #[must_use]
    #[cfg(any(feature = "std", feature = "libm"))]
    pub fn halflife(mut self, halflife: f64) -> Self {
        let ln2 = core::f64::consts::LN_2;
        let alpha = 1.0 - crate::math::exp(-ln2 / halflife);
        self.alpha = Some(alpha);
        self
    }

    /// Span for interval smoothing.
    #[inline]
    #[must_use]
    pub fn span(mut self, n: u64) -> Self {
        let alpha = 2.0 / (n as f64 + 1.0);
        self.alpha = Some(alpha);
        self
    }

    /// Minimum events before rate is valid. Default: 2.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Builds the event rate tracker.
    ///
    /// # Errors
    ///
    /// - Alpha must have been set.
    /// - Alpha must be in (0, 1) exclusive.
    #[inline]
    pub fn build(self) -> Result<EventRateF64, crate::ConfigError> {
        let alpha = self.alpha.ok_or(crate::ConfigError::Missing("alpha"))?;
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(crate::ConfigError::Invalid(
                "EventRate alpha must be in (0, 1)",
            ));
        }

        Ok(EventRateF64 {
            alpha,
            one_minus_alpha: 1.0 - alpha,
            interval: 0.0,
            last_timestamp: 0.0,
            count: 0,
            min_samples: self.min_samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_rate() {
        let mut er = EventRateF64::builder().alpha(0.3).build().unwrap();

        // Events every 10 units -> rate should converge to 0.1
        for i in 0..100 {
            er.update(i as f64 * 10.0).unwrap();
        }

        let rate = er.rate().unwrap();
        assert!((rate - 0.1).abs() < 0.01, "rate should be ~0.1, got {rate}");
    }

    #[test]
    fn burst_increases_rate() {
        let mut er = EventRateF64::builder().alpha(0.5).build().unwrap();

        // Normal rate: every 10 units
        for i in 0..20 {
            er.update(i as f64 * 10.0).unwrap();
        }
        let normal_rate = er.rate().unwrap();

        // Burst: events every 1 unit
        for i in 0..20 {
            er.update(200.0 + i as f64).unwrap();
        }
        let burst_rate = er.rate().unwrap();

        assert!(
            burst_rate > normal_rate,
            "burst rate ({burst_rate}) should exceed normal ({normal_rate})"
        );
    }

    #[test]
    fn priming() {
        let mut er = EventRateF64::builder()
            .alpha(0.3)
            .min_samples(5)
            .build()
            .unwrap();

        for i in 0..4 {
            er.update(i as f64 * 10.0).unwrap();
            assert!(er.rate().is_none());
        }
        er.update(40.0).unwrap();
        assert!(er.rate().is_some());
    }

    #[test]
    fn reset() {
        let mut er = EventRateF64::builder().alpha(0.3).build().unwrap();
        for i in 0..10 {
            er.update(i as f64 * 10.0).unwrap();
        }
        er.reset();
        assert_eq!(er.count(), 0);
        assert!(er.rate().is_none());
    }

    #[test]
    fn zero_interval_returns_none() {
        let mut er = EventRateF64::builder().alpha(0.3).build().unwrap();
        er.update(100.0).unwrap();
        er.update(100.0).unwrap(); // same timestamp -> interval = 0
        // rate() should return None (division by zero guard)
        assert!(
            er.rate().is_none(),
            "rate should be None with zero interval"
        );
    }

    #[test]
    fn errors_without_alpha() {
        let result = EventRateF64::builder().build();
        assert!(matches!(result, Err(crate::ConfigError::Missing("alpha"))));
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut er = EventRateF64::builder().alpha(0.3).build().unwrap();
        assert!(matches!(
            er.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            er.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
