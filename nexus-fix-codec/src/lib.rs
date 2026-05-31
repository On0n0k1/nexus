//! Zero-copy FIX protocol parsing primitives with SIMD acceleration.
//!
//! Provides the core building blocks for FIX message parsing:
//! - SOH and `=` delimiter scanning (SWAR + SSE2 + AVX2 + AVX-512)
//! - [`DelimiterScanner`] iterator with SIMD mask caching
//! - [`FieldSpan`] for zero-copy field access
//!
//! Generated FIX decoders (from `nexus-fix-codegen`) depend on these primitives.

mod span;

pub mod scan;

pub use scan::DelimiterScanner;
pub use span::FieldSpan;
