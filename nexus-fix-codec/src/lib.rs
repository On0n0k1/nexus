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
mod types;

pub mod reader;
pub mod scan;
pub mod writer;

pub use error::{ChecksumError, DecodeError};
pub use nexus_ascii::AsciiTextStr;
pub use reader::{FieldReader, RawField, checksum, find_tag, parse_tag, validate_checksum};
pub use scan::DelimiterScanner;
pub use span::{FieldSpan, GroupSpan};
pub use types::{
    FixDate, FixDecimal, FixTime, FixTimestamp, encode_fix_bool, encode_fix_int, encode_fix_seqnum,
    encode_fix_uint, parse_fix_bool, parse_fix_int, parse_fix_seqnum, parse_fix_uint,
};
pub use writer::{FieldWriter, encode_field, format_checksum};

#[cfg(feature = "nexus-decimal")]
pub use types::DecimalConvError;

#[cfg(feature = "nexus-decimal")]
pub use types::DecimalToFixError;
