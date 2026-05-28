/// Max gauge — tracks the maximum since last take.
///
/// `take()` returns the maximum and resets the gauge. Useful for
/// periodic reporting ("what was the peak since last report?").
///
/// # Use Cases
/// - Peak latency per reporting interval
/// - High-water mark gauges (Prometheus-style)
/// - Periodic max collection
#[derive(Debug, Clone)]
pub struct MaxGaugeF64 {
    max: f64,
    has_value: bool,
}

impl MaxGaugeF64 {
    /// Creates a new empty gauge.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max: f64::MIN,
            has_value: false,
        }
    }

    /// Records a sample.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<(), crate::DataError> {
        check_finite!(sample);
        if !self.has_value || sample > self.max {
            self.max = sample;
        }
        self.has_value = true;
        Ok(())
    }

    /// Returns the max since last take/reset, and resets the gauge.
    #[inline]
    pub fn take(&mut self) -> Option<f64> {
        if self.has_value {
            let val = self.max;
            self.max = f64::MIN;
            self.has_value = false;
            Some(val)
        } else {
            None
        }
    }

    /// Peeks at the current max without resetting.
    #[inline]
    #[must_use]
    pub fn peek(&self) -> Option<f64> {
        if self.has_value { Some(self.max) } else { None }
    }

    /// Resets the gauge.
    #[inline]
    pub fn reset(&mut self) {
        self.max = f64::MIN;
        self.has_value = false;
    }
}

impl Default for MaxGaugeF64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Max gauge — tracks the maximum since last take.
///
/// `take()` returns the maximum and resets the gauge. Useful for
/// periodic reporting ("what was the peak since last report?").
///
/// # Use Cases
/// - Peak latency per reporting interval
/// - High-water mark gauges (Prometheus-style)
/// - Periodic max collection
#[derive(Debug, Clone)]
pub struct MaxGaugeI64 {
    max: i64,
    has_value: bool,
}

impl MaxGaugeI64 {
    /// Creates a new empty gauge.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max: i64::MIN,
            has_value: false,
        }
    }

    /// Records a sample.
    #[inline]
    pub fn update(&mut self, sample: i64) {
        if !self.has_value || sample > self.max {
            self.max = sample;
        }
        self.has_value = true;
    }

    /// Returns the max since last take/reset, and resets the gauge.
    #[inline]
    pub fn take(&mut self) -> Option<i64> {
        if self.has_value {
            let val = self.max;
            self.max = i64::MIN;
            self.has_value = false;
            Some(val)
        } else {
            None
        }
    }

    /// Peeks at the current max without resetting.
    #[inline]
    #[must_use]
    pub fn peek(&self) -> Option<i64> {
        if self.has_value { Some(self.max) } else { None }
    }

    /// Resets the gauge.
    #[inline]
    pub fn reset(&mut self) {
        self.max = i64::MIN;
        self.has_value = false;
    }
}

impl Default for MaxGaugeI64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let mut g = MaxGaugeF64::new();
        assert!(g.peek().is_none());
        assert!(g.take().is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn tracks_max() {
        let mut g = MaxGaugeF64::new();
        g.update(10.0).unwrap();
        g.update(50.0).unwrap();
        g.update(30.0).unwrap();
        assert_eq!(g.peek(), Some(50.0));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn take_returns_and_resets() {
        let mut g = MaxGaugeF64::new();
        g.update(50.0).unwrap();
        assert_eq!(g.take(), Some(50.0));
        assert!(g.take().is_none()); // already taken

        g.update(20.0).unwrap();
        assert_eq!(g.take(), Some(20.0));
    }

    #[test]
    fn i64_basic() {
        let mut g = MaxGaugeI64::new();
        g.update(100);
        g.update(200);
        assert_eq!(g.take(), Some(200));
    }

    #[test]
    fn reset() {
        let mut g = MaxGaugeF64::new();
        g.update(100.0).unwrap();
        g.reset();
        assert!(g.peek().is_none());
    }

    #[test]
    fn default_is_empty() {
        let g = MaxGaugeI64::default();
        assert!(g.peek().is_none());
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut g = MaxGaugeF64::new();
        assert!(matches!(
            g.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            g.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
        assert!(matches!(
            g.update(f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
