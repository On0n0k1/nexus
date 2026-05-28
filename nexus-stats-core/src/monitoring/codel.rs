use super::windowed::{WindowedMinF64, WindowedMinI64};
use crate::Condition;

// =========================================================================
// Raw (u64 timestamp) variants — no_std compatible
// =========================================================================

/// CoDel — Controlled Delay queue monitor (Nichols & Jacobson, 2012).
///
/// Composes a windowed minimum of sojourn times with a threshold.
/// Reports `Degraded` when even the minimum sojourn time in the
/// observation window exceeds the target. `no_std` compatible.
#[derive(Debug, Clone)]
pub struct CoDelI64 {
    windowed_min: WindowedMinI64,
    target: i64,
    min_samples: u64,
}

/// Builder for [`CoDelI64`].
#[derive(Debug, Clone)]
pub struct CoDelI64Builder {
    target: Option<i64>,
    window: Option<u64>,
    min_samples: u64,
}

impl CoDelI64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> CoDelI64Builder {
        CoDelI64Builder {
            target: None,
            window: None,
            min_samples: 1,
        }
    }

    /// Feeds a sojourn time at the given timestamp.
    ///
    /// Returns `Some(Condition)` once primed, `None` before.
    #[inline]
    #[must_use]
    pub fn update(&mut self, timestamp: u64, sojourn: i64) -> Option<Condition> {
        let min = self.windowed_min.update(timestamp, sojourn);

        if self.windowed_min.count() < self.min_samples {
            return None;
        }

        if min > self.target {
            Some(Condition::Degraded)
        } else {
            Some(Condition::Normal)
        }
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    #[must_use]
    pub fn update_i64(&mut self, timestamp: i64, sojourn: i64) -> Option<Condition> {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, sojourn)
    }

    /// Current windowed minimum sojourn time, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min_sojourn(&self) -> Option<i64> {
        self.windowed_min.min()
    }

    /// Whether the queue is currently elevated.
    #[inline]
    #[must_use]
    pub fn is_elevated(&self) -> bool {
        self.windowed_min.min().is_some_and(|min| min > self.target)
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.windowed_min.count()
    }

    /// Whether the monitor has reached `min_samples`.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.windowed_min.count() >= self.min_samples
    }

    /// Resets to empty state. Parameters unchanged.
    #[inline]
    pub fn reset(&mut self) {
        self.windowed_min.reset();
    }
}

impl CoDelI64Builder {
    /// Target sojourn time. Elevated when minimum exceeds this.
    #[inline]
    #[must_use]
    pub fn target(mut self, target: i64) -> Self {
        self.target = Some(target);
        self
    }

    /// Observation window in raw units (same as timestamps).
    #[inline]
    #[must_use]
    pub fn window(mut self, window: u64) -> Self {
        self.window = Some(window);
        self
    }

    /// Minimum samples before monitoring activates. Default: 1.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Builds the CoDel monitor.
    ///
    /// # Errors
    ///
    /// - Target must have been set.
    /// - Window must have been set and be positive.
    #[inline]
    pub fn build(self) -> Result<CoDelI64, crate::ConfigError> {
        let target = self.target.ok_or(crate::ConfigError::Missing("target"))?;
        let window = self.window.ok_or(crate::ConfigError::Missing("window"))?;
        if window == 0 {
            return Err(crate::ConfigError::Invalid("CoDel window must be positive"));
        }

        Ok(CoDelI64 {
            windowed_min: WindowedMinI64::new(window)?,
            target,
            min_samples: self.min_samples,
        })
    }
}

/// CoDel — Controlled Delay queue monitor (Nichols & Jacobson, 2012).
///
/// Composes a windowed minimum of sojourn times with a threshold.
/// Reports `Degraded` when even the minimum sojourn time in the
/// observation window exceeds the target. `no_std` compatible.
#[derive(Debug, Clone)]
pub struct CoDelF64 {
    windowed_min: WindowedMinF64,
    target: f64,
    min_samples: u64,
}

/// Builder for [`CoDelF64`].
#[derive(Debug, Clone)]
pub struct CoDelF64Builder {
    target: Option<f64>,
    window: Option<u64>,
    min_samples: u64,
}

impl CoDelF64 {
    /// Creates a builder.
    #[inline]
    #[must_use]
    pub fn builder() -> CoDelF64Builder {
        CoDelF64Builder {
            target: None,
            window: None,
            min_samples: 1,
        }
    }

