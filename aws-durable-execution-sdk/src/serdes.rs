//! Serialization/Deserialization system for the AWS Durable Execution SDK.
//!
//! This module provides a trait-based abstraction for serializing and deserializing
//! data in checkpoints, allowing users to customize encoding strategies.
//!
//! # Overview
//!
//! The [`SerDes`] trait defines the interface for serialization, while [`JsonSerDes`]
//! provides a default JSON implementation using serde_json.
//!
//! # Sealed Trait
//!
//! The `SerDes` trait is sealed and cannot be implemented outside of this crate.
//! This allows the SDK maintainers to evolve the serialization interface without
//! breaking external code. If you need custom serialization behavior, use the
//! provided factory functions.
//!
//! # Example
//!
//! ```rust
//! use aws_durable_execution_sdk::serdes::{SerDes, JsonSerDes, SerDesContext};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Serialize, Deserialize, PartialEq, Debug)]
//! struct MyData {
//!     value: i32,
//! }
//!
//! let serdes = JsonSerDes::<MyData>::new();
//! let context = SerDesContext::new("op-123", "arn:aws:lambda:...");
//! let data = MyData { value: 42 };
//!
//! let serialized = serdes.serialize(&data, &context).unwrap();
//! let deserialized = serdes.deserialize(&serialized, &context).unwrap();
//! assert_eq!(data, deserialized);
//! ```

use std::fmt;
use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Serialize};

use crate::sealed::Sealed;

/// Error type for serialization/deserialization failures.
///
/// This error captures both serialization and deserialization failures
/// with descriptive messages.
#[derive(Debug, Clone)]
pub struct SerDesError {
    /// The kind of error (serialization or deserialization)
    pub kind: SerDesErrorKind,
    /// Descriptive error message
    pub message: String,
}

/// The kind of SerDes error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerDesErrorKind {
    /// Error during serialization
    Serialization,
    /// Error during deserialization
    Deserialization,
}

impl SerDesError {
    /// Creates a new serialization error.
    pub fn serialization(message: impl Into<String>) -> Self {
        Self {
            kind: SerDesErrorKind::Serialization,
            message: message.into(),
        }
    }

    /// Creates a new deserialization error.
    pub fn deserialization(message: impl Into<String>) -> Self {
        Self {
            kind: SerDesErrorKind::Deserialization,
            message: message.into(),
        }
    }
}

impl fmt::Display for SerDesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            SerDesErrorKind::Serialization => write!(f, "Serialization error: {}", self.message),
            SerDesErrorKind::Deserialization => {
                write!(f, "Deserialization error: {}", self.message)
            }
        }
    }
}

impl std::error::Error for SerDesError {}

impl From<serde_json::Error> for SerDesError {
    fn from(error: serde_json::Error) -> Self {
        if error.is_io() || error.is_syntax() || error.is_data() {
            Self::deserialization(error.to_string())
        } else {
            Self::serialization(error.to_string())
        }
    }
}

/// Context provided to serializers during serialization/deserialization.
///
/// This context contains information about the current operation and execution,
/// which can be used by custom serializers for logging, metrics, or custom encoding.
#[derive(Debug, Clone)]
pub struct SerDesContext {
    /// The unique identifier for the current operation
    pub operation_id: String,
    /// The ARN of the durable execution
    pub durable_execution_arn: String,
}

impl SerDesContext {
    /// Creates a new SerDesContext.
    pub fn new(operation_id: impl Into<String>, durable_execution_arn: impl Into<String>) -> Self {
        Self {
            operation_id: operation_id.into(),
            durable_execution_arn: durable_execution_arn.into(),
        }
    }
}

