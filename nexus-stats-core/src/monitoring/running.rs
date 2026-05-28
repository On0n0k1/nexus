/// All-time minimum tracker.
///
/// Tracks the smallest value ever seen. One comparison per update.
///
/// # Use Cases
/// - Best-case latency tracking (all-time min RTT)
/// - Low-water mark for prices or levels
/// - Input to range calculations (max - min)
#[derive(Debug, Clone)]
pub struct RunningMinF64 {
    min: f64,
    count: u64,
}

impl RunningMinF64 {
    /// Creates a new empty tracker.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min: f64::MAX,
            count: 0,
        }
    }

    /// Feeds a sample. Returns the current all-time minimum.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<f64, crate::DataError> {
        check_finite!(sample);
        self.count += 1;
        if sample < self.min {
            self.min = sample;
        }
        Ok(self.min)
    }

    /// All-time minimum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.min)
        }
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether at least one sample has been fed.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count > 0
    }

    /// Resets to empty state.
    #[inline]
    pub fn reset(&mut self) {
        self.min = f64::MAX;
        self.count = 0;
    }
}

impl Default for RunningMinF64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// All-time minimum tracker.
///
/// Tracks the smallest value ever seen. One comparison per update.
///
/// # Use Cases
/// - Best-case latency tracking (all-time min RTT)
/// - Low-water mark for prices or levels
/// - Input to range calculations (max - min)
#[derive(Debug, Clone)]
pub struct RunningMinI64 {
    min: i64,
    count: u64,
}

impl RunningMinI64 {
    /// Creates a new empty tracker.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            min: i64::MAX,
            count: 0,
        }
    }

    /// Feeds a sample. Returns the current all-time minimum.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> i64 {
        self.count += 1;
        if sample < self.min {
            self.min = sample;
        }
        self.min
    }

    /// All-time minimum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            Some(self.min)
        }
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether at least one sample has been fed.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count > 0
    }

    /// Resets to empty state.
    #[inline]
    pub fn reset(&mut self) {
        self.min = i64::MAX;
        self.count = 0;
    }
}

impl Default for RunningMinI64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// All-time maximum tracker.
///
/// Tracks the largest value ever seen. One comparison per update.
///
/// # Use Cases
/// - High-water mark tracking (peak throughput, max latency)
/// - Capacity planning (peak resource usage)
/// - Input to range calculations (max - min)
#[derive(Debug, Clone)]
pub struct RunningMaxF64 {
    max: f64,
    count: u64,
}

impl RunningMaxF64 {
    /// Creates a new empty tracker.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max: f64::MIN,
            count: 0,
        }
    }

    /// Feeds a sample. Returns the current all-time maximum.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sample is NaN, or
    /// `DataError::Infinite` if the sample is infinite.
    #[inline]
    pub fn update(&mut self, sample: f64) -> Result<f64, crate::DataError> {
        check_finite!(sample);
        self.count += 1;
        if sample > self.max {
            self.max = sample;
        }
        Ok(self.max)
    }

    /// All-time maximum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn max(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether at least one sample has been fed.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count > 0
    }

    /// Resets to empty state.
    #[inline]
    pub fn reset(&mut self) {
        self.max = f64::MIN;
        self.count = 0;
    }
}

impl Default for RunningMaxF64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// All-time maximum tracker.
///
/// Tracks the largest value ever seen. One comparison per update.
///
/// # Use Cases
/// - High-water mark tracking (peak throughput, max latency)
/// - Capacity planning (peak resource usage)
/// - Input to range calculations (max - min)
#[derive(Debug, Clone)]
pub struct RunningMaxI64 {
    max: i64,
    count: u64,
}

impl RunningMaxI64 {
    /// Creates a new empty tracker.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max: i64::MIN,
            count: 0,
        }
    }

    /// Feeds a sample. Returns the current all-time maximum.
    #[inline]
    #[must_use]
    pub fn update(&mut self, sample: i64) -> i64 {
        self.count += 1;
        if sample > self.max {
            self.max = sample;
        }
        self.max
    }

    /// All-time maximum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn max(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Whether at least one sample has been fed.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.count > 0
    }

    /// Resets to empty state.
    #[inline]
    pub fn reset(&mut self) {
        self.max = i64::MIN;
        self.count = 0;
    }
}

impl Default for RunningMaxI64 {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_empty() {
        let rm = RunningMinF64::new();
        assert!(rm.min().is_none());
        assert!(!rm.is_primed());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn min_tracks() {
        let mut rm = RunningMinF64::new();
        assert_eq!(rm.update(50.0).unwrap(), 50.0);
        assert_eq!(rm.update(30.0).unwrap(), 30.0);
        assert_eq!(rm.update(40.0).unwrap(), 30.0); // still 30
        assert_eq!(rm.update(10.0).unwrap(), 10.0);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn min_reset() {
        let mut rm = RunningMinF64::new();
        let _ = rm.update(10.0).unwrap();
        rm.reset();
        assert!(rm.min().is_none());
        assert_eq!(rm.update(50.0).unwrap(), 50.0);
    }

    #[test]
    fn min_i64() {
        let mut rm = RunningMinI64::new();
        assert_eq!(rm.update(100), 100);
        assert_eq!(rm.update(50), 50);
        assert_eq!(rm.update(75), 50);
    }

    #[test]
    fn min_default() {
        let rm = RunningMinF64::default();
        assert_eq!(rm.count(), 0);
    }

    #[test]
    fn max_empty() {
        let rm = RunningMaxF64::new();
        assert!(rm.max().is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn max_tracks() {
        let mut rm = RunningMaxF64::new();
        assert_eq!(rm.update(30.0).unwrap(), 30.0);
        assert_eq!(rm.update(50.0).unwrap(), 50.0);
        assert_eq!(rm.update(40.0).unwrap(), 50.0); // still 50
        assert_eq!(rm.update(90.0).unwrap(), 90.0);
    }

    #[test]
    fn max_i64() {
        let mut rm = RunningMaxI64::new();
        assert_eq!(rm.update(50), 50);
        assert_eq!(rm.update(100), 100);
        assert_eq!(rm.update(75), 100);
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut min64 = RunningMinF64::new();
        assert!(matches!(
            min64.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            min64.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
        assert!(matches!(
            min64.update(f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        ));

        let mut max64 = RunningMaxF64::new();
        assert!(matches!(
            max64.update(f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            max64.update(f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
