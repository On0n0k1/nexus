# Changelog

All notable changes to nexus-stats-core are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [Unreleased]

## [2.0.0] — 2026-05-18

Clock trait and Instant type removal.

### Added

- **`clock` module.** `Clock` trait (`stamp() -> u64`), `WallClock` (std,
  wraps `Instant`, returns elapsed nanos), `EpochClock` (manual/test clock
  with `set()`/`advance()`). Stats types accept `u64` timestamps; the caller
  owns the clock.
- **`elapsed(from, to)` helper** — saturating u64 subtraction.

### Removed

- **All `Instant`-based stats types.** `WindowedMax/MinF64/F32/I64/I32/I128`,
  `CoDelF64/F32/I64/I32/I128` (Instant variants), `LivenessInstant`,
  `EventRateInstant`, and `BucketAccumulator::update_instant()`. Epoch
  management belongs in the Clock, not the stats type.

### Changed

- **Renamed Raw variants to canonical.** `WindowedMaxF64Raw` → `WindowedMaxF64`,
  `CoDelF64Raw` → `CoDelF64`, etc. The `Raw` suffix was disambiguation for
  the now-removed Instant variants.

## [1.2.1] and earlier

Earlier history is not documented in this CHANGELOG. See git history
and GitHub release notes for details.
