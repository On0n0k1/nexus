//! Zero-copy FIX protocol reading and writing with SIMD acceleration.
//!
//! Provides the core building blocks for FIX message handling:
//! - SOH and `=` delimiter scanning (SWAR + SSE2 + AVX2 + AVX-512)
//! - [`DelimiterScanner`] iterator with SIMD mask caching
//! - [`FieldReader`] with fused PSADBW checksum accumulation
//! - [`FieldWriter`] for writing `tag=value` fields into a buffer
//! - [`FieldSpan`] / [`GroupSpan`] for zero-copy field access
//! - [`validate_checksum`] for FIX checksum verification
//!
//! Generated FIX codecs (from `nexus-fix-codegen`) depend on these primitives.

mod error;
mod span;

pub mod reader;
pub mod scan;
pub mod writer;

pub use error::{ChecksumError, DecodeError};
pub use reader::{FieldReader, RawField, checksum, find_tag, parse_tag, validate_checksum};
pub use scan::DelimiterScanner;
pub use span::{FieldSpan, GroupSpan};
pub use writer::{FieldWriter, encode_field, format_checksum};
