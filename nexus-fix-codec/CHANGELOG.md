# Changelog

All notable changes to nexus-fix-codec are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [Unreleased]

### Added

- `FieldSpan` and `GroupSpan` zero-copy field reference types
- SIMD SOH and `=` scanning: AVX-512, AVX2, SSE2, SWAR, scalar
- `DelimiterScanner` iterator with SIMD mask caching
- `FieldReader` with fused PSADBW checksum accumulation
- `FieldWriter` for writing `tag=value\x01` fields
- `parse_tag`, `find_tag`, `checksum`, `validate_checksum` helpers
- `encode_field`, `format_checksum` writer helpers
- `DecodeError` and `ChecksumError` error types
- Cycle-level benchmarks (`examples/perf_scan.rs`)
