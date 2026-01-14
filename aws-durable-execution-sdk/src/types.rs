//! Newtype wrappers for domain identifiers in the AWS Durable Execution SDK.
//!
//! This module provides type-safe wrappers for string identifiers used throughout
//! the SDK. These newtypes prevent accidental mixing of different ID types at
//! compile time while maintaining full compatibility with string-based APIs.
//!
//! # Example
//!
//! ```rust
//! use aws_durable_execution_sdk::types::{OperationId, ExecutionArn, CallbackId};
//!
//! // Create from String or &str (no validation)
//! let op_id = OperationId::from("op-123");
//! let op_id2: OperationId = "op-456".into();
//!
//! // Create with validation
//! let op_id3 = OperationId::new("op-789").unwrap();
//! assert!(OperationId::new("").is_err()); // Empty strings rejected
//!
//! // Use as string via Deref
//! assert!(op_id.starts_with("op-"));
//!
//! // Use in HashMap (implements Hash, Eq)
//! use std::collections::HashMap;
//! let mut map: HashMap<OperationId, String> = HashMap::new();
//! map.insert(op_id, "value".to_string());
//! ```

use std::fmt;
use std::hash::Hash;
use std::ops::Deref;

use serde::{Deserialize, Serialize};

/// Error returned when newtype validation fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// The type name that failed validation
    pub type_name: &'static str,
    /// Description of the validation failure
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.type_name, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// A unique identifier for an operation within a durable execution.
///
/// `OperationId` wraps a `String` to provide type safety, preventing accidental
/// mixing with other string-based identifiers like `ExecutionArn` or `CallbackId`.
///
/// # Construction
///
/// ```rust
/// use aws_durable_execution_sdk::types::OperationId;
///
/// // From String (no validation)
/// let id1 = OperationId::from("op-123".to_string());
///
/// // From &str (no validation)
/// let id2 = OperationId::from("op-456");
///
/// // Using Into trait (no validation)
/// let id3: OperationId = "op-789".into();
///
/// // With validation
/// let id4 = OperationId::new("op-abc").unwrap();
/// assert!(OperationId::new("").is_err()); // Empty strings rejected
/// ```
///
/// # String Access
///
/// ```rust
/// use aws_durable_execution_sdk::types::OperationId;
///
/// let id = OperationId::from("op-123");
///
/// // Via Deref (automatic)
/// assert!(id.starts_with("op-"));
/// assert_eq!(id.len(), 6);
///
/// // Via AsRef
/// let s: &str = id.as_ref();
/// assert_eq!(s, "op-123");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(String);

impl OperationId {
    /// Creates a new `OperationId` with validation.
    ///
    /// Returns an error if the value is empty.
    pub fn new(id: impl Into<String>) -> Result<Self, ValidationError> {
        let id = id.into();
        if id.is_empty() {
            return Err(ValidationError {
                type_name: "OperationId",
                message: "value cannot be empty".to_string(),
            });
        }
        Ok(Self(id))
    }