/// Trait for serialization and deserialization of checkpoint data.
///
/// Implement this trait to provide custom serialization strategies for
/// checkpoint data. The SDK provides [`JsonSerDes`] as the default implementation.
///
/// # Sealed Trait
///
/// This trait is sealed and cannot be implemented outside of this crate.
/// This allows the SDK maintainers to evolve the serialization interface without
/// breaking external code. If you need custom serialization behavior, use the
/// provided factory functions.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` to support concurrent operations.
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::serdes::{SerDes, JsonSerDes, SerDesContext};
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct MyData {
///     value: i32,
/// }
///
/// // Use the provided JsonSerDes implementation
/// let serdes = JsonSerDes::<MyData>::new();
/// let context = SerDesContext::new("op-123", "arn:aws:lambda:...");
/// let data = MyData { value: 42 };
///
/// let serialized = serdes.serialize(&data, &context).unwrap();
/// let deserialized = serdes.deserialize(&serialized, &context).unwrap();
/// assert_eq!(data, deserialized);
/// ```
///
/// # Requirements
///
/// - 3.2: THE SDK SHALL implement the sealed trait pattern for the `SerDes` trait
/// - 3.5: THE SDK SHALL document that these traits are sealed and cannot be implemented externally
#[allow(private_bounds)]
pub trait SerDes<T>: Sealed + Send + Sync {
    /// Serializes a value to a string representation.
    ///
    /// # Arguments
    ///
    /// * `value` - The value to serialize
    /// * `context` - Context containing operation and execution information
    ///
    /// # Returns
    ///
    /// A string representation of the value, or a [`SerDesError`] on failure.
    fn serialize(&self, value: &T, context: &SerDesContext) -> Result<String, SerDesError>;

    /// Deserializes a string representation back to a value.
    ///
    /// # Arguments
    ///
    /// * `data` - The string representation to deserialize
    /// * `context` - Context containing operation and execution information
    ///
    /// # Returns
    ///
    /// The deserialized value, or a [`SerDesError`] on failure.
    fn deserialize(&self, data: &str, context: &SerDesContext) -> Result<T, SerDesError>;
}

/// Default JSON serialization implementation using serde_json.
///
/// This is the default serializer used by the SDK when no custom serializer
/// is provided. It uses serde_json for JSON encoding/decoding.
///
/// # Type Parameters
///
/// * `T` - The type to serialize/deserialize. Must implement `Serialize` and `DeserializeOwned`.
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::serdes::{JsonSerDes, SerDes, SerDesContext};
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct MyData {
///     name: String,
///     count: u32,
/// }
///
/// let serdes = JsonSerDes::<MyData>::new();
/// let context = SerDesContext::new("op-1", "arn:aws:...");
/// let data = MyData { name: "test".to_string(), count: 42 };
///
/// let json = serdes.serialize(&data, &context).unwrap();
/// let restored: MyData = serdes.deserialize(&json, &context).unwrap();
/// assert_eq!(data, restored);
/// ```
pub struct JsonSerDes<T> {
    _marker: PhantomData<T>,
}

// Implement Sealed for JsonSerDes to allow it to implement SerDes
impl<T> Sealed for JsonSerDes<T> {}

impl<T> JsonSerDes<T> {
    /// Creates a new JsonSerDes instance.
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<T> Default for JsonSerDes<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for JsonSerDes<T> {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<T> fmt::Debug for JsonSerDes<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonSerDes").finish()
    }
}

impl<T> SerDes<T> for JsonSerDes<T>
where
    T: Serialize + DeserializeOwned,
{
    fn serialize(&self, value: &T, _context: &SerDesContext) -> Result<String, SerDesError> {
        serde_json::to_string(value).map_err(|e| SerDesError::serialization(e.to_string()))
    }

    fn deserialize(&self, data: &str, _context: &SerDesContext) -> Result<T, SerDesError> {
        serde_json::from_str(data).map_err(|e| SerDesError::deserialization(e.to_string()))
    }
}

// Ensure JsonSerDes is Send + Sync
unsafe impl<T> Send for JsonSerDes<T> {}
unsafe impl<T> Sync for JsonSerDes<T> {}

/// A custom serializer/deserializer that delegates to user-provided closures.
///
/// This struct allows users to provide custom serialization behavior without
/// implementing the sealed `SerDes` trait directly.
///
/// # Type Parameters
///
/// * `T` - The type to serialize/deserialize
/// * `S` - The serialization closure type
/// * `D` - The deserialization closure type
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::serdes::{custom_serdes, SerDes, SerDesContext, SerDesError};
///
/// // Create a custom serializer for strings that adds a prefix
/// let serdes = custom_serdes::<String, _, _>(
///     |value, _ctx| Ok(format!("PREFIX:{}", value)),
///     |data, _ctx| {
///         data.strip_prefix("PREFIX:")
///             .map(|s| s.to_string())
///             .ok_or_else(|| SerDesError::deserialization("Missing PREFIX"))
///     },
/// );
/// ```
///
/// # Requirements
///
/// - 3.6: THE SDK SHALL provide factory functions or builders for users who need custom behavior
pub struct CustomSerDes<T, S, D>
where
    T: Send + Sync,
    S: Fn(&T, &SerDesContext) -> Result<String, SerDesError> + Send + Sync,
    D: Fn(&str, &SerDesContext) -> Result<T, SerDesError> + Send + Sync,
{
    serialize_fn: S,
    deserialize_fn: D,
    _marker: PhantomData<T>,
}

// Implement Sealed for CustomSerDes to allow it to implement SerDes
impl<T, S, D> Sealed for CustomSerDes<T, S, D>
where
    T: Send + Sync,
    S: Fn(&T, &SerDesContext) -> Result<String, SerDesError> + Send + Sync,
    D: Fn(&str, &SerDesContext) -> Result<T, SerDesError> + Send + Sync,
{
}

impl<T, S, D> SerDes<T> for CustomSerDes<T, S, D>
where
    T: Send + Sync,
    S: Fn(&T, &SerDesContext) -> Result<String, SerDesError> + Send + Sync,
    D: Fn(&str, &SerDesContext) -> Result<T, SerDesError> + Send + Sync,
{
    fn serialize(&self, value: &T, context: &SerDesContext) -> Result<String, SerDesError> {
        (self.serialize_fn)(value, context)
    }

    fn deserialize(&self, data: &str, context: &SerDesContext) -> Result<T, SerDesError> {
        (self.deserialize_fn)(data, context)
    }
}

impl<T, S, D> fmt::Debug for CustomSerDes<T, S, D>
where
    T: Send + Sync,
    S: Fn(&T, &SerDesContext) -> Result<String, SerDesError> + Send + Sync,
    D: Fn(&str, &SerDesContext) -> Result<T, SerDesError> + Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CustomSerDes").finish()
    }
}

