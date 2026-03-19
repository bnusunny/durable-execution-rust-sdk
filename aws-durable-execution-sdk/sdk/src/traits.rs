//! Trait aliases for common bounds in the AWS Durable Execution SDK.
//!
//! This module provides trait aliases that simplify common trait bound combinations
//! used throughout the SDK. These aliases make function signatures more readable
//! and maintainable while preserving full type safety.
//!
//! # Overview
//!
//! The SDK frequently requires types to implement multiple traits for serialization,
//! thread safety, and lifetime requirements. Instead of repeating these bounds
//! everywhere, this module provides convenient trait aliases.
//!
//! # Available Trait Aliases
//!
//! - [`DurableValue`]: For values that can be durably stored and retrieved
//!
//! # Example
//!
//! ```rust
//! use durable_execution_sdk::traits::DurableValue;
//!
//! // Using DurableValue in a generic function
//! fn process_value<T: DurableValue>(value: T) -> String {
//!     // T is guaranteed to be Serialize + DeserializeOwned + Send
//!     serde_json::to_string(&value).unwrap_or_default()
//! }
//! ```

use serde::{de::DeserializeOwned, Serialize};

/// Trait alias for values that can be durably stored and retrieved.
///
/// This trait combines the necessary bounds for serialization, deserialization,
/// and thread-safe sending across async boundaries. Any type implementing
/// `DurableValue` can be:
///
/// - Serialized to JSON for checkpointing
/// - Deserialized from JSON during replay
/// - Safely sent between threads
///
/// # Equivalent Bounds
///
/// `DurableValue` is equivalent to:
/// ```text
/// Serialize + DeserializeOwned + Send
/// ```
///
/// # Blanket Implementation
///
/// This trait is automatically implemented for all types that satisfy the bounds.
/// You don't need to implement it manually.
///
/// # Example
///
/// ```rust
/// use durable_execution_sdk::traits::DurableValue;
/// use serde::{Deserialize, Serialize};
///
/// // This type automatically implements DurableValue
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct OrderResult {
///     order_id: String,
///     status: String,
///     total: f64,
/// }
///
/// // Use in generic functions
/// fn store_result<T: DurableValue>(result: T) {
///     let json = serde_json::to_string(&result).unwrap();
///     println!("Storing: {}", json);
/// }
///
/// let result = OrderResult {
///     order_id: "ORD-123".to_string(),
///     status: "completed".to_string(),
///     total: 99.99,
/// };
/// store_result(result);
/// ```
pub trait DurableValue: Serialize + DeserializeOwned + Send {}

/// Blanket implementation for all types meeting the bounds.
impl<T> DurableValue for T where T: Serialize + DeserializeOwned + Send {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    // Test that common types implement DurableValue
    #[test]
    fn test_durable_value_primitives() {
        fn assert_durable_value<T: DurableValue>() {}

        assert_durable_value::<i32>();
        assert_durable_value::<i64>();
        assert_durable_value::<u32>();
        assert_durable_value::<u64>();
        assert_durable_value::<f32>();
        assert_durable_value::<f64>();
        assert_durable_value::<bool>();
        assert_durable_value::<String>();
        assert_durable_value::<()>();
    }

    #[test]
    fn test_durable_value_collections() {
        fn assert_durable_value<T: DurableValue>() {}

        assert_durable_value::<Vec<i32>>();
        assert_durable_value::<Vec<String>>();
        assert_durable_value::<std::collections::HashMap<String, i32>>();
        assert_durable_value::<Option<String>>();
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        id: String,
        value: i32,
    }

    #[test]
    fn test_durable_value_custom_struct() {
        fn assert_durable_value<T: DurableValue>() {}
        assert_durable_value::<TestStruct>();
    }

    #[test]
    fn test_durable_value_serialization() {
        fn serialize_value<T: DurableValue>(value: &T) -> String {
            serde_json::to_string(value).unwrap()
        }

        fn deserialize_value<T: DurableValue>(json: &str) -> T {
            serde_json::from_str(json).unwrap()
        }

        let original = TestStruct {
            id: "test-123".to_string(),
            value: 42,
        };

        let json = serialize_value(&original);
        let restored: TestStruct = deserialize_value(&json);

        assert_eq!(original, restored);
    }
}
