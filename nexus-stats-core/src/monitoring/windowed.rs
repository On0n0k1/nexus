// Windowed extrema using Nichols' 3-sample sub-window promotion algorithm.
//
// Ported from the Linux kernel's `win_minmax.h` (used by TCP BBR).
// Maintains the window extremum using only 3 stored samples, each covering
// a sub-window of `window/3` ticks. When a sub-window expires, the next
// candidate is promoted.
//
// State: 3 × (timestamp, value) + u64 window.

/// Internal sample stored per sub-window.
#[derive(Debug, Clone, Copy)]
struct Sample<T: Copy> {
    timestamp: u64,
    value: T,
}

/// Streaming windowed maximum over a sliding time window (Nichols' algorithm).
///
/// Tracks the maximum value within a `u64` timestamp window using only
/// 3 stored samples. O(1) amortized per update, `no_std` compatible.
///
/// # Use Cases
/// - Max throughput tracking
/// - BBR-style bandwidth estimation
/// - Peak detection within a time window
#[derive(Debug, Clone)]
pub struct WindowedMaxF64 {
    window: u64,
    samples: [Sample<f64>; 3],
    count: u64,
}

impl WindowedMaxF64 {
    /// Creates a new windowed max tracker.
    ///
    /// `window` is in the same units as the timestamps you will pass
    /// to [`update`](Self::update). Must be positive.
    #[inline]
    pub fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: f64::MIN,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    /// Feeds a sample at the given timestamp. Returns current window max.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the value is NaN, or
    /// `DataError::Infinite` if the value is infinite.
    #[inline]
    pub fn update(&mut self, timestamp: u64, value: f64) -> Result<f64, crate::DataError> {
        check_finite!(value);
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value >= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return Ok(s[0].value);
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value >= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value >= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        Ok(s[0].value)
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    pub fn update_i64(&mut self, timestamp: i64, value: f64) -> Result<f64, crate::DataError> {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, value)
    }

    /// Current window maximum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn max(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    /// Window size in raw units.
    #[inline]
    #[must_use]
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets to empty state. Window size is preserved.
    #[inline]
    pub fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: f64::MIN,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

/// Streaming windowed maximum over a sliding time window (Nichols' algorithm).
///
/// Tracks the maximum value within a `u64` timestamp window using only
/// 3 stored samples. O(1) amortized per update, `no_std` compatible.
///
/// # Use Cases
/// - Max throughput tracking
/// - BBR-style bandwidth estimation
/// - Peak detection within a time window
/// - Deterministic replay with raw tick counters
#[derive(Debug, Clone)]
pub struct WindowedMaxI64 {
    window: u64,
    samples: [Sample<i64>; 3],
    count: u64,
}

impl WindowedMaxI64 {
    /// Creates a new windowed max tracker.
    ///
    /// `window` is in the same units as the timestamps you will pass
    /// to [`update`](Self::update). Must be positive.
    #[inline]
    pub fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: i64::MIN,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    /// Feeds a sample at the given timestamp. Returns current window max.
    #[inline]
    #[must_use]
    pub fn update(&mut self, timestamp: u64, value: i64) -> i64 {
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value >= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return s[0].value;
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value >= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value >= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        s[0].value
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    #[must_use]
    pub fn update_i64(&mut self, timestamp: i64, value: i64) -> i64 {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, value)
    }

    /// Current window maximum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn max(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    /// Window size in raw units.
    #[inline]
    #[must_use]
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets to empty state. Window size is preserved.
    #[inline]
    pub fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: i64::MIN,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

/// Streaming windowed minimum over a sliding time window (Nichols' algorithm).
///
/// Tracks the minimum value within a `u64` timestamp window using only
/// 3 stored samples. O(1) amortized per update, `no_std` compatible.
///
/// # Use Cases
/// - Min RTT tracking (BBR)
/// - Minimum price in a window
/// - Best-case latency estimation
/// - Deterministic replay with raw tick counters
#[derive(Debug, Clone)]
pub struct WindowedMinF64 {
    window: u64,
    samples: [Sample<f64>; 3],
    count: u64,
}

impl WindowedMinF64 {
    /// Creates a new windowed min tracker.
    ///
    /// `window` is in the same units as the timestamps you will pass
    /// to [`update`](Self::update). Must be positive.
    #[inline]
    pub fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: f64::MAX,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    /// Feeds a sample at the given timestamp. Returns current window min.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the value is NaN, or
    /// `DataError::Infinite` if the value is infinite.
    #[inline]
    pub fn update(&mut self, timestamp: u64, value: f64) -> Result<f64, crate::DataError> {
        check_finite!(value);
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value <= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return Ok(s[0].value);
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value <= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value <= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        Ok(s[0].value)
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    pub fn update_i64(&mut self, timestamp: i64, value: f64) -> Result<f64, crate::DataError> {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, value)
    }

