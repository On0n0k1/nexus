//! Error types for decimal operations.
//!
//! Each error type is scoped to the operations that can produce it.
//! `checked_*` methods return `Option` (std convention).
//! `try_*` and fallible constructors return `Result` with the
//! narrowest error type for that operation.
//!
//! ## Error shape convention
//!
//! Error shapes in this crate match the number of distinct runtime states:
//!
//! - **Single-state errors are unit structs** (e.g., [`OverflowError`]).
//!   Match arms are `Err(OverflowError)`, no variant pattern needed.
//! - **Multi-state errors are enums** (e.g., [`DivError`], [`ParseError`]).
//!   Each variant represents an actually-distinct runtime outcome.
//!
//! When adding a new error type, pick the shape that matches its actual
//! runtime states. Don't make a unit struct into an enum "in case we add
//! variants later" — bump the type when that case actually arrives.

use core::fmt;

/// Arithmetic overflow (add, sub, mul, neg).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverflowError;

impl fmt::Display for OverflowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("decimal overflow")
    }
}

/// Division failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DivError {
    /// Result exceeds representable range.
    Overflow,
    /// Divisor is zero.
    DivisionByZero,
}

impl fmt::Display for DivError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => f.write_str("decimal division overflow"),
            Self::DivisionByZero => f.write_str("division by zero"),
        }
    }
}

/// String parsing failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    /// Input is not a valid decimal string.
    InvalidFormat,
    /// Parsed value exceeds representable range.
    Overflow,
    /// Input has more decimal places than the type supports.
    PrecisionLoss,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => f.write_str("invalid decimal format"),
            Self::Overflow => f.write_str("decimal parse overflow"),
            Self::PrecisionLoss => f.write_str("precision loss in decimal parse"),
        }
    }
}

/// Type or float conversion failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertError {
    /// Value exceeds target type's representable range.
    Overflow,
    /// Conversion would lose precision.
    PrecisionLoss,
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => f.write_str("decimal conversion overflow"),
            Self::PrecisionLoss => f.write_str("precision loss in decimal conversion"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for OverflowError {}

#[cfg(feature = "std")]
impl std::error::Error for DivError {}

#[cfg(feature = "std")]
impl std::error::Error for ParseError {}

#[cfg(feature = "std")]
impl std::error::Error for ConvertError {}
