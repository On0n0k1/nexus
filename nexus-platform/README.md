# nexus-platform

Platform-specific OS primitives behind a portable Rust API.

## Overview

`nexus-platform` provides low-level OS primitives that other nexus crates build
on. Each primitive has a portable Rust API with platform-specific backends
selected at compile time.

## Primitives

### `FileLock`

RAII exclusive file lock for mutual exclusion. Acquires an advisory lock on a
file at the given path, creating it if necessary. The lock is released when the
struct drops.

```rust,no_run
use nexus_platform::FileLock;

// Blocking — waits until the lock is available
let mut lock = FileLock::lock("/tmp/my.lock").unwrap();

// Non-blocking — returns None if already held
let lock = FileLock::try_lock("/tmp/my.lock").unwrap();
```

### `ProcessLease`

Kernel-mediated process liveness detection. Uses an advisory byte-range lock
on a file descriptor to signal that a process is alive. When the owning
process exits (even via `SIGKILL`), the kernel releases the lock automatically.

This is **not** mutual exclusion — it is a liveness oracle backed by the
kernel's lock table.

```rust,no_run
use std::os::fd::AsFd;
use nexus_platform::{ProcessLease, Liveness};

// Owner claims a lease on a shared file's fd
let file = std::fs::File::open("/dev/shm/my-segment").unwrap();
let claimed = ProcessLease::claim(file.as_fd()).unwrap();

// Peer probes whether the owner is still alive
let status = ProcessLease::probe(file.as_fd());
assert_eq!(status, Liveness::Alive);
```

## Platform Support

| Platform | `FileLock` | `ProcessLease` |
|----------|-----------|----------------|
| Linux    | OFD locks (`F_OFD_SETLK`) | OFD locks (`F_OFD_SETLK` / `F_OFD_GETLK`) |
| macOS    | Planned   | Planned        |
| Windows  | Planned   | Planned        |

## Dependency Chain

```text
nexus-platform    (FileLock, ProcessLease, OS primitives)
       ↑
   nexus-shm      (segments, regions, liveness)
       ↑
  nexus-journal   (SegmentedLog, Conductor, manifest)
```
