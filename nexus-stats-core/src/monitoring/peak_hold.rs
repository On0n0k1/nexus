/// Peak hold with decay — instant attack, configurable hold, exponential decay.
///
/// Captures peaks instantly, holds them for a configurable number of
/// samples, then decays exponentially.
///
/// # Use Cases
/// - VU meter / level indicator behavior
/// - Peak envelope tracking
/// - "What was the recent peak?" with graceful decay
#[derive(Debug, Clone)]
pub struct PeakHoldF64 {
    peak: f64,
    hold_samples: u64,
    decay_rate: f64,
    hold_remaining: u64,
    count: u64,
}

/// Builder for [`PeakHoldF64`].
#[derive(Debug, Clone)]
pub struct PeakHoldF64Builder {
    hold_samples: u64,
    decay_rate: Option<f64>,
}

impl PeakHoldF64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> PeakHoldF64Builder {
        PeakHoldF64Builder {
            hold_samples: 0,
            decay_rate: None,
        }
    }

    /// Feeds a sample. Returns the current envelope value.
    ///
    /// New peaks are captured instantly. During the hold period, the
    /// peak is maintained. After hold expires, the envelope decays
    /// multiplicatively each sample.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<f64, crate::DataError> {
        check_finite!(sample);
        self.count += 1;

        // Instant attack — new peak
        if sample >= self.peak {
            self.peak = sample;
            self.hold_remaining = self.hold_samples;
            return Ok(self.peak);
        }

        // Hold period
        if self.hold_remaining > 0 {
            self.hold_remaining -= 1;
            return Ok(self.peak);
        }

        // Decay
        self.peak *= self.decay_rate;

        // If sample is above decayed peak, capture it
        if sample > self.peak {
            self.peak = sample;
            self.hold_remaining = self.hold_samples;
        }

        Ok(self.peak)
    }

    /// Current envelope value.
    #[inline]
    #[must_use]
    pub fn peak(&self) -> f64 {
        self.peak
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets the envelope.
    #[inline]
    pub fn reset(&mut self) {
        self.peak = 0.0;
        self.hold_remaining = 0;
        self.count = 0;
    }
}

impl PeakHoldF64Builder {
    /// Number of samples to hold the peak before decaying. Default: 0.
    #[inline]
    #[must_use]
    pub fn hold_samples(mut self, n: u64) -> Self {
        self.hold_samples = n;
        self
    }

    /// Per-sample multiplicative decay rate (0 to 1). Default must be set.
    ///
    /// 0.99 = slow decay, 0.9 = fast decay.
    #[inline]
    #[must_use]
    pub fn decay_rate(mut self, rate: f64) -> Self {
        self.decay_rate = Some(rate);
        self
    }

    /// Builds the peak hold envelope.
    ///
    /// # Errors
    ///
    /// - decay_rate must have been set.
    /// - decay_rate must be in (0, 1].
    #[inline]
    pub fn build(self) -> Result<PeakHoldF64, crate::ConfigError> {
        let rate = self
            .decay_rate
            .ok_or(crate::ConfigError::Missing("decay_rate"))?;
        if !(rate > 0.0 && rate <= 1.0) {
            return Err(crate::ConfigError::Invalid("decay_rate must be in (0, 1]"));
        }

        Ok(PeakHoldF64 {
            peak: 0.0,
            hold_samples: self.hold_samples,
            decay_rate: rate,
            hold_remaining: 0,
            count: 0,
        })
    }
}

/// Peak hold (integer) — instant attack, configurable hold, no decay.
///
/// Integer variant tracks the peak during the hold window. After hold
/// expires, the peak resets to the current sample (no exponential decay
/// for integers — use the float variant for decay behavior).
#[derive(Debug, Clone)]
pub struct PeakHoldI64 {
    peak: i64,
    hold_samples: u64,
    hold_remaining: u64,
    count: u64,
}

/// Builder for [`PeakHoldI64`].
#[derive(Debug, Clone)]
pub struct PeakHoldI64Builder {
    hold_samples: u64,
}