    /// Current window minimum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    /// Window size in raw units.
    #[inline]
    #[must_use]
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets to empty state. Window size is preserved.
    #[inline]
    pub fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: f64::MAX,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

/// Streaming windowed minimum over a sliding time window (Nichols' algorithm).
///
/// Tracks the minimum value within a `u64` timestamp window using only
/// 3 stored samples. O(1) amortized per update, `no_std` compatible.
///
/// # Use Cases
/// - Min RTT tracking (BBR)
/// - Minimum price in a window
/// - Best-case latency estimation
/// - Deterministic replay with raw tick counters
#[derive(Debug, Clone)]
pub struct WindowedMinI64 {
    window: u64,
    samples: [Sample<i64>; 3],
    count: u64,
}

impl WindowedMinI64 {
    /// Creates a new windowed min tracker.
    ///
    /// `window` is in the same units as the timestamps you will pass
    /// to [`update`](Self::update). Must be positive.
    #[inline]
    pub fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: i64::MAX,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    /// Feeds a sample at the given timestamp. Returns current window min.
    #[inline]
    #[must_use]
    pub fn update(&mut self, timestamp: u64, value: i64) -> i64 {
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value <= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return s[0].value;
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value <= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value <= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        s[0].value
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    #[must_use]
    pub fn update_i64(&mut self, timestamp: i64, value: i64) -> i64 {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, value)
    }

    /// Current window minimum, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min(&self) -> Option<i64> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    /// Window size in raw units.
    #[inline]
    #[must_use]
    pub fn window(&self) -> u64 {
        self.window
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Resets to empty state. Window size is preserved.
    #[inline]
    pub fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: i64::MAX,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

// --- pub(crate) f32 variants for normalizers ---

#[derive(Debug, Clone)]
pub(crate) struct WindowedMaxF32 {
    window: u64,
    samples: [Sample<f32>; 3],
    count: u64,
}

impl WindowedMaxF32 {
    #[inline]
    pub(crate) fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: f32::MIN,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    #[inline]
    pub(crate) fn update(&mut self, timestamp: u64, value: f32) -> Result<f32, crate::DataError> {
        check_finite!(value);
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value >= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return Ok(s[0].value);
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value >= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value >= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        Ok(s[0].value)
    }

    #[inline]
    pub(crate) fn max(&self) -> Option<f32> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    #[inline]
    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    #[inline]
    pub(crate) fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: f32::MIN,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

#[derive(Debug, Clone)]
pub(crate) struct WindowedMinF32 {
    window: u64,
    samples: [Sample<f32>; 3],
    count: u64,
}

impl WindowedMinF32 {
    #[inline]
    pub(crate) fn new(window: u64) -> Result<Self, crate::ConfigError> {
        if window == 0 {
            return Err(crate::ConfigError::Invalid("window must be positive"));
        }
        let init = Sample {
            timestamp: 0,
            value: f32::MAX,
        };
        Ok(Self {
            window,
            samples: [init; 3],
            count: 0,
        })
    }

    #[inline]
    pub(crate) fn update(&mut self, timestamp: u64, value: f32) -> Result<f32, crate::DataError> {
        check_finite!(value);
        self.count += 1;
        let win = self.window;
        let s = &mut self.samples;

        if value <= s[0].value || timestamp.wrapping_sub(s[2].timestamp) > win {
            s[0] = Sample { timestamp, value };
            s[1] = s[0];
            s[2] = s[0];
            return Ok(s[0].value);
        }

        if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if timestamp.wrapping_sub(s[2].timestamp) > win / 3 {
            s[2] = Sample { timestamp, value };
        }

        if value <= s[1].value {
            s[1] = Sample { timestamp, value };
            s[2] = s[1];
        } else if value <= s[2].value {
            s[2] = Sample { timestamp, value };
        }

        if timestamp.wrapping_sub(s[0].timestamp) > win {
            s[0] = s[1];
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        } else if timestamp.wrapping_sub(s[1].timestamp) > win / 3 {
            s[1] = s[2];
            s[2] = Sample { timestamp, value };
        }

        Ok(s[0].value)
    }

    #[inline]
    pub(crate) fn min(&self) -> Option<f32> {
        if self.count == 0 {
            None
        } else {
            Some(self.samples[0].value)
        }
    }

    #[inline]
    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    #[inline]
    pub(crate) fn reset(&mut self) {
        let init = Sample {
            timestamp: 0,
            value: f32::MAX,
        };
        self.samples = [init; 3];
        self.count = 0;
    }
}

#[cfg(test)]
mod raw_tests {
    use super::*;

    #[test]
    fn raw_max_basic() {
        let mut wm = WindowedMaxF64::new(100).unwrap();
        assert_eq!(wm.update(0, 10.0).unwrap(), 10.0);
        assert_eq!(wm.update(50, 20.0).unwrap(), 20.0);
    }

    #[test]
    fn raw_max_expires() {
        let mut wm = WindowedMaxF64::new(10).unwrap();
        let _ = wm.update(0, 100.0).unwrap();
        let _ = wm.update(5, 50.0).unwrap();
        let result = wm.update(11, 60.0).unwrap();
        assert!(result <= 60.0);
    }

    #[test]
    fn raw_min_basic() {
        let mut wm = WindowedMinI64::new(100).unwrap();
        assert_eq!(wm.update(0, 100), 100);
        assert_eq!(wm.update(1, 50), 50);
    }

    #[test]
    fn raw_max_i64_convenience() {
        let mut wm = WindowedMaxF64::new(1000).unwrap();
        assert_eq!(wm.update_i64(100i64, 42.0).unwrap(), 42.0);
    }

    #[test]
    fn raw_rejects_zero_window() {
        assert!(WindowedMaxF64::new(0).is_err());
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut wm = WindowedMaxF64::new(100).unwrap();
        assert!(matches!(
            wm.update(0, f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            wm.update(0, f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));

        let mut wn = WindowedMinF64::new(100).unwrap();
        assert!(matches!(
            wn.update(0, f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            wn.update(0, f64::NEG_INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
