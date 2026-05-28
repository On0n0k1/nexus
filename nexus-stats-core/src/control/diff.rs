/// First difference — `x[n] - x[n-1]`.
///
/// Returns the change between consecutive samples.
/// `None` on the first sample (no previous value to diff against).
///
/// # Use Cases
/// - Computing returns from prices
/// - Velocity from position
/// - Rate of change of any signal
#[derive(Debug, Clone)]
pub struct FirstDiffF64 {
    prev: f64,
    initialized: bool,
}

impl FirstDiffF64 {
    /// Creates a new first-difference filter.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prev: 0.0,
            initialized: false,
        }
    }

    /// Feeds a sample. Returns `Ok(Some(x[n] - x[n-1]))` or `Ok(None)` on first sample.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<Option<f64>, crate::DataError> {
        check_finite!(sample);
        if !self.initialized {
            self.prev = sample;
            self.initialized = true;
            return Ok(Option::None);
        }
        let diff = sample - self.prev;
        self.prev = sample;
        Ok(Option::Some(diff))
    }

    /// Resets to uninitialized state.
    #[inline]
    pub fn reset(&mut self) {
        self.prev = 0.0;
        self.initialized = false;
    }
}

impl Default for FirstDiffF64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// First difference — `x[n] - x[n-1]`.
#[derive(Debug, Clone)]
pub struct FirstDiffI64 {
    prev: i64,
    initialized: bool,
}

impl FirstDiffI64 {
    /// Creates a new first-difference filter.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prev: 0,
            initialized: false,
        }
    }

    /// Feeds a sample. Returns `Some(x[n] - x[n-1])` or `None` on first sample.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> Option<i64> {
        if !self.initialized {
            self.prev = sample;
            self.initialized = true;
            return Option::None;
        }
        let diff = sample - self.prev;
        self.prev = sample;
        Option::Some(diff)
    }

    /// Resets to uninitialized state.
    #[inline]
    pub fn reset(&mut self) {
        self.prev = 0;
        self.initialized = false;
    }
}

impl Default for FirstDiffI64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Second difference — `x[n] - 2*x[n-1] + x[n-2]`.
///
/// Returns the acceleration (change in change) of a signal.
/// `None` until the third sample.
///
/// # Use Cases
/// - Acceleration from position
/// - Curvature detection
/// - "Is the rate of change itself changing?"
#[derive(Debug, Clone)]
pub struct SecondDiffF64 {
    prev2: f64,
    prev1: f64,
    count: u64,
}

impl SecondDiffF64 {
    /// Creates a new second-difference filter.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prev2: 0.0,
            prev1: 0.0,
            count: 0,
        }
    }

    /// Feeds a sample. Returns `Ok(Some(x[n] - 2*x[n-1] + x[n-2]))` or `Ok(None)`
    /// until 3 samples have been fed.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    #[allow(clippy::suboptimal_flops)]
    pub fn update(&mut self, sample: f64) -> Result<Option<f64>, crate::DataError> {
        check_finite!(sample);
        self.count += 1;

        if self.count == 1 {
            self.prev1 = sample;
            return Ok(Option::None);
        }
        if self.count == 2 {
            self.prev2 = self.prev1;
            self.prev1 = sample;
            return Ok(Option::None);
        }

        let diff2 = sample - 2.0 * self.prev1 + self.prev2;
        self.prev2 = self.prev1;
        self.prev1 = sample;
        Ok(Option::Some(diff2))
    }

    /// Resets to uninitialized state.
    #[inline]
    pub fn reset(&mut self) {
        self.prev2 = 0.0;
        self.prev1 = 0.0;
        self.count = 0;
    }
}

impl Default for SecondDiffF64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Second difference — `x[n] - 2*x[n-1] + x[n-2]`.
#[derive(Debug, Clone)]
pub struct SecondDiffI64 {
    prev2: i64,
    prev1: i64,
    count: u64,
}

