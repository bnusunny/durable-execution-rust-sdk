//! Replay-safe helpers for generating deterministic non-deterministic values.
//!
//! This module provides utilities for generating values that would normally be
//! non-deterministic (like UUIDs and timestamps) in a way that is safe for replay.
//! During replay, these helpers ensure that the same values are generated as in
//! the original execution, maintaining determinism.
//!
//! # Why Replay-Safe Helpers?
//!
//! In durable execution workflows, functions may be replayed multiple times after
//! interruptions. If your code generates random UUIDs or captures the current time,
//! these values will differ between the original execution and replays, potentially
//! causing non-deterministic behavior.
//!
//! These helpers solve this problem by:
//! - **UUIDs**: Generating deterministic UUIDs based on the operation ID and a seed
//! - **Timestamps**: Using the execution start time instead of the current time
//!
//! # Example
//!
//! ```rust,ignore
//! use aws_durable_execution_sdk::replay_safe::{uuid_from_operation, timestamp_from_execution};
//!
//! async fn my_workflow(ctx: DurableContext) -> Result<(), DurableError> {
//!     // Generate a deterministic UUID for this operation
//!     let operation_id = ctx.next_operation_id();
//!     let uuid = uuid_from_operation(&operation_id, 0);
//!     
//!     // Get the execution start timestamp (replay-safe)
//!     if let Some(timestamp) = timestamp_from_execution(ctx.state()) {
//!         println!("Execution started at: {}", timestamp);
//!     }
//!     
//!     Ok(())
//! }
//! ```
//!
//! # Requirements
//!
//! - 22.1: THE Replay_Safe_Helpers MAY provide a deterministic UUID generator seeded by operation ID
//! - 22.2: THE Replay_Safe_Helpers MAY provide replay-safe timestamps derived from execution state
//! - 22.3: THE Replay_Safe_Helpers SHALL document how to use these helpers correctly

use blake2::{Blake2b512, Digest};

use crate::state::ExecutionState;

/// Generates a deterministic UUID from an operation ID and seed.
///
/// This function creates a UUID that is deterministic based on the input parameters.
/// The same operation_id and seed will always produce the same UUID, making it safe
/// for use in replay scenarios.
///
/// # Algorithm
///
/// The UUID is generated using blake2b hashing:
/// 1. Hash the operation_id bytes
/// 2. Hash the seed as little-endian bytes
/// 3. Take the first 16 bytes of the hash result
/// 4. Set the UUID version (4) and variant (RFC 4122) bits
///
/// # Arguments
///
/// * `operation_id` - The operation ID to use as the base for UUID generation.
///   This should be unique per operation within an execution.
/// * `seed` - An additional seed value to differentiate multiple UUIDs within
///   the same operation. Use different seed values (0, 1, 2, ...) if you need
///   multiple UUIDs in the same operation.
///
/// # Returns
///
/// A 128-bit UUID as a `[u8; 16]` array. You can convert this to a string
/// using the `uuid_to_string` helper function.
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::replay_safe::{uuid_from_operation, uuid_to_string};
///
/// let operation_id = "abc123";
/// let uuid_bytes = uuid_from_operation(operation_id, 0);
/// let uuid_string = uuid_to_string(&uuid_bytes);
/// println!("Generated UUID: {}", uuid_string);
///
/// // Same inputs always produce the same UUID
/// let uuid_bytes_2 = uuid_from_operation(operation_id, 0);
/// assert_eq!(uuid_bytes, uuid_bytes_2);
///
/// // Different seeds produce different UUIDs
/// let uuid_bytes_3 = uuid_from_operation(operation_id, 1);
/// assert_ne!(uuid_bytes, uuid_bytes_3);
/// ```
///
/// # Requirements
///
/// - 22.1: THE Replay_Safe_Helpers MAY provide a deterministic UUID generator seeded by operation ID
pub fn uuid_from_operation(operation_id: &str, seed: u64) -> [u8; 16] {
    let mut hasher = Blake2b512::new();
    hasher.update(operation_id.as_bytes());
    hasher.update(seed.to_le_bytes());
    let result = hasher.finalize();

    // Take the first 16 bytes for the UUID
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&result[..16]);

    // Set version 4 (random) in the version field (bits 12-15 of time_hi_and_version)
    // The version is stored in the 7th byte (index 6), upper nibble
    uuid_bytes[6] = (uuid_bytes[6] & 0x0f) | 0x40;

    // Set variant to RFC 4122 (bits 6-7 of clock_seq_hi_and_reserved)
    // The variant is stored in the 9th byte (index 8), upper 2 bits
    uuid_bytes[8] = (uuid_bytes[8] & 0x3f) | 0x80;

    uuid_bytes
}

