# Changelog

All notable changes to nexus-platform are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `FileLock` — RAII exclusive file lock for mutual exclusion (blocking
  and non-blocking). Extracted from nexus-shm.
- `ProcessLease` — kernel-mediated process liveness detection via OFD
  byte-range locks. Extracted from nexus-shm.
- `Liveness` enum (`Alive`, `Dead`, `Unknown`) for lease probe results.
- Linux backend using OFD locks (`F_OFD_SETLK` / `F_OFD_GETLK`).
