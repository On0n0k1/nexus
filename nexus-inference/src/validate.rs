//! Shared validation steps for model constructors.
//!
//! These keep each `from_parts` readable as a sequence of named checks while
//! letting every call site supply its own error message, so messages stay
//! specific to the model being loaded.

use crate::LoadError;

/// Error with `msg` if any dimension is zero.
pub(crate) fn require_nonzero(dims: &[usize], msg: &'static str) -> Result<(), LoadError> {
    if dims.contains(&0) {
        return Err(LoadError::Validation(msg));
    }
    Ok(())
}

/// Error with `msg` if any dimension exceeds `u16::MAX`, the packed-size limit.
pub(crate) fn require_u16(dims: &[usize], msg: &'static str) -> Result<(), LoadError> {
    if dims.iter().any(|&d| d > u16::MAX as usize) {
        return Err(LoadError::Validation(msg));
    }
    Ok(())
}

/// Error with `msg` if any value is non-finite (NaN or infinite).
pub(crate) fn require_all_finite(
    values: impl IntoIterator<Item = f32>,
    msg: &'static str,
) -> Result<(), LoadError> {
    if values.into_iter().any(|v| !v.is_finite()) {
        return Err(LoadError::Validation(msg));
    }
    Ok(())
}