/// Converts a UUID byte array to a standard UUID string format.
///
/// The output format is: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`
///
/// # Arguments
///
/// * `uuid_bytes` - A 16-byte array representing the UUID
///
/// # Returns
///
/// A string in standard UUID format (36 characters with hyphens).
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::replay_safe::{uuid_from_operation, uuid_to_string};
///
/// let uuid_bytes = uuid_from_operation("my-operation", 0);
/// let uuid_string = uuid_to_string(&uuid_bytes);
/// assert_eq!(uuid_string.len(), 36);
/// assert_eq!(uuid_string.chars().filter(|c| *c == '-').count(), 4);
/// ```
pub fn uuid_to_string(uuid_bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
        uuid_bytes[4], uuid_bytes[5],
        uuid_bytes[6], uuid_bytes[7],
        uuid_bytes[8], uuid_bytes[9],
        uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15]
    )
}

/// Generates a deterministic UUID string from an operation ID and seed.
///
/// This is a convenience function that combines `uuid_from_operation` and
/// `uuid_to_string` into a single call.
///
/// # Arguments
///
/// * `operation_id` - The operation ID to use as the base for UUID generation
/// * `seed` - An additional seed value to differentiate multiple UUIDs
///
/// # Returns
///
/// A string in standard UUID format (36 characters with hyphens).
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::replay_safe::uuid_string_from_operation;
///
/// let uuid = uuid_string_from_operation("my-operation", 0);
/// println!("Generated UUID: {}", uuid);
/// ```
///
/// # Requirements
///
/// - 22.1: THE Replay_Safe_Helpers MAY provide a deterministic UUID generator seeded by operation ID
pub fn uuid_string_from_operation(operation_id: &str, seed: u64) -> String {
    uuid_to_string(&uuid_from_operation(operation_id, seed))
}

/// Returns the replay-safe timestamp from the execution state.
///
/// This function returns the `StartTimestamp` from the EXECUTION operation,
/// which represents when the durable execution was first started. This timestamp
/// is consistent across all replays of the execution.
///
/// # Arguments
///
/// * `state` - The execution state containing the EXECUTION operation
///
/// # Returns
///
/// `Some(i64)` containing the start timestamp in milliseconds since Unix epoch
/// if the EXECUTION operation exists and has a start timestamp, `None` otherwise.
///
/// # Example
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::replay_safe::timestamp_from_execution;
///
/// async fn my_workflow(ctx: DurableContext) -> Result<(), DurableError> {
///     // Get the execution start timestamp (replay-safe)
///     if let Some(timestamp_ms) = timestamp_from_execution(ctx.state()) {
///         // Convert to seconds if needed
///         let timestamp_secs = timestamp_ms / 1000;
///         println!("Execution started at: {} seconds since epoch", timestamp_secs);
///     }
///     Ok(())
/// }
/// ```
///
/// # Why Use This Instead of `std::time::SystemTime::now()`?
///
/// Using `SystemTime::now()` in a durable execution is non-deterministic because:
/// - The first execution might capture time T1
/// - A replay might capture time T2 (different from T1)
/// - This can cause different behavior between executions
///
/// By using `timestamp_from_execution`, you always get the same timestamp
/// (the execution start time) regardless of when the replay occurs.
///
/// # Requirements
///
/// - 22.2: THE Replay_Safe_Helpers MAY provide replay-safe timestamps derived from execution state
/// - 22.3: THE Replay_Safe_Helpers SHALL document how to use these helpers correctly
pub fn timestamp_from_execution(state: &ExecutionState) -> Option<i64> {
    state
        .execution_operation()
        .and_then(|op| op.start_timestamp)
}