    /// Feeds a sojourn time at the given timestamp.
    ///
    /// Returns `Some(Condition)` once primed, `None` before.
    ///
    /// # Errors
    ///
    /// Returns `DataError::NotANumber` if the sojourn is NaN, or
    /// `DataError::Infinite` if the sojourn is infinite.
    #[inline]
    pub fn update(
        &mut self,
        timestamp: u64,
        sojourn: f64,
    ) -> Result<Option<Condition>, crate::DataError> {
        check_finite!(sojourn);
        let min = self.windowed_min.update(timestamp, sojourn)?;

        if self.windowed_min.count() < self.min_samples {
            return Ok(None);
        }

        if min > self.target {
            Ok(Some(Condition::Degraded))
        } else {
            Ok(Some(Condition::Normal))
        }
    }

    /// Convenience for `i64` timestamps (e.g., wire protocol epoch nanos).
    ///
    /// Timestamps must be non-negative. Negative values wrap to large
    /// `u64` values and will produce incorrect window expiration.
    #[inline]
    pub fn update_i64(
        &mut self,
        timestamp: i64,
        sojourn: f64,
    ) -> Result<Option<Condition>, crate::DataError> {
        debug_assert!(timestamp >= 0, "negative timestamp: {timestamp}");
        self.update(timestamp as u64, sojourn)
    }

    /// Current windowed minimum sojourn time, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn min_sojourn(&self) -> Option<f64> {
        self.windowed_min.min()
    }

    /// Whether the queue is currently elevated.
    #[inline]
    #[must_use]
    pub fn is_elevated(&self) -> bool {
        self.windowed_min.min().is_some_and(|min| min > self.target)
    }

    /// Number of samples processed.
    #[inline]
    #[must_use]
    pub fn count(&self) -> u64 {
        self.windowed_min.count()
    }

    /// Whether the monitor has reached `min_samples`.
    #[inline]
    #[must_use]
    pub fn is_primed(&self) -> bool {
        self.windowed_min.count() >= self.min_samples
    }

    /// Resets to empty state. Parameters unchanged.
    #[inline]
    pub fn reset(&mut self) {
        self.windowed_min.reset();
    }
}

impl CoDelF64Builder {
    /// Target sojourn time. Elevated when minimum exceeds this.
    #[inline]
    #[must_use]
    pub fn target(mut self, target: f64) -> Self {
        self.target = Some(target);
        self
    }

    /// Observation window in raw units (same as timestamps).
    #[inline]
    #[must_use]
    pub fn window(mut self, window: u64) -> Self {
        self.window = Some(window);
        self
    }

    /// Minimum samples before monitoring activates. Default: 1.
    #[inline]
    #[must_use]
    pub fn min_samples(mut self, min: u64) -> Self {
        self.min_samples = min;
        self
    }

    /// Builds the CoDel monitor.
    ///
    /// # Errors
    ///
    /// - Target must have been set.
    /// - Window must have been set and be positive.
    #[inline]
    pub fn build(self) -> Result<CoDelF64, crate::ConfigError> {
        let target = self.target.ok_or(crate::ConfigError::Missing("target"))?;
        let window = self.window.ok_or(crate::ConfigError::Missing("window"))?;
        if window == 0 {
            return Err(crate::ConfigError::Invalid("CoDel window must be positive"));
        }

        Ok(CoDelF64 {
            windowed_min: WindowedMinF64::new(window)?,
            target,
            min_samples: self.min_samples,
        })
    }
}

#[cfg(test)]
mod raw_tests {
    use super::*;
    use crate::Condition;

    #[test]
    fn raw_codel_normal() {
        let mut cd = CoDelI64::builder()
            .target(100)
            .window(1000)
            .build()
            .unwrap();
        assert_eq!(cd.update(0, 50), Some(Condition::Normal));
    }

    #[test]
    fn raw_codel_degraded() {
        let mut cd = CoDelI64::builder().target(50).window(1000).build().unwrap();
        for t in 0..10 {
            let _ = cd.update(t * 100, 200);
        }
        assert!(cd.is_elevated());
    }

    #[test]
    fn raw_codel_f64() {
        let mut cd = CoDelF64::builder()
            .target(0.5)
            .window(1000)
            .build()
            .unwrap();
        assert_eq!(cd.update(0, 0.1).unwrap(), Some(Condition::Normal));
    }

    #[test]
    fn rejects_nan_and_inf() {
        let mut cd = CoDelF64::builder()
            .target(0.5)
            .window(1000)
            .build()
            .unwrap();
        assert!(matches!(
            cd.update(0, f64::NAN),
            Err(crate::DataError::NotANumber)
        ));
        assert!(matches!(
            cd.update(0, f64::INFINITY),
            Err(crate::DataError::Infinite)
        ));
    }
}
