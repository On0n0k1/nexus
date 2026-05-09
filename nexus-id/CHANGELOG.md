# Changelog

All notable changes to nexus-id are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/),
with the project-specific allowance that a minor bump may carry small,
narrowly-scoped breaking changes when external blast radius is
contained.

## [1.1.4] and earlier

`nexus-id` ships a broad family of ID generators (`Snowflake64`,
`Snowflake32`, `UuidV4`, `UuidV7`, `UlidGenerator`) and the
corresponding ID types (`Uuid`, `UuidCompact`, `Ulid`, `HexId64`,
`Base62Id`, `Base36Id`, `TypeId`, `MixedId64`, `SnowflakeId64`/`32`).
SIMD-accelerated hex encode/decode (SSSE3 / SSE2) on x86_64, with a
scalar fallback that is also used as the parity-test reference.

Earlier per-version history is not documented here. See git history
and GitHub release notes for details.