    /// Creates a new `OperationId` without validation.
    ///
    /// Use this when you know the value is valid or when migrating
    /// from existing code that uses raw strings.
    #[inline]
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the inner string value.
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Returns a reference to the inner string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for OperationId {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for OperationId {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for OperationId {
    #[inline]
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for OperationId {
    #[inline]
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}


/// The Amazon Resource Name identifying a durable execution.
///
/// `ExecutionArn` wraps a `String` to provide type safety for execution ARNs.
/// It includes validation to ensure the ARN follows the expected format.
///
/// # ARN Format
///
/// A valid durable execution ARN follows the pattern:
/// `arn:<partition>:lambda:<region>:<account>:function:<function-name>:durable:<execution-id>`
///
/// Supported partitions include: `aws`, `aws-cn`, `aws-us-gov`, `aws-iso`, `aws-iso-b`, etc.
///
/// # Construction
///
/// ```rust
/// use aws_durable_execution_sdk::types::ExecutionArn;
///
/// // From String (no validation)
/// let arn1 = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123".to_string());
///
/// // With validation
/// let arn2 = ExecutionArn::new("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123");
/// assert!(arn2.is_ok());
///
/// // China region
/// let arn_cn = ExecutionArn::new("arn:aws-cn:lambda:cn-north-1:123456789012:function:my-func:durable:abc123");
/// assert!(arn_cn.is_ok());
///
/// // Invalid ARN rejected
/// assert!(ExecutionArn::new("").is_err());
/// assert!(ExecutionArn::new("not-an-arn").is_err());
/// ```
///
/// # String Access
///
/// ```rust
/// use aws_durable_execution_sdk::types::ExecutionArn;
///
/// let arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123");
///
/// // Via Deref (automatic)
/// assert!(arn.starts_with("arn:"));
///
/// // Via AsRef
/// let s: &str = arn.as_ref();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecutionArn(String);

impl ExecutionArn {
    /// Creates a new `ExecutionArn` with validation.
    ///
    /// Returns an error if the value is empty, doesn't start with "arn:aws:lambda:",
    /// or doesn't contain ":durable:".
    pub fn new(arn: impl Into<String>) -> Result<Self, ValidationError> {
        let arn = arn.into();
        Self::validate(&arn)?;
        Ok(Self(arn))
    }

    /// Creates a new `ExecutionArn` without validation.
    ///
    /// Use this when you know the value is valid or when migrating
    /// from existing code that uses raw strings.
    #[inline]
    pub fn new_unchecked(arn: impl Into<String>) -> Self {
        Self(arn.into())
    }

    /// Returns the inner string value.
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Returns a reference to the inner string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validates that the string is a valid durable execution ARN format.
    ///
    /// A valid ARN should:
    /// - Not be empty
    /// - Start with "arn:" followed by a valid AWS partition (aws, aws-cn, aws-us-gov, etc.)
    /// - Contain ":lambda:" service identifier
    /// - Contain ":durable:" segment
    fn validate(value: &str) -> Result<(), ValidationError> {
        if value.is_empty() {
            return Err(ValidationError {
                type_name: "ExecutionArn",
                message: "value cannot be empty".to_string(),
            });
        }

        // Check for valid ARN prefix with any AWS partition
        // Valid partitions: aws, aws-cn, aws-us-gov, aws-iso, aws-iso-b, etc.
        if !value.starts_with("arn:") {
            return Err(ValidationError {
                type_name: "ExecutionArn",
                message: "must start with 'arn:'".to_string(),
            });
        }

        // Check for lambda service
        if !value.contains(":lambda:") {
            return Err(ValidationError {
                type_name: "ExecutionArn",
                message: "must contain ':lambda:' service identifier".to_string(),
            });
        }

        if !value.contains(":durable:") {
            return Err(ValidationError {
                type_name: "ExecutionArn",
                message: "must contain ':durable:' segment".to_string(),
            });
        }

        Ok(())
    }
}

impl fmt::Display for ExecutionArn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for ExecutionArn {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for ExecutionArn {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for ExecutionArn {
    #[inline]
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ExecutionArn {
    #[inline]
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A unique identifier for a callback operation.
///
/// `CallbackId` wraps a `String` to provide type safety for callback identifiers.
/// Callback IDs are used by external systems to signal completion of asynchronous
/// operations.
///
/// # Construction
///
/// ```rust
/// use aws_durable_execution_sdk::types::CallbackId;
///
/// // From String (no validation)
/// let id1 = CallbackId::from("callback-123".to_string());
///
/// // From &str (no validation)
/// let id2 = CallbackId::from("callback-456");
///
/// // With validation
/// let id3 = CallbackId::new("callback-abc").unwrap();
/// assert!(CallbackId::new("").is_err()); // Empty strings rejected
/// ```
///
/// # String Access
///
/// ```rust
/// use aws_durable_execution_sdk::types::CallbackId;
///
/// let id = CallbackId::from("callback-123");
///
/// // Via Deref (automatic)
/// assert!(id.starts_with("callback-"));
///
/// // Via AsRef
/// let s: &str = id.as_ref();
/// assert_eq!(s, "callback-123");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallbackId(String);

impl CallbackId {
    /// Creates a new `CallbackId` with validation.
    ///
    /// Returns an error if the value is empty.
    pub fn new(id: impl Into<String>) -> Result<Self, ValidationError> {
        let id = id.into();
        if id.is_empty() {
            return Err(ValidationError {
                type_name: "CallbackId",
                message: "value cannot be empty".to_string(),
            });
        }
        Ok(Self(id))
    }

    /// Creates a new `CallbackId` without validation.
    ///
    /// Use this when you know the value is valid or when migrating
    /// from existing code that uses raw strings.
    #[inline]
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the inner string value.
    #[inline]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Returns a reference to the inner string.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CallbackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Deref for CallbackId {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for CallbackId {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<String> for CallbackId {
    #[inline]
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CallbackId {
    #[inline]
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ==================== OperationId Tests ====================

    #[test]
    fn test_operation_id_from_string() {
        let id = OperationId::from("op-123".to_string());
        assert_eq!(id.as_str(), "op-123");
    }

    #[test]
    fn test_operation_id_from_str() {
        let id = OperationId::from("op-456");
        assert_eq!(id.as_str(), "op-456");
    }

    #[test]
    fn test_operation_id_into() {
        let id: OperationId = "op-789".into();
        assert_eq!(id.as_str(), "op-789");
    }

    #[test]
    fn test_operation_id_new_valid() {
        let id = OperationId::new("op-abc").unwrap();
        assert_eq!(id.as_str(), "op-abc");
    }

    #[test]
    fn test_operation_id_new_empty_rejected() {
        let result = OperationId::new("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.type_name, "OperationId");
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn test_operation_id_display() {
        let id = OperationId::from("op-display");
        assert_eq!(format!("{}", id), "op-display");
    }

    #[test]
    fn test_operation_id_debug() {
        let id = OperationId::from("op-debug");
        let debug_str = format!("{:?}", id);
        assert!(debug_str.contains("op-debug"));
    }

    #[test]
    fn test_operation_id_deref() {
        let id = OperationId::from("op-deref-test");
        assert!(id.starts_with("op-"));
        assert_eq!(id.len(), 13);
    }

    #[test]
    fn test_operation_id_as_ref() {
        let id = OperationId::from("op-asref");
        let s: &str = id.as_ref();
        assert_eq!(s, "op-asref");
    }

    #[test]
    fn test_operation_id_hash_and_eq() {
        let id1 = OperationId::from("op-hash");
        let id2 = OperationId::from("op-hash");
        let id3 = OperationId::from("op-different");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);

        let mut map: HashMap<OperationId, String> = HashMap::new();
        map.insert(id1.clone(), "value1".to_string());
        assert_eq!(map.get(&id2), Some(&"value1".to_string()));
        assert_eq!(map.get(&id3), None);
    }

    #[test]
    fn test_operation_id_serde_roundtrip() {
        let id = OperationId::from("op-serde-test");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"op-serde-test\"");

        let deserialized: OperationId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn test_operation_id_into_inner() {
        let id = OperationId::from("op-inner");
        let inner = id.into_inner();
        assert_eq!(inner, "op-inner");
    }

    // ==================== ExecutionArn Tests ====================

    #[test]
    fn test_execution_arn_from_string() {
        let arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123".to_string());
        assert!(arn.as_str().starts_with("arn:aws:lambda:"));
    }

    #[test]
    fn test_execution_arn_from_str() {
        let arn = ExecutionArn::from("arn:aws:lambda:us-west-2:123456789012:function:test:durable:xyz");
        assert!(arn.contains(":durable:"));
    }

    #[test]
    fn test_execution_arn_new_valid() {
        let arn = ExecutionArn::new("arn:aws:lambda:eu-west-1:123456789012:function:func:durable:id123");
        assert!(arn.is_ok());
    }

    #[test]
    fn test_execution_arn_new_empty_rejected() {
        let result = ExecutionArn::new("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.type_name, "ExecutionArn");
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn test_execution_arn_new_invalid_prefix_rejected() {
        let result = ExecutionArn::new("not-an-arn");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("arn:"));
    }

    #[test]
    fn test_execution_arn_new_missing_lambda_rejected() {
        let result = ExecutionArn::new("arn:aws:s3:us-east-1:123456789012:bucket:durable:abc");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains(":lambda:"));
    }

    #[test]
    fn test_execution_arn_new_missing_durable_rejected() {
        let result = ExecutionArn::new("arn:aws:lambda:us-east-1:123456789012:function:my-func");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains(":durable:"));
    }

    #[test]
    fn test_execution_arn_new_aws_cn_partition() {
        let arn = ExecutionArn::new("arn:aws-cn:lambda:cn-north-1:123456789012:function:my-func:durable:abc123");
        assert!(arn.is_ok());
    }

    #[test]
    fn test_execution_arn_new_aws_us_gov_partition() {
        let arn = ExecutionArn::new("arn:aws-us-gov:lambda:us-gov-west-1:123456789012:function:my-func:durable:abc123");
        assert!(arn.is_ok());
    }

    #[test]
    fn test_execution_arn_new_aws_iso_partition() {
        let arn = ExecutionArn::new("arn:aws-iso:lambda:us-iso-east-1:123456789012:function:my-func:durable:abc123");
        assert!(arn.is_ok());
    }

    #[test]
    fn test_execution_arn_display() {
        let arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:f:durable:x");
        assert_eq!(format!("{}", arn), "arn:aws:lambda:us-east-1:123456789012:function:f:durable:x");
    }

    #[test]
    fn test_execution_arn_deref() {
        let arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123456789012:function:f:durable:x");
        assert!(arn.starts_with("arn:"));
        assert!(arn.contains("lambda"));
    }

    #[test]
    fn test_execution_arn_hash_and_eq() {
        let arn1 = ExecutionArn::from("arn:aws:lambda:us-east-1:123:function:f:durable:a");
        let arn2 = ExecutionArn::from("arn:aws:lambda:us-east-1:123:function:f:durable:a");
        let arn3 = ExecutionArn::from("arn:aws:lambda:us-east-1:123:function:f:durable:b");

        assert_eq!(arn1, arn2);
        assert_ne!(arn1, arn3);

        let mut map: HashMap<ExecutionArn, i32> = HashMap::new();
        map.insert(arn1.clone(), 42);
        assert_eq!(map.get(&arn2), Some(&42));
    }

    #[test]
    fn test_execution_arn_serde_roundtrip() {
        let arn = ExecutionArn::from("arn:aws:lambda:us-east-1:123:function:f:durable:serde");
        let json = serde_json::to_string(&arn).unwrap();
        let deserialized: ExecutionArn = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, arn);
    }

    // ==================== CallbackId Tests ====================

    #[test]
    fn test_callback_id_from_string() {
        let id = CallbackId::from("callback-123".to_string());
        assert_eq!(id.as_str(), "callback-123");
    }

    #[test]
    fn test_callback_id_from_str() {
        let id = CallbackId::from("callback-456");
        assert_eq!(id.as_str(), "callback-456");
    }

    #[test]
    fn test_callback_id_new_valid() {
        let id = CallbackId::new("callback-abc").unwrap();
        assert_eq!(id.as_str(), "callback-abc");
    }

    #[test]
    fn test_callback_id_new_empty_rejected() {
        let result = CallbackId::new("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.type_name, "CallbackId");
        assert!(err.message.contains("empty"));
    }

    #[test]
    fn test_callback_id_display() {
        let id = CallbackId::from("callback-display");
        assert_eq!(format!("{}", id), "callback-display");
    }

    #[test]
    fn test_callback_id_deref() {
        let id = CallbackId::from("callback-deref");
        assert!(id.starts_with("callback-"));
        assert_eq!(id.len(), 14);
    }

    #[test]
    fn test_callback_id_as_ref() {
        let id = CallbackId::from("callback-asref");
        let s: &str = id.as_ref();
        assert_eq!(s, "callback-asref");
    }

    #[test]
    fn test_callback_id_hash_and_eq() {
        let id1 = CallbackId::from("callback-hash");
        let id2 = CallbackId::from("callback-hash");
        let id3 = CallbackId::from("callback-other");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);

        let mut map: HashMap<CallbackId, bool> = HashMap::new();
        map.insert(id1.clone(), true);
        assert_eq!(map.get(&id2), Some(&true));
    }

    #[test]
    fn test_callback_id_serde_roundtrip() {
        let id = CallbackId::from("callback-serde");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"callback-serde\"");

        let deserialized: CallbackId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    // ==================== ValidationError Tests ====================

    #[test]
    fn test_validation_error_display() {
        let err = ValidationError {
            type_name: "TestType",
            message: "test error message".to_string(),
        };
        assert_eq!(format!("{}", err), "TestType: test error message");
    }

    // ==================== Backward Compatibility Tests ====================

    /// Tests that existing serialized data (plain JSON strings) can be deserialized
    /// into newtypes, ensuring backward compatibility with existing checkpoints.
    #[test]
    fn test_backward_compat_deserialize_plain_string_to_operation_id() {
        // Simulate existing serialized data (plain JSON string)
        let existing_json = "\"existing-op-id-12345\"";
        
        // Should deserialize successfully into OperationId
        let id: OperationId = serde_json::from_str(existing_json).unwrap();
        assert_eq!(id.as_str(), "existing-op-id-12345");
    }

    #[test]
    fn test_backward_compat_deserialize_plain_string_to_execution_arn() {
        // Simulate existing serialized data (plain JSON string)
        let existing_json = "\"arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:abc123\"";
        
        // Should deserialize successfully into ExecutionArn
        let arn: ExecutionArn = serde_json::from_str(existing_json).unwrap();
        assert!(arn.contains(":durable:"));
    }

    #[test]
    fn test_backward_compat_deserialize_plain_string_to_callback_id() {
        // Simulate existing serialized data (plain JSON string)
        let existing_json = "\"callback-xyz-789\"";
        
        // Should deserialize successfully into CallbackId
        let id: CallbackId = serde_json::from_str(existing_json).unwrap();
        assert_eq!(id.as_str(), "callback-xyz-789");
    }

    /// Tests that functions accepting `impl Into<NewType>` work with String,
    /// ensuring backward compatibility with existing code.
    #[test]
    fn test_backward_compat_impl_into_with_string() {
        fn accept_operation_id(id: impl Into<OperationId>) -> OperationId {
            id.into()
        }

        fn accept_execution_arn(arn: impl Into<ExecutionArn>) -> ExecutionArn {
            arn.into()
        }

        fn accept_callback_id(id: impl Into<CallbackId>) -> CallbackId {
            id.into()
        }

        // Should work with String
        let op_id = accept_operation_id("op-123".to_string());
        assert_eq!(op_id.as_str(), "op-123");

        // Should work with &str
        let op_id = accept_operation_id("op-456");
        assert_eq!(op_id.as_str(), "op-456");

        // Should work with ExecutionArn
        let arn = accept_execution_arn("arn:aws:lambda:us-east-1:123:function:f:durable:x".to_string());
        assert!(arn.contains(":durable:"));

        // Should work with CallbackId
        let cb_id = accept_callback_id("callback-abc");
        assert_eq!(cb_id.as_str(), "callback-abc");
    }

    /// Tests that newtypes serialize to the same format as plain strings,
    /// ensuring backward compatibility with existing consumers.
    #[test]
    fn test_backward_compat_serialize_same_as_string() {
        let op_id = OperationId::from("op-123");
        let plain_string = "op-123".to_string();

        let op_id_json = serde_json::to_string(&op_id).unwrap();
        let string_json = serde_json::to_string(&plain_string).unwrap();

        // Both should serialize to the same JSON
        assert_eq!(op_id_json, string_json);
        assert_eq!(op_id_json, "\"op-123\"");
    }

    /// Tests that newtypes can be used interchangeably with &str in string operations.
    #[test]
    fn test_backward_compat_string_operations() {
        let op_id = OperationId::from("op-123-suffix");

        // Should work with string methods via Deref
        assert!(op_id.starts_with("op-"));
        assert!(op_id.ends_with("-suffix"));
        assert!(op_id.contains("123"));
        assert_eq!(op_id.len(), 13); // "op-123-suffix" is 13 characters

        // Should work with functions expecting &str
        fn process_str(s: &str) -> String {
            s.to_uppercase()
        }
        assert_eq!(process_str(&op_id), "OP-123-SUFFIX");
    }

    // ==================== Property-Based Tests ====================

    use proptest::prelude::*;

    /// Strategy for generating non-empty strings (valid for OperationId and CallbackId)
    fn non_empty_string_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_-]{1,64}".prop_map(|s| s)
    }

    /// Strategy for generating valid ExecutionArn strings
    fn valid_execution_arn_strategy() -> impl Strategy<Value = String> {
        (
            prop_oneof![
                Just("aws"),
                Just("aws-cn"),
                Just("aws-us-gov"),
                Just("aws-iso"),
                Just("aws-iso-b"),
            ],
            prop_oneof![
                Just("us-east-1"),
                Just("us-west-2"),
                Just("eu-west-1"),
                Just("cn-north-1"),
                Just("us-gov-west-1"),
            ],
            "[0-9]{12}",
            "[a-zA-Z0-9_-]{1,32}",
            "[a-zA-Z0-9]{8,32}",
        )
            .prop_map(|(partition, region, account, func, exec_id)| {
                format!(
                    "arn:{}:lambda:{}:{}:function:{}:durable:{}",
                    partition, region, account, func, exec_id
                )
            })
    }

    /// Strategy for generating invalid ARN strings (missing required components)
    fn invalid_arn_strategy() -> impl Strategy<Value = String> {
        prop_oneof![
            // Empty string
            Just("".to_string()),
            // Missing arn: prefix
            "[a-zA-Z0-9]{10,30}".prop_map(|s| s),
            // Has arn: but missing lambda
            "[a-zA-Z0-9]{5,20}".prop_map(|s| format!("arn:aws:s3:us-east-1:123456789012:{}", s)),
            // Has arn: and lambda but missing durable
            "[a-zA-Z0-9]{5,20}".prop_map(|s| format!("arn:aws:lambda:us-east-1:123456789012:function:{}", s)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // ==================== OperationId Property Tests ====================

        /// Feature: rust-sdk-test-suite, Property 8: OperationId Validation and Round-Trip
        /// For any non-empty string, OperationId::new() SHALL succeed and round-trip through serde.
        /// **Validates: Requirements 4.1**
        #[test]
        fn prop_operation_id_validation_and_roundtrip(s in non_empty_string_strategy()) {
            // Validation should succeed for non-empty strings
            let id = OperationId::new(&s).expect("non-empty string should be valid");
            
            // Round-trip through serde
            let json = serde_json::to_string(&id).expect("serialization should succeed");
            let deserialized: OperationId = serde_json::from_str(&json).expect("deserialization should succeed");
            
            // Should produce the same value
            prop_assert_eq!(&id, &deserialized);
            prop_assert_eq!(deserialized.as_str(), s.as_str());
        }

        /// Feature: rust-sdk-test-suite, Property 8: OperationId Empty String Validation
        /// For any empty string, OperationId::new() SHALL return ValidationError.
        /// **Validates: Requirements 4.4**
        #[test]
        fn prop_operation_id_empty_string_rejected(_dummy in Just(())) {
            let result = OperationId::new("");
            prop_assert!(result.is_err());
            let err = result.unwrap_err();
            prop_assert_eq!(err.type_name, "OperationId");
            prop_assert!(err.message.contains("empty"));
        }

        /// Feature: rust-sdk-test-suite, Property 8: OperationId From/Into Round-Trip
        /// For any string, creating via From and serializing should round-trip.
        #[test]
        fn prop_operation_id_from_roundtrip(s in ".*") {
            let id = OperationId::from(s.clone());
            
            // Round-trip through serde
            let json = serde_json::to_string(&id).expect("serialization should succeed");
            let deserialized: OperationId = serde_json::from_str(&json).expect("deserialization should succeed");
            
            prop_assert_eq!(&id, &deserialized);
            prop_assert_eq!(deserialized.as_str(), s.as_str());
        }

        // ==================== ExecutionArn Property Tests ====================

        /// Feature: rust-sdk-test-suite, Property 9: ExecutionArn Validation and Round-Trip
        /// For any valid ARN string, ExecutionArn::new() SHALL succeed and round-trip through serde.
        /// **Validates: Requirements 4.2**
        #[test]
        fn prop_execution_arn_validation_and_roundtrip(arn_str in valid_execution_arn_strategy()) {
            // Validation should succeed for valid ARN strings
            let arn = ExecutionArn::new(&arn_str).expect("valid ARN should be accepted");
            
            // Round-trip through serde
            let json = serde_json::to_string(&arn).expect("serialization should succeed");
            let deserialized: ExecutionArn = serde_json::from_str(&json).expect("deserialization should succeed");
            
            // Should produce the same value
            prop_assert_eq!(&arn, &deserialized);
            prop_assert_eq!(deserialized.as_str(), arn_str.as_str());
        }

        /// Feature: rust-sdk-test-suite, Property 9: ExecutionArn Invalid String Validation
        /// For any string not matching ARN pattern, ExecutionArn::new() SHALL return ValidationError.
        /// **Validates: Requirements 4.5**
        #[test]
        fn prop_execution_arn_invalid_rejected(invalid_arn in invalid_arn_strategy()) {
            let result = ExecutionArn::new(&invalid_arn);
            prop_assert!(result.is_err(), "Invalid ARN '{}' should be rejected", invalid_arn);
        }

        /// Feature: rust-sdk-test-suite, Property 9: ExecutionArn From/Into Round-Trip
        /// For any string, creating via From and serializing should round-trip.
        #[test]
        fn prop_execution_arn_from_roundtrip(s in ".*") {
            let arn = ExecutionArn::from(s.clone());
            
            // Round-trip through serde
            let json = serde_json::to_string(&arn).expect("serialization should succeed");
            let deserialized: ExecutionArn = serde_json::from_str(&json).expect("deserialization should succeed");
            
            prop_assert_eq!(&arn, &deserialized);
            prop_assert_eq!(deserialized.as_str(), s.as_str());
        }

        // ==================== CallbackId Property Tests ====================

        /// Feature: rust-sdk-test-suite, Property 10: CallbackId Validation and Round-Trip
        /// For any non-empty string, CallbackId::new() SHALL succeed and round-trip through serde.
        /// **Validates: Requirements 4.3**
        #[test]
        fn prop_callback_id_validation_and_roundtrip(s in non_empty_string_strategy()) {
            // Validation should succeed for non-empty strings
            let id = CallbackId::new(&s).expect("non-empty string should be valid");
            
            // Round-trip through serde
            let json = serde_json::to_string(&id).expect("serialization should succeed");
            let deserialized: CallbackId = serde_json::from_str(&json).expect("deserialization should succeed");
            
            // Should produce the same value
            prop_assert_eq!(&id, &deserialized);
            prop_assert_eq!(deserialized.as_str(), s.as_str());
        }

        /// Feature: rust-sdk-test-suite, Property 10: CallbackId Empty String Validation
        /// For any empty string, CallbackId::new() SHALL return ValidationError.
        /// **Validates: Requirements 4.4 (applies to CallbackId as well)**
        #[test]
        fn prop_callback_id_empty_string_rejected(_dummy in Just(())) {
            let result = CallbackId::new("");
            prop_assert!(result.is_err());
            let err = result.unwrap_err();
            prop_assert_eq!(err.type_name, "CallbackId");
            prop_assert!(err.message.contains("empty"));
        }

        /// Feature: rust-sdk-test-suite, Property 10: CallbackId From/Into Round-Trip
        /// For any string, creating via From and serializing should round-trip.
        #[test]
        fn prop_callback_id_from_roundtrip(s in ".*") {
            let id = CallbackId::from(s.clone());
            
            // Round-trip through serde
            let json = serde_json::to_string(&id).expect("serialization should succeed");
            let deserialized: CallbackId = serde_json::from_str(&json).expect("deserialization should succeed");
            
            prop_assert_eq!(&id, &deserialized);
            prop_assert_eq!(deserialized.as_str(), s.as_str());
        }

        // ==================== HashMap Key Behavior Property Tests ====================

        /// Feature: rust-sdk-test-suite, Property 11: OperationId HashMap Key Behavior
        /// For any OperationId instance, inserting into a HashMap and retrieving by an equal key
        /// SHALL return the same value.
        /// **Validates: Requirements 4.6**
        #[test]
        fn prop_operation_id_hashmap_key(s in non_empty_string_strategy(), value in any::<i32>()) {
            let id1 = OperationId::from(s.clone());
            let id2 = OperationId::from(s.clone());
            
            let mut map: HashMap<OperationId, i32> = HashMap::new();
            map.insert(id1, value);
            
            // Retrieving by equal key should return the same value
            prop_assert_eq!(map.get(&id2), Some(&value));
        }

        /// Feature: rust-sdk-test-suite, Property 11: ExecutionArn HashMap Key Behavior
        /// For any ExecutionArn instance, inserting into a HashMap and retrieving by an equal key
        /// SHALL return the same value.
        /// **Validates: Requirements 4.6**
        #[test]
        fn prop_execution_arn_hashmap_key(arn_str in valid_execution_arn_strategy(), value in any::<i32>()) {
            let arn1 = ExecutionArn::from(arn_str.clone());
            let arn2 = ExecutionArn::from(arn_str.clone());
            
            let mut map: HashMap<ExecutionArn, i32> = HashMap::new();
            map.insert(arn1, value);
            
            // Retrieving by equal key should return the same value
            prop_assert_eq!(map.get(&arn2), Some(&value));
        }

        /// Feature: rust-sdk-test-suite, Property 11: CallbackId HashMap Key Behavior
        /// For any CallbackId instance, inserting into a HashMap and retrieving by an equal key
        /// SHALL return the same value.
        /// **Validates: Requirements 4.6**
        #[test]
        fn prop_callback_id_hashmap_key(s in non_empty_string_strategy(), value in any::<i32>()) {
            let id1 = CallbackId::from(s.clone());
            let id2 = CallbackId::from(s.clone());
            
            let mut map: HashMap<CallbackId, i32> = HashMap::new();
            map.insert(id1, value);
            
            // Retrieving by equal key should return the same value
            prop_assert_eq!(map.get(&id2), Some(&value));
        }

        /// Feature: rust-sdk-test-suite, Property 11: Different keys should not collide
        /// For any two different strings, their newtypes should not be equal and should
        /// not collide in HashMap lookups.
        #[test]
        fn prop_different_keys_no_collision(
            s1 in non_empty_string_strategy(),
            s2 in non_empty_string_strategy(),
            value in any::<i32>()
        ) {
            prop_assume!(s1 != s2);
            
            let id1 = OperationId::from(s1);
            let id2 = OperationId::from(s2);
            
            let mut map: HashMap<OperationId, i32> = HashMap::new();
            map.insert(id1.clone(), value);
            
            // Different key should not find the value
            prop_assert_eq!(map.get(&id2), None);
            // Original key should still find the value
            prop_assert_eq!(map.get(&id1), Some(&value));
        }
    }
}
