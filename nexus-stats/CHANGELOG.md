# Changelog

All notable changes to nexus-stats are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [Unreleased]

## [5.0.0] — 2026-05-18

Breaking: tracks nexus-stats-core 2.0.0.

### Added

- **`clock` module** re-exported from nexus-stats-core. `Clock` trait,
  `WallClock`, `EpochClock`.

### Changed

- `std` feature now also provides `WallClock`.

### Removed

- All `Instant`-based stats types (see nexus-stats-core 2.0.0 CHANGELOG).
- `Raw` suffix dropped from windowed/CoDel type names.

## [4.x] — workspace re-export pattern

`nexus-stats` is the umbrella crate that re-exports from the
focused subcrates: `nexus-stats-core`, `nexus-stats-control`,
`nexus-stats-detection`, `nexus-stats-regression`, and
`nexus-stats-smoothing`. The umbrella version tracks the workspace
release cadence; subcrate versions track per-area changes.

For per-algorithm or per-type changes, see the relevant subcrate's
CHANGELOG (or its git history).

## [4.2.2] and earlier

Earlier history is not documented in this CHANGELOG. See git history,
GitHub release notes, and the per-subcrate CHANGELOGs for details.