/// Creates a custom serializer/deserializer with user-provided closures.
///
/// This factory function allows users to create custom serialization behavior
/// without implementing the sealed `SerDes` trait directly.
///
/// # Type Parameters
///
/// * `T` - The type to serialize/deserialize
///
/// # Arguments
///
/// * `serialize_fn` - Closure that serializes a value to a string
/// * `deserialize_fn` - Closure that deserializes a string to a value
///
/// # Example
///
/// ```rust
/// use aws_durable_execution_sdk::serdes::{custom_serdes, SerDes, SerDesContext, SerDesError};
///
/// // Create a custom serializer for i32 that uses a simple format
/// let serdes = custom_serdes::<i32, _, _>(
///     |value, _ctx| Ok(value.to_string()),
///     |data, _ctx| data.parse().map_err(|e| SerDesError::deserialization(format!("{}", e))),
/// );
///
/// let context = SerDesContext::new("op-1", "arn:test");
/// let serialized = serdes.serialize(&42, &context).unwrap();
/// assert_eq!(serialized, "42");
///
/// let deserialized = serdes.deserialize("42", &context).unwrap();
/// assert_eq!(deserialized, 42);
/// ```
///
/// # Requirements
///
/// - 3.6: THE SDK SHALL provide factory functions or builders for users who need custom behavior
pub fn custom_serdes<T, S, D>(serialize_fn: S, deserialize_fn: D) -> CustomSerDes<T, S, D>
where
    T: Send + Sync,
    S: Fn(&T, &SerDesContext) -> Result<String, SerDesError> + Send + Sync,
    D: Fn(&str, &SerDesContext) -> Result<T, SerDesError> + Send + Sync,
{
    CustomSerDes {
        serialize_fn,
        deserialize_fn,
        _marker: PhantomData,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
    struct TestData {
        name: String,
        value: i32,
    }

    fn create_test_context() -> SerDesContext {
        SerDesContext::new("test-op-123", "arn:aws:lambda:us-east-1:123456789:function:test")
    }

    #[test]
    fn test_serdes_context_creation() {
        let ctx = SerDesContext::new("op-1", "arn:test");
        assert_eq!(ctx.operation_id, "op-1");
        assert_eq!(ctx.durable_execution_arn, "arn:test");
    }

    #[test]
    fn test_serdes_error_serialization() {
        let error = SerDesError::serialization("failed to serialize");
        assert_eq!(error.kind, SerDesErrorKind::Serialization);
        assert!(error.to_string().contains("Serialization error"));
    }

    #[test]
    fn test_serdes_error_deserialization() {
        let error = SerDesError::deserialization("failed to deserialize");
        assert_eq!(error.kind, SerDesErrorKind::Deserialization);
        assert!(error.to_string().contains("Deserialization error"));
    }

    #[test]
    fn test_json_serdes_serialize() {
        let serdes = JsonSerDes::<TestData>::new();
        let context = create_test_context();
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let result = serdes.serialize(&data, &context).unwrap();
        assert!(result.contains("\"name\":\"test\""));
        assert!(result.contains("\"value\":42"));
    }

    #[test]
    fn test_json_serdes_deserialize() {
        let serdes = JsonSerDes::<TestData>::new();
        let context = create_test_context();
        let json = r#"{"name":"test","value":42}"#;

        let result = serdes.deserialize(json, &context).unwrap();
        assert_eq!(result.name, "test");
        assert_eq!(result.value, 42);
    }

    #[test]
    fn test_json_serdes_round_trip() {
        let serdes = JsonSerDes::<TestData>::new();
        let context = create_test_context();
        let original = TestData {
            name: "round-trip".to_string(),
            value: 123,
        };

        let serialized = serdes.serialize(&original, &context).unwrap();
        let deserialized = serdes.deserialize(&serialized, &context).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_json_serdes_deserialize_invalid() {
        let serdes = JsonSerDes::<TestData>::new();
        let context = create_test_context();
        let invalid_json = "not valid json";

        let result = serdes.deserialize(invalid_json, &context);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, SerDesErrorKind::Deserialization);
    }

    #[test]
    fn test_json_serdes_default() {
        let serdes: JsonSerDes<TestData> = JsonSerDes::default();
        let context = create_test_context();
        let data = TestData {
            name: "default".to_string(),
            value: 1,
        };

        let result = serdes.serialize(&data, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_serdes_clone() {
        let serdes = JsonSerDes::<TestData>::new();
        let cloned = serdes.clone();
        let context = create_test_context();
        let data = TestData {
            name: "clone".to_string(),
            value: 2,
        };

        let result1 = serdes.serialize(&data, &context).unwrap();
        let result2 = cloned.serialize(&data, &context).unwrap();
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_json_serdes_primitive_types() {
        // Test with String
        let string_serdes = JsonSerDes::<String>::new();
        let context = create_test_context();
        let original = "hello world".to_string();
        let serialized = string_serdes.serialize(&original, &context).unwrap();
        let deserialized: String = string_serdes.deserialize(&serialized, &context).unwrap();
        assert_eq!(original, deserialized);

        // Test with i32
        let int_serdes = JsonSerDes::<i32>::new();
        let original = 42i32;
        let serialized = int_serdes.serialize(&original, &context).unwrap();
        let deserialized: i32 = int_serdes.deserialize(&serialized, &context).unwrap();
        assert_eq!(original, deserialized);

        // Test with Vec
        let vec_serdes = JsonSerDes::<Vec<i32>>::new();
        let original = vec![1, 2, 3, 4, 5];
        let serialized = vec_serdes.serialize(&original, &context).unwrap();
        let deserialized: Vec<i32> = vec_serdes.deserialize(&serialized, &context).unwrap();
        assert_eq!(original, deserialized);
    }

    /// Tests for sealed SerDes trait implementations.
    ///
    /// The SerDes trait is sealed, meaning it cannot be implemented outside this crate.
    /// This is enforced at compile time by requiring the private `Sealed` supertrait.
    /// External crates attempting to implement SerDes will get a compile error:
    /// "the trait bound `MyType: Sealed` is not satisfied"
    ///
    /// These tests verify that the internal implementations work correctly.
    mod sealed_serdes_tests {
        use super::*;

        #[test]
        fn test_json_serdes_implements_serdes() {
            // JsonSerDes should implement SerDes (compile-time check)
            let serdes: &dyn SerDes<String> = &JsonSerDes::<String>::new();
            let context = create_test_context();
            
            let serialized = serdes.serialize(&"test".to_string(), &context).unwrap();
            let deserialized = serdes.deserialize(&serialized, &context).unwrap();
            assert_eq!(deserialized, "test");
        }

        #[test]
        fn test_custom_serdes_implements_serdes() {
            // CustomSerDes should implement SerDes (compile-time check)
            let serdes = custom_serdes::<i32, _, _>(
                |value, _ctx| Ok(value.to_string()),
                |data, _ctx| data.parse().map_err(|e| SerDesError::deserialization(format!("{}", e))),
            );
            
            let serdes_ref: &dyn SerDes<i32> = &serdes;
            let context = create_test_context();
            
            let serialized = serdes_ref.serialize(&42, &context).unwrap();
            assert_eq!(serialized, "42");
            
            let deserialized = serdes_ref.deserialize("42", &context).unwrap();
            assert_eq!(deserialized, 42);
        }

        #[test]
        fn test_custom_serdes_round_trip() {
            let serdes = custom_serdes::<String, _, _>(
                |value, _ctx| Ok(format!("PREFIX:{}", value)),
                |data, _ctx| {
                    data.strip_prefix("PREFIX:")
                        .map(|s| s.to_string())
                        .ok_or_else(|| SerDesError::deserialization("Missing PREFIX"))
                },
            );
            
            let context = create_test_context();
            let original = "hello world".to_string();
            
            let serialized = serdes.serialize(&original, &context).unwrap();
            assert_eq!(serialized, "PREFIX:hello world");
            
            let deserialized = serdes.deserialize(&serialized, &context).unwrap();
            assert_eq!(deserialized, original);
        }

        #[test]
        fn test_custom_serdes_error_handling() {
            let serdes = custom_serdes::<i32, _, _>(
                |_value, _ctx| Err(SerDesError::serialization("intentional error")),
                |_data, _ctx| Err(SerDesError::deserialization("intentional error")),
            );
            
            let context = create_test_context();
            
            let serialize_result = serdes.serialize(&42, &context);
            assert!(serialize_result.is_err());
            assert_eq!(serialize_result.unwrap_err().kind, SerDesErrorKind::Serialization);
            
            let deserialize_result = serdes.deserialize("42", &context);
            assert!(deserialize_result.is_err());
            assert_eq!(deserialize_result.unwrap_err().kind, SerDesErrorKind::Deserialization);
        }

        #[test]
        fn test_custom_serdes_receives_context() {
            use std::sync::atomic::{AtomicBool, Ordering};
            
            let context_received = std::sync::Arc::new(AtomicBool::new(false));
            let context_clone = context_received.clone();
            
            let serdes = custom_serdes::<String, _, _>(
                move |value, ctx| {
                    assert_eq!(ctx.operation_id, "test-op-123");
                    assert!(ctx.durable_execution_arn.contains("lambda"));
                    context_clone.store(true, Ordering::SeqCst);
                    Ok(value.clone())
                },
                |data, _ctx| Ok(data.to_string()),
            );
            
            let context = create_test_context();
            let _ = serdes.serialize(&"test".to_string(), &context);
            
            assert!(context_received.load(Ordering::SeqCst));
        }

        #[test]
        fn test_custom_serdes_with_complex_type() {
            #[derive(Debug, Clone, PartialEq)]
            struct Point {
                x: i32,
                y: i32,
            }
            
            let serdes = custom_serdes::<Point, _, _>(
                |point, _ctx| Ok(format!("{},{}", point.x, point.y)),
                |data, _ctx| {
                    let parts: Vec<&str> = data.split(',').collect();
                    if parts.len() != 2 {
                        return Err(SerDesError::deserialization("Invalid format"));
                    }
                    let x = parts[0].parse().map_err(|_| SerDesError::deserialization("Invalid x"))?;
                    let y = parts[1].parse().map_err(|_| SerDesError::deserialization("Invalid y"))?;
                    Ok(Point { x, y })
                },
            );
            
            let context = create_test_context();
            let original = Point { x: 10, y: 20 };
            
            let serialized = serdes.serialize(&original, &context).unwrap();
            assert_eq!(serialized, "10,20");
            
            let deserialized = serdes.deserialize(&serialized, &context).unwrap();
            assert_eq!(deserialized, original);
        }
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use serde::{Deserialize, Serialize};

    /// **Feature: durable-execution-rust-sdk, Property 2: SerDes Round-Trip**
    /// **Validates: Requirements 11.2**
    ///
    /// For any value T that implements Serialize + DeserializeOwned,
    /// serializing then deserializing with JsonSerDes SHALL produce
    /// a value equal to the original.
    mod serdes_round_trip {
        use super::*;

        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct ComplexData {
            string_field: String,
            int_field: i64,
            bool_field: bool,
            optional_field: Option<String>,
            vec_field: Vec<i32>,
        }

        fn arbitrary_context() -> impl Strategy<Value = SerDesContext> {
            (any::<String>(), any::<String>()).prop_map(|(op_id, arn)| {
                SerDesContext::new(
                    if op_id.is_empty() {
                        "default-op".to_string()
                    } else {
                        op_id
                    },
                    if arn.is_empty() {
                        "arn:default".to_string()
                    } else {
                        arn
                    },
                )
            })
        }

        fn arbitrary_complex_data() -> impl Strategy<Value = ComplexData> {
            (
                any::<String>(),
                any::<i64>(),
                any::<bool>(),
                any::<Option<String>>(),
                any::<Vec<i32>>(),
            )
                .prop_map(
                    |(string_field, int_field, bool_field, optional_field, vec_field)| {
                        ComplexData {
                            string_field,
                            int_field,
                            bool_field,
                            optional_field,
                            vec_field,
                        }
                    },
                )
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: String round-trip
            /// For any String, serialize then deserialize produces the original value.
            #[test]
            fn prop_string_round_trip(value: String, context in arbitrary_context()) {
                let serdes = JsonSerDes::<String>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: i64 round-trip
            /// For any i64, serialize then deserialize produces the original value.
            #[test]
            fn prop_i64_round_trip(value: i64, context in arbitrary_context()) {
                let serdes = JsonSerDes::<i64>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: f64 round-trip with approximate equality
            /// For any finite f64, serialize then deserialize produces a value
            /// that is approximately equal (within floating-point precision limits).
            /// Note: JSON serialization may lose some precision for extreme values.
            #[test]
            fn prop_f64_round_trip(value in any::<f64>().prop_filter("finite", |v| v.is_finite()), context in arbitrary_context()) {
                let serdes = JsonSerDes::<f64>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized: f64 = serdes.deserialize(&serialized, &context).unwrap();
                
                // Use relative epsilon comparison for floating-point values
                // JSON may lose some precision, so we check if values are "close enough"
                let epsilon = 1e-10;
                let diff = (value - deserialized).abs();
                let relative_diff = if value.abs() > epsilon {
                    diff / value.abs()
                } else {
                    diff
                };
                prop_assert!(
                    relative_diff < epsilon,
                    "f64 round-trip failed: original={}, deserialized={}, relative_diff={}",
                    value, deserialized, relative_diff
                );
            }

            /// Property test: bool round-trip
            /// For any bool, serialize then deserialize produces the original value.
            #[test]
            fn prop_bool_round_trip(value: bool, context in arbitrary_context()) {
                let serdes = JsonSerDes::<bool>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: Vec<i32> round-trip
            /// For any Vec<i32>, serialize then deserialize produces the original value.
            #[test]
            fn prop_vec_round_trip(value: Vec<i32>, context in arbitrary_context()) {
                let serdes = JsonSerDes::<Vec<i32>>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: Option<String> round-trip
            /// For any Option<String>, serialize then deserialize produces the original value.
            #[test]
            fn prop_option_round_trip(value: Option<String>, context in arbitrary_context()) {
                let serdes = JsonSerDes::<Option<String>>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: ComplexData round-trip
            /// For any ComplexData struct, serialize then deserialize produces the original value.
            #[test]
            fn prop_complex_data_round_trip(
                value in arbitrary_complex_data(),
                context in arbitrary_context()
            ) {
                let serdes = JsonSerDes::<ComplexData>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }

            /// Property test: Nested structures round-trip
            /// For any HashMap<String, Vec<i32>>, serialize then deserialize produces the original value.
            #[test]
            fn prop_nested_round_trip(value: std::collections::HashMap<String, Vec<i32>>, context in arbitrary_context()) {
                let serdes = JsonSerDes::<std::collections::HashMap<String, Vec<i32>>>::new();
                let serialized = serdes.serialize(&value, &context).unwrap();
                let deserialized = serdes.deserialize(&serialized, &context).unwrap();
                prop_assert_eq!(value, deserialized);
            }
        }
    }
}
