# Changelog

All notable changes to nexus-stats-detection are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [Unreleased]

## [1.1.0] — 2026-05-18

### Added

- **`PageHinkleyF64` / `PageHinkleyF32`** — sequential test for mean drift.
  O(1) per update, two-sided (detects upward and downward shifts).
- **`AdwinF64` / `AdwinF32`** — adaptive windowing for distribution change
  detection (Bifet & Gavalda, 2007). O(log n) per update, O(log n) memory.
  Requires `alloc` + (`std` or `libm`).
- **`PredictiveInfoBoundF64` / `PredictiveInfoBoundF32`** — streaming binned
  mutual information I(X;Y) with Miller-Madow bias correction. Equi-width
  bins on user-specified ranges. Requires `alloc` + (`std` or `libm`).

## [1.0.1] and earlier

Earlier history is not documented in this CHANGELOG. See git history
and GitHub release notes for details.