impl SecondDiffI64 {
    /// Creates a new second-difference filter.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            prev2: 0,
            prev1: 0,
            count: 0,
        }
    }

    /// Feeds a sample. Returns `Some(x[n] - 2*x[n-1] + x[n-2])` or `None`
    /// until 3 samples have been fed.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> Option<i64> {
        self.count += 1;

        if self.count == 1 {
            self.prev1 = sample;
            return Option::None;
        }
        if self.count == 2 {
            self.prev2 = self.prev1;
            self.prev1 = sample;
            return Option::None;
        }

        let diff2 = sample - 2 * self.prev1 + self.prev2;
        self.prev2 = self.prev1;
        self.prev1 = sample;
        Option::Some(diff2)
    }

    /// Resets to uninitialized state.
    #[inline]
    pub fn reset(&mut self) {
        self.prev2 = 0;
        self.prev1 = 0;
        self.count = 0;
    }
}

impl Default for SecondDiffI64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // First diff
    #[test]
    fn first_diff_none_on_first() {
        let mut fd = FirstDiffF64::new();
        assert!(fd.update(100.0).unwrap().is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn first_diff_computes() {
        let mut fd = FirstDiffF64::new();
        let _ = fd.update(100.0).unwrap();
        assert_eq!(fd.update(110.0).unwrap(), Some(10.0));
        assert_eq!(fd.update(105.0).unwrap(), Some(-5.0));
    }

    #[test]
    fn first_diff_i64() {
        let mut fd = FirstDiffI64::new();
        let _ = fd.update(100);
        assert_eq!(fd.update(130), Some(30));
    }

    #[test]
    fn first_diff_reset() {
        let mut fd = FirstDiffF64::new();
        let _ = fd.update(100.0).unwrap();
        fd.reset();
        assert!(fd.update(50.0).unwrap().is_none()); // re-initialized
    }

    // Second diff
    #[test]
    fn second_diff_none_until_third() {
        let mut sd = SecondDiffF64::new();
        assert!(sd.update(1.0).unwrap().is_none());
        assert!(sd.update(2.0).unwrap().is_none());
        assert!(sd.update(3.0).unwrap().is_some());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn second_diff_linear_is_zero() {
        let mut sd = SecondDiffF64::new();
        let _ = sd.update(10.0).unwrap();
        let _ = sd.update(20.0).unwrap();
        // Linear: 10, 20, 30 → second diff = 30 - 2*20 + 10 = 0
        assert_eq!(sd.update(30.0).unwrap(), Some(0.0));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn second_diff_quadratic() {
        let mut sd = SecondDiffF64::new();
        // x^2: 1, 4, 9 → 9 - 2*4 + 1 = 2
        let _ = sd.update(1.0).unwrap();
        let _ = sd.update(4.0).unwrap();
        assert_eq!(sd.update(9.0).unwrap(), Some(2.0));
    }

    #[test]
    fn second_diff_i64() {
        let mut sd = SecondDiffI64::new();
        let _ = sd.update(10);
        let _ = sd.update(20);
        assert_eq!(sd.update(30), Some(0)); // linear
        assert_eq!(sd.update(50), Some(10)); // acceleration
    }

    #[test]
    fn second_diff_reset() {
        let mut sd = SecondDiffF64::new();
        let _ = sd.update(1.0).unwrap();
        let _ = sd.update(2.0).unwrap();
        sd.reset();
        assert!(sd.update(5.0).unwrap().is_none());
    }

    #[test]
    fn first_diff_rejects_nan_and_inf() {
        let mut fd = FirstDiffF64::new();
        assert_eq!(fd.update(f64::NAN), Err(crate::DataError::NotANumber));
        assert_eq!(fd.update(f64::INFINITY), Err(crate::DataError::Infinite));
        assert_eq!(
            fd.update(f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        );
    }

    #[test]
    fn second_diff_rejects_nan_and_inf() {
        let mut sd = SecondDiffF64::new();
        assert_eq!(sd.update(f64::NAN), Err(crate::DataError::NotANumber));
        assert_eq!(sd.update(f64::INFINITY), Err(crate::DataError::Infinite));
    }
}