impl PeakHoldI64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> PeakHoldI64Builder {
        PeakHoldI64Builder { hold_samples: 0 }
    }

    /// Feeds a sample. Returns the current peak.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> i64 {
        self.count += 1;

        if sample >= self.peak || self.count == 1 {
            self.peak = sample;
            self.hold_remaining = self.hold_samples;
            return self.peak;
        }

        if self.hold_remaining > 0 {
            self.hold_remaining -= 1;
            return self.peak;
        }

        // Hold expired — reset to current sample
        self.peak = sample;
        self.hold_remaining = self.hold_samples;
        self.peak
    }

    /// Current peak value.
    #[inline]
    #[must_use]
    pub fn peak(&self) -> i64 {
        self.peak
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets the peak.
    #[inline]
    pub fn reset(&mut self) {
        self.peak = 0;
        self.hold_remaining = 0;
        self.count = 0;
    }
}

impl PeakHoldI64Builder {
    /// Number of samples to hold the peak. Default: 0.
    #[inline]
    #[must_use]
    pub fn hold_samples(mut self, n: u64) -> Self {
        self.hold_samples = n;
        self
    }

    /// Builds the peak hold tracker.
    #[inline]
    pub fn build(self) -> Result<PeakHoldI64, crate::ConfigError> {
        Ok(PeakHoldI64 {
            peak: 0,
            hold_samples: self.hold_samples,
            hold_remaining: 0,
            count: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn instant_attack() {
        let mut ph = PeakHoldF64::builder()
            .decay_rate(0.95)
            .hold_samples(5)
            .build()
            .unwrap();
        assert_eq!(ph.update(50.0).unwrap(), 50.0);
        assert_eq!(ph.update(100.0).unwrap(), 100.0); // instant capture
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn hold_period() {
        let mut ph = PeakHoldF64::builder()
            .decay_rate(0.95)
            .hold_samples(3)
            .build()
            .unwrap();
        let _ = ph.update(100.0).unwrap();
        assert_eq!(ph.update(50.0).unwrap(), 100.0); // held
        assert_eq!(ph.update(50.0).unwrap(), 100.0); // held
        assert_eq!(ph.update(50.0).unwrap(), 100.0); // held (3rd hold sample)
    }

    #[test]
    fn decay_after_hold() {
        let mut ph = PeakHoldF64::builder()
            .decay_rate(0.9)
            .hold_samples(0)
            .build()
            .unwrap();
        let _ = ph.update(100.0).unwrap();
        let v = ph.update(0.0).unwrap(); // decay immediately (no hold)
        assert!(v < 100.0, "should have decayed, got {v}");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn new_peak_during_hold() {
        let mut ph = PeakHoldF64::builder()
            .decay_rate(0.95)
            .hold_samples(10)
            .build()
            .unwrap();
        let _ = ph.update(100.0).unwrap();
        let _ = ph.update(50.0).unwrap(); // holding at 100
        assert_eq!(ph.update(200.0).unwrap(), 200.0); // new peak resets hold
    }

    #[test]
    fn i64_hold() {
        let mut ph = PeakHoldI64::builder().hold_samples(3).build().unwrap();
        let _ = ph.update(100);
        assert_eq!(ph.update(50), 100); // held
        assert_eq!(ph.update(50), 100); // held
        assert_eq!(ph.update(50), 100); // held
        assert_eq!(ph.update(50), 50); // hold expired, reset to current
    }

    #[test]
    fn reset() {
        let mut ph = PeakHoldF64::builder().decay_rate(0.95).build().unwrap();
        let _ = ph.update(100.0).unwrap();
        ph.reset();
        assert_eq!(ph.count(), 0);
    }

    #[test]
    fn errors_without_decay_rate() {
        let result = PeakHoldF64::builder().build();
        assert!(matches!(
            result,
            Err(crate::ConfigError::Missing("decay_rate"))
        ));
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut ph = PeakHoldF64::builder().decay_rate(0.95).build().unwrap();
        assert!(matches!(
            ph.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            ph.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
        assert!(matches!(
            ph.update(f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
