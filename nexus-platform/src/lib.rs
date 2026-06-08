//! Platform-specific OS primitives behind a portable Rust API.
//!
//! # Primitives
//!
//! - [`FileLock`] — RAII exclusive file lock for mutual exclusion
//! - [`ProcessLease`] — kernel-mediated process liveness detection
//! - [`Liveness`] — result of probing a process lease

pub mod file_lock;
pub mod lease;

pub use file_lock::FileLock;
pub use lease::{Liveness, ProcessLease};
