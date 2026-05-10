# Changelog

All notable changes to nexus-pool are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [Unreleased]

## [1.1.0] — 2026-05-10

Performance + contract change. Pooled guards now hold a strong
reference (Rc/Arc) instead of Weak, eliminating per-release refcount
work on the hot path.

### Changed (breaking contract)

- **In-pool values live until the last `Pooled<T>` guard drops.**
  Previously, dropping the pool with outstanding guards caused
  in-pool values to drop immediately, and guard drops bypassed the
  reset closure. Now: guards return values to the orphaned `Inner`
  (reset runs normally), and `Inner` with all in-pool values dies
  when the last guard exits. No leak, no UAF — just a delayed-drop
  semantic that affects shutdown ordering for resources holding
  external handles. See `docs/caveats.md` §2.
- `Pooled<T>::inner` field type changed from `Weak<Inner<T>>` to
  `Rc<Inner<T>>` (local) / `Arc<Inner<T>>` (sync). External code
  cannot observe the field directly, and `Pooled<T>` size is
  unchanged on both 32-bit and 64-bit targets (`Rc` and `Weak` are
  the same size).

### Performance

Measured on Intel Core Ultra 7 165U, pinned to physical P-cores
0,2 via `taskset`. Best-of-5 floor per percentile.

- **Sync pool same-thread release p50:** 72cy → 62cy (–10cy, 13.9%).
- **Sync pool concurrent 1-thread release p50:** 74cy → 60cy
  (–14cy, 18.9%).
- **Sync pool concurrent 4-thread release p50:** 476cy → 288cy
  (–188cy, 39.5%) — biggest tail-latency win because the contended
  `Weak::upgrade` CAS retry loop is gone.
- **Sync pool cross-thread 1-Returner release p50:** 430cy → 366cy
  (–64cy, 14.9%).
- **Local pool release p50:** unchanged at p50 in rdtscp benches,
  but `cargo asm` confirms hot-path `Pooled::drop` shrinks from
  3 `Cell` RMWs + 4 conditional branches to 1 RMW + 1 branch
  (function body 141 → 108 lines of generated asm). The single-
  threaded `Cell` load/dec/store pipelines well behind the
  surrounding `return_value` call and `Vec::push`, so the saved
  cycles aren't measurable above the rdtscp floor at p50. The
  codegen reduction is real and the contract now matches the sync
  path; the cycles win simply doesn't surface in this measurement
  methodology.

### Internal

- `#[inline]` added to hot accessors in `local::BoundedPool`,
  `local::Pool`, `sync::Pool`: `try_acquire`, `acquire`, `take`,
  `try_take`, `put`, `available` (8 sites total). Carries through
  monomorphization to user code without `cargo-llvm-lines`
  ballooning. (Originally landed in #244.)

## [1.0.4] — 2026-05-08

### Added

- **`#[must_use]` on `Pooled<T>`** in both `local::Pool` and
  `sync::Pool`. Dropping the guard immediately returns the object to
  the pool, so silently discarding the result of `pool.acquire()` was
  almost always a bug — the lint now catches it.

## [1.0.3] and earlier

Earlier history is not documented in this CHANGELOG. See git history
and GitHub release notes for details.