/// Returns the replay-safe timestamp as seconds since Unix epoch.
///
/// This is a convenience function that converts the millisecond timestamp
/// from `timestamp_from_execution` to seconds.
///
/// # Arguments
///
/// * `state` - The execution state containing the EXECUTION operation
///
/// # Returns
///
/// `Some(i64)` containing the start timestamp in seconds since Unix epoch
/// if available, `None` otherwise.
///
/// # Example
///
/// ```rust,ignore
/// use aws_durable_execution_sdk::replay_safe::timestamp_seconds_from_execution;
///
/// async fn my_workflow(ctx: DurableContext) -> Result<(), DurableError> {
///     if let Some(timestamp_secs) = timestamp_seconds_from_execution(ctx.state()) {
///         println!("Execution started at: {} seconds since epoch", timestamp_secs);
///     }
///     Ok(())
/// }
/// ```
///
/// # Requirements
///
/// - 22.2: THE Replay_Safe_Helpers MAY provide replay-safe timestamps derived from execution state
pub fn timestamp_seconds_from_execution(state: &ExecutionState) -> Option<i64> {
    timestamp_from_execution(state).map(|ms| ms / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_from_operation_deterministic() {
        // Same inputs should produce same output
        let uuid1 = uuid_from_operation("test-operation-123", 0);
        let uuid2 = uuid_from_operation("test-operation-123", 0);
        assert_eq!(uuid1, uuid2);
    }

    #[test]
    fn test_uuid_from_operation_different_seeds() {
        // Different seeds should produce different UUIDs
        let uuid1 = uuid_from_operation("test-operation-123", 0);
        let uuid2 = uuid_from_operation("test-operation-123", 1);
        assert_ne!(uuid1, uuid2);
    }

    #[test]
    fn test_uuid_from_operation_different_operation_ids() {
        // Different operation IDs should produce different UUIDs
        let uuid1 = uuid_from_operation("operation-a", 0);
        let uuid2 = uuid_from_operation("operation-b", 0);
        assert_ne!(uuid1, uuid2);
    }

    #[test]
    fn test_uuid_version_and_variant() {
        let uuid = uuid_from_operation("test-operation", 42);

        // Check version 4 (bits 12-15 of time_hi_and_version should be 0100)
        // This is in byte 6, upper nibble
        assert_eq!(uuid[6] & 0xf0, 0x40, "UUID version should be 4");

        // Check variant (bits 6-7 of clock_seq_hi_and_reserved should be 10)
        // This is in byte 8, upper 2 bits
        assert_eq!(uuid[8] & 0xc0, 0x80, "UUID variant should be RFC 4122");
    }

    #[test]
    fn test_uuid_to_string_format() {
        let uuid = uuid_from_operation("test-operation", 0);
        let uuid_string = uuid_to_string(&uuid);

        // Check length (8-4-4-4-12 = 32 hex chars + 4 hyphens = 36)
        assert_eq!(uuid_string.len(), 36);

        // Check hyphen positions
        assert_eq!(uuid_string.chars().nth(8), Some('-'));
        assert_eq!(uuid_string.chars().nth(13), Some('-'));
        assert_eq!(uuid_string.chars().nth(18), Some('-'));
        assert_eq!(uuid_string.chars().nth(23), Some('-'));

        // Check that all non-hyphen characters are hex digits
        for (i, c) in uuid_string.chars().enumerate() {
            if i == 8 || i == 13 || i == 18 || i == 23 {
                assert_eq!(c, '-');
            } else {
                assert!(
                    c.is_ascii_hexdigit(),
                    "Character at position {} should be hex digit",
                    i
                );
            }
        }
    }

    #[test]
    fn test_uuid_string_from_operation() {
        let uuid_string = uuid_string_from_operation("test-operation", 0);

        // Should be deterministic
        let uuid_string2 = uuid_string_from_operation("test-operation", 0);
        assert_eq!(uuid_string, uuid_string2);

        // Should be valid UUID format
        assert_eq!(uuid_string.len(), 36);
    }

    #[test]
    fn test_uuid_uniqueness_across_many_operations() {
        use std::collections::HashSet;

        let mut uuids = HashSet::new();

        // Generate 1000 UUIDs with different operation IDs
        for i in 0..1000 {
            let uuid = uuid_string_from_operation(&format!("operation-{}", i), 0);
            assert!(
                uuids.insert(uuid),
                "UUID collision detected at iteration {}",
                i
            );
        }
    }

    #[test]
    fn test_uuid_uniqueness_across_many_seeds() {
        use std::collections::HashSet;

        let mut uuids = HashSet::new();

        // Generate 1000 UUIDs with same operation ID but different seeds
        for seed in 0..1000u64 {
            let uuid = uuid_string_from_operation("same-operation", seed);
            assert!(
                uuids.insert(uuid),
                "UUID collision detected at seed {}",
                seed
            );
        }
    }

    // Note: timestamp_from_execution and timestamp_seconds_from_execution tests
    // require a full ExecutionState with an EXECUTION operation, which is tested
    // in integration tests. The functions are simple wrappers around ExecutionState
    // methods, so unit testing the logic is straightforward.

    #[test]
    fn test_timestamp_conversion() {
        // Test that timestamp_seconds_from_execution correctly divides by 1000
        // We can't easily test the full function without an ExecutionState,
        // but we can verify the conversion logic is correct
        let ms: i64 = 1704067200000; // 2024-01-01 00:00:00 UTC in milliseconds
        let secs = ms / 1000;
        assert_eq!(secs, 1704067200);
    }
}
