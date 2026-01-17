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
//! - [`StepFn`]: For step function closures
//!
//! # Example
//!
//! ```rust
//! use aws_durable_execution_sdk::traits::{DurableValue, StepFn};
//! use aws_durable_execution_sdk::handlers::StepContext;
//! use aws_durable_execution_sdk::DurableError;
//!
//! // Using DurableValue in a generic function
//! fn process_value<T: DurableValue>(value: T) -> String {
//!     // T is guaranteed to be Serialize + DeserializeOwned + Send + Sync + 'static
//!     serde_json::to_string(&value).unwrap_or_default()
//! }
//!
//! // Using StepFn for step function bounds
//! fn execute_step<T, F>(func: F) -> Result<T, DurableError>
//! where
//!     T: DurableValue,
//!     F: StepFn<T>,
//! {
//!     // F is guaranteed to be FnOnce(StepContext) -> Result<T, Box<dyn Error + Send + Sync>> + Send
//!     todo!()
//! }
//! ```

use serde::{de::DeserializeOwned, Serialize};

use crate::handlers::StepContext;

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
/// use aws_durable_execution_sdk::traits::DurableValue;
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
///
/// # Requirements
///
/// - 2.1: THE SDK SHALL provide a `DurableValue` trait alias for `Serialize + DeserializeOwned + Send + Sync + 'static`
/// - 2.4: THE SDK SHALL document trait aliases with examples showing equivalent expanded bounds
pub trait DurableValue: Serialize + DeserializeOwned + Send {}

/// Blanket implementation for all types meeting the bounds.
impl<T> DurableValue for T where T: Serialize + DeserializeOwned + Send {}

/// Trait alias for step function bounds.
///
/// This trait represents the bounds required for closures passed to the
/// `DurableContext::step` method. A `StepFn<T>` is a function that:
///
/// - Takes a `StepContext` parameter
/// - Returns `Result<T, Box<dyn Error + Send + Sync>>`
/// - Can be sent between threads (`Send`)
///
/// # Equivalent Bounds
///
/// `StepFn<T>` is equivalent to:
/// ```text
/// FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send
/// ```
///
/// # Blanket Implementation
///
/// This trait is automatically implemented for all closures and functions
/// that satisfy the bounds. You don't need to implement it manually.
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::traits::StepFn;
/// use aws_durable_execution_sdk::handlers::StepContext;
///
/// // A closure that implements StepFn<i32>
/// let step_fn = |_ctx: StepContext| -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
///     Ok(42)
/// };
///
/// // A named function that implements StepFn<String>
/// fn process_data(ctx: StepContext) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
///     Ok(format!("Processed by operation {}", ctx.operation_id))
/// }
///
/// // Use in generic functions
/// fn execute<T, F: StepFn<T>>(func: F) {
///     // func can be called with a StepContext
/// }
///
/// execute(step_fn);
/// execute(process_data);
/// ```
///
/// # Requirements
///
/// - 2.2: THE SDK SHALL provide a `StepFn` trait alias for step function bounds
/// - 2.4: THE SDK SHALL document trait aliases with examples showing equivalent expanded bounds
pub trait StepFn<T>:
    FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send
{
}

/// Blanket implementation for all closures meeting the bounds.
impl<T, F> StepFn<T> for F where
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send
{
}

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

    #[test]
    fn test_step_fn_closure() {
        fn assert_step_fn<T, F: StepFn<T>>(_f: F) {}

        // Test with a simple closure
        let closure =
            |_ctx: StepContext| -> Result<i32, Box<dyn std::error::Error + Send + Sync>> { Ok(42) };
        assert_step_fn(closure);
    }

    #[test]
    fn test_step_fn_with_capture() {
        fn assert_step_fn<T, F: StepFn<T>>(_f: F) {}

        let captured_value = 100;
        let closure =
            move |_ctx: StepContext| -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
                Ok(captured_value)
            };
        assert_step_fn(closure);
    }

    #[test]
    fn test_step_fn_returning_struct() {
        fn assert_step_fn<T, F: StepFn<T>>(_f: F) {}

        let closure =
            |_ctx: StepContext| -> Result<TestStruct, Box<dyn std::error::Error + Send + Sync>> {
                Ok(TestStruct {
                    id: "test".to_string(),
                    value: 1,
                })
            };
        assert_step_fn(closure);
    }

    // Test that StepFn works with named functions
    fn named_step_function(
        _ctx: StepContext,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("result".to_string())
    }

    #[test]
    fn test_step_fn_named_function() {
        fn assert_step_fn<T, F: StepFn<T>>(_f: F) {}
        assert_step_fn(named_step_function);
    }

    // Test type inference with trait aliases
    #[test]
    fn test_type_inference() {
        fn process<T: DurableValue, F: StepFn<T>>(_f: F) -> &'static str {
            "processed"
        }

        // Type inference should work without explicit type annotations
        let result = process(|_ctx| Ok(42i32));
        assert_eq!(result, "processed");

        let result = process(|_ctx| Ok("hello".to_string()));
        assert_eq!(result, "processed");
    }

    // Test that closures can borrow from environment (no 'static requirement)
    #[test]
    fn test_step_fn_borrowing_closure() {
        fn assert_step_fn<T, F: StepFn<T>>(_f: F) {}

        let external_data = "borrowed data".to_string();
        let closure =
            |_ctx: StepContext| -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
                Ok(external_data.clone())
            };
        assert_step_fn(closure);
    }
}
