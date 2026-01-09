//! # Determinism in Durable Executions
//!
//! For detailed documentation on determinism requirements, see the
//! [DETERMINISM.md](https://github.com/aws/aws-durable-execution-sdk-rust/blob/main/aws-durable-execution-sdk/docs/DETERMINISM.md)
//! file in the repository.
//!
//! ## Quick Summary
//!
//! Durable execution workflows must be deterministic because they can be replayed.
//! Common sources of non-determinism to avoid:
//!
//! - `HashMap`/`HashSet` iteration order (use `BTreeMap`/`BTreeSet` instead)
//! - Random number generation outside steps
//! - `SystemTime::now()` outside steps
//! - UUID generation outside steps
//! - Environment variables that affect control flow
//! - External API calls outside steps
//!
//! Use the replay-safe helpers in [`crate::replay_safe`] for deterministic
//! UUID and timestamp generation.

// This module is documentation-only.
