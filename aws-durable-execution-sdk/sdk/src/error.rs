//! Error types for the AWS Durable Execution SDK.
//!
//! This module defines a comprehensive error hierarchy for handling
//! different failure modes in durable execution workflows.
//!
//! # Type Aliases
//!
//! This module provides semantic type aliases for common result types:
//!
//! - [`DurableResult<T>`] - General result type for durable operations
//! - [`StepResult<T>`] - Result type for step operations
//! - [`CheckpointResult<T>`] - Result type for checkpoint operations
//!
//! These aliases improve code readability and make function signatures
//! more self-documenting.

use serde::{Deserialize, Serialize};
use thiserror::Error;

// =============================================================================
// Result Type Aliases
// =============================================================================

/// Result type for durable operations.
///
/// This is a type alias for `Result<T, DurableError>`, providing a more
/// semantic and concise way to express the return type of durable operations.
///
/// # Examples
///
/// ```rust
/// use aws_durable_execution_sdk::{DurableResult, DurableError};
///
/// fn process_data(data: &str) -> DurableResult<String> {
///     if data.is_empty() {
///         Err(DurableError::validation("Data cannot be empty"))
///     } else {
///         Ok(data.to_uppercase())
///     }
/// }
/// ```
///
/// # Expanded Form
///
/// ```rust,ignore
/// type DurableResult<T> = Result<T, DurableError>;
/// ```
pub type DurableResult<T> = Result<T, DurableError>;

/// Result type for step operations.
///
/// This is a type alias for `Result<T, DurableError>`, specifically intended
/// for step function return types. Using this alias makes it clear that a
/// function is a step operation within a durable execution.
///
/// # Examples
///
/// ```rust
/// use aws_durable_execution_sdk::{StepResult, DurableError};
///
/// fn validate_input(input: &str) -> StepResult<String> {
///     if input.len() > 100 {
///         Err(DurableError::validation("Input too long"))
///     } else {
///         Ok(input.to_string())
///     }
/// }
/// ```
///
/// # Expanded Form
///
/// ```rust,ignore
/// type StepResult<T> = Result<T, DurableError>;
/// ```
pub type StepResult<T> = Result<T, DurableError>;

/// Result type for checkpoint operations.
///
/// This is a type alias for `Result<T, DurableError>`, specifically intended
/// for checkpoint-related operations. Using this alias makes it clear that a
/// function performs checkpointing within a durable execution.
///
/// # Examples
///
/// ```rust
/// use aws_durable_execution_sdk::{CheckpointResult, DurableError};
///
/// fn save_checkpoint(data: &[u8]) -> CheckpointResult<()> {
///     if data.len() > 1_000_000 {
///         Err(DurableError::size_limit("Checkpoint data too large"))
///     } else {
///         Ok(())
///     }
/// }
/// ```
///
/// # Expanded Form
///
/// ```rust,ignore
/// type CheckpointResult<T> = Result<T, DurableError>;
/// ```
pub type CheckpointResult<T> = Result<T, DurableError>;

/// The main error type for the AWS Durable Execution SDK.
///
/// This enum covers all possible error conditions that can occur
/// during durable execution workflows.
///
/// # Examples
///
/// Creating common error types:
///
/// ```
/// use aws_durable_execution_sdk::DurableError;
///
/// // Execution error (fails workflow without retry)
/// let exec_err = DurableError::execution("Order validation failed");
/// assert!(!exec_err.is_retriable());
///
/// // Validation error
/// let val_err = DurableError::validation("Invalid input format");
///
/// // Retriable checkpoint error
/// let cp_err = DurableError::checkpoint_retriable("Temporary network issue");
/// assert!(cp_err.is_retriable());
/// ```
///
/// Checking error types:
///
/// ```
/// use aws_durable_execution_sdk::DurableError;
///
/// let err = DurableError::size_limit("Payload too large");
/// assert!(err.is_size_limit());
/// assert!(!err.is_throttling());
///
/// let throttle_err = DurableError::throttling("Rate limit exceeded");
/// assert!(throttle_err.is_throttling());
/// ```
#[derive(Debug, Error)]
pub enum DurableError {
    /// Execution error that returns FAILED status without Lambda retry.
    #[error("Execution error: {message}")]
    Execution {
        /// Error message describing what went wrong
        message: String,
        /// The reason for termination
        termination_reason: TerminationReason,
    },

    /// Invocation error that triggers Lambda retry.
    #[error("Invocation error: {message}")]
    Invocation {
        /// Error message describing what went wrong
        message: String,
        /// The reason for termination
        termination_reason: TerminationReason,
    },

    /// Checkpoint error for checkpoint failures.
    #[error("Checkpoint error: {message}")]
    Checkpoint {
        /// Error message describing what went wrong
        message: String,
        /// Whether this error is retriable
        is_retriable: bool,
        /// Optional underlying AWS error details
        aws_error: Option<AwsError>,
    },

    /// Callback error for callback-specific failures.
    #[error("Callback error: {message}")]
    Callback {
        /// Error message describing what went wrong
        message: String,
        /// The callback ID if available
        callback_id: Option<String>,
    },

    /// Non-deterministic execution error for replay mismatches.
    #[error("Non-deterministic execution: {message}")]
    NonDeterministic {
        /// Error message describing the mismatch
        message: String,
        /// The operation ID where the mismatch occurred
        operation_id: Option<String>,
    },

    /// Validation error for invalid configuration or arguments.
    #[error("Validation error: {message}")]
    Validation {
        /// Error message describing the validation failure
        message: String,
    },

    /// Serialization/deserialization error.
    #[error("Serialization error: {message}")]
    SerDes {
        /// Error message describing the serialization failure
        message: String,
    },

    /// Suspend execution signal to pause and return control to Lambda runtime.
    #[error("Suspend execution")]
    Suspend {
        /// Optional timestamp when execution should resume
        scheduled_timestamp: Option<f64>,
    },

    /// Orphaned child error when a child operation's parent has completed.
    #[error("Orphaned child: {message}")]
    OrphanedChild {
        /// Error message describing the orphaned state
        message: String,
        /// The operation ID of the orphaned child
        operation_id: String,
    },

    /// User code error wrapping errors from user-provided closures.
    #[error("User code error: {message}")]
    UserCode {
        /// Error message from the user code
        message: String,
        /// The type of error
        error_type: String,
        /// Optional stack trace
        stack_trace: Option<String>,
    },

    /// Size limit exceeded error for payload size violations.
    ///
    /// This error occurs when:
    /// - Checkpoint payload exceeds the maximum allowed size
    /// - Response payload exceeds Lambda's 6MB limit
    /// - History size exceeds service limits
    ///
    /// This error is NOT retriable - the operation should fail without retry.
    ///
    /// # Requirements
    ///
    /// - 13.9: THE Error_System SHALL provide ErrorObject with ErrorType, ErrorMessage, StackTrace, and ErrorData fields
    /// - 25.6: THE SDK SHALL gracefully handle execution limits by returning clear error messages
    #[error("Size limit exceeded: {message}")]
    SizeLimit {
        /// Error message describing the size limit violation
        message: String,
        /// The actual size that exceeded the limit (in bytes)
        actual_size: Option<usize>,
        /// The maximum allowed size (in bytes)
        max_size: Option<usize>,
    },

    /// Throttling error for rate limit exceeded.
    ///
    /// This error occurs when the AWS API rate limit is exceeded.
    /// This error IS retriable with exponential backoff.
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    #[error("Throttling: {message}")]
    Throttling {
        /// Error message describing the throttling condition
        message: String,
        /// Suggested retry delay in milliseconds (if provided by AWS)
        retry_after_ms: Option<u64>,
    },

    /// Resource not found error for missing executions or operations.
    ///
    /// This error occurs when:
    /// - The durable execution ARN does not exist
    /// - The operation ID is not found
    /// - The Lambda function does not exist
    ///
    /// This error is NOT retriable.
    ///
    /// # Requirements
    ///
    /// - 18.6: THE AWS_Integration SHALL handle ResourceNotFoundException appropriately
    #[error("Resource not found: {message}")]
    ResourceNotFound {
        /// Error message describing what resource was not found
        message: String,
        /// The resource identifier that was not found
        resource_id: Option<String>,
    },
}

impl DurableError {
    /// Creates a new Execution error.
    pub fn execution(message: impl Into<String>) -> Self {
        Self::Execution {
            message: message.into(),
            termination_reason: TerminationReason::ExecutionError,
        }
    }

    /// Creates a new Invocation error.
    pub fn invocation(message: impl Into<String>) -> Self {
        Self::Invocation {
            message: message.into(),
            termination_reason: TerminationReason::InvocationError,
        }
    }

    /// Creates a new retriable Checkpoint error.
    pub fn checkpoint_retriable(message: impl Into<String>) -> Self {
        Self::Checkpoint {
            message: message.into(),
            is_retriable: true,
            aws_error: None,
        }
    }

    /// Creates a new non-retriable Checkpoint error.
    pub fn checkpoint_non_retriable(message: impl Into<String>) -> Self {
        Self::Checkpoint {
            message: message.into(),
            is_retriable: false,
            aws_error: None,
        }
    }

    /// Creates a new Validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    /// Creates a new SerDes error.
    pub fn serdes(message: impl Into<String>) -> Self {
        Self::SerDes {
            message: message.into(),
        }
    }

    /// Creates a new Suspend signal.
    pub fn suspend() -> Self {
        Self::Suspend {
            scheduled_timestamp: None,
        }
    }

    /// Creates a new Suspend signal with a scheduled timestamp.
    pub fn suspend_until(timestamp: f64) -> Self {
        Self::Suspend {
            scheduled_timestamp: Some(timestamp),
        }
    }

    /// Creates a new SizeLimit error.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of the size limit violation
    ///
    /// # Requirements
    ///
    /// - 25.6: THE SDK SHALL gracefully handle execution limits by returning clear error messages
    pub fn size_limit(message: impl Into<String>) -> Self {
        Self::SizeLimit {
            message: message.into(),
            actual_size: None,
            max_size: None,
        }
    }

    /// Creates a new SizeLimit error with size details.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of the size limit violation
    /// * `actual_size` - The actual size that exceeded the limit
    /// * `max_size` - The maximum allowed size
    ///
    /// # Requirements
    ///
    /// - 25.6: THE SDK SHALL gracefully handle execution limits by returning clear error messages
    pub fn size_limit_with_details(
        message: impl Into<String>,
        actual_size: usize,
        max_size: usize,
    ) -> Self {
        Self::SizeLimit {
            message: message.into(),
            actual_size: Some(actual_size),
            max_size: Some(max_size),
        }
    }

    /// Creates a new Throttling error.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of the throttling condition
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    pub fn throttling(message: impl Into<String>) -> Self {
        Self::Throttling {
            message: message.into(),
            retry_after_ms: None,
        }
    }

    /// Creates a new Throttling error with retry delay.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of the throttling condition
    /// * `retry_after_ms` - Suggested retry delay in milliseconds
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    pub fn throttling_with_retry_delay(message: impl Into<String>, retry_after_ms: u64) -> Self {
        Self::Throttling {
            message: message.into(),
            retry_after_ms: Some(retry_after_ms),
        }
    }

    /// Creates a new ResourceNotFound error.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of what resource was not found
    ///
    /// # Requirements
    ///
    /// - 18.6: THE AWS_Integration SHALL handle ResourceNotFoundException appropriately
    pub fn resource_not_found(message: impl Into<String>) -> Self {
        Self::ResourceNotFound {
            message: message.into(),
            resource_id: None,
        }
    }

    /// Creates a new ResourceNotFound error with resource ID.
    ///
    /// # Arguments
    ///
    /// * `message` - Description of what resource was not found
    /// * `resource_id` - The identifier of the resource that was not found
    ///
    /// # Requirements
    ///
    /// - 18.6: THE AWS_Integration SHALL handle ResourceNotFoundException appropriately
    pub fn resource_not_found_with_id(
        message: impl Into<String>,
        resource_id: impl Into<String>,
    ) -> Self {
        Self::ResourceNotFound {
            message: message.into(),
            resource_id: Some(resource_id.into()),
        }
    }

    /// Returns true if this is a Checkpoint error that is retriable.
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            Self::Checkpoint {
                is_retriable: true,
                ..
            }
        )
    }

    /// Returns true if this is a Suspend signal.
    pub fn is_suspend(&self) -> bool {
        matches!(self, Self::Suspend { .. })
    }

    /// Returns true if this is an invalid checkpoint token error.
    ///
    /// Invalid checkpoint token errors occur when:
    /// - The token has already been consumed by a previous checkpoint
    /// - The token is malformed or expired
    ///
    /// These errors are retriable because Lambda will provide a fresh token
    /// on the next invocation.
    ///
    /// # Requirements
    ///
    /// - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry
    pub fn is_invalid_checkpoint_token(&self) -> bool {
        match self {
            Self::Checkpoint {
                aws_error: Some(aws_error),
                ..
            } => {
                aws_error.code == "InvalidParameterValueException"
                    && aws_error.message.contains("Invalid checkpoint token")
            }
            Self::Checkpoint { message, .. } => message.contains("Invalid checkpoint token"),
            _ => false,
        }
    }

    /// Returns true if this is a SizeLimit error.
    ///
    /// Size limit errors are NOT retriable - the operation should fail.
    pub fn is_size_limit(&self) -> bool {
        matches!(self, Self::SizeLimit { .. })
    }

    /// Returns true if this is a Throttling error.
    ///
    /// Throttling errors ARE retriable with exponential backoff.
    pub fn is_throttling(&self) -> bool {
        matches!(self, Self::Throttling { .. })
    }

    /// Returns true if this is a ResourceNotFound error.
    ///
    /// Resource not found errors are NOT retriable.
    pub fn is_resource_not_found(&self) -> bool {
        matches!(self, Self::ResourceNotFound { .. })
    }

    /// Returns the suggested retry delay for throttling errors.
    ///
    /// Returns `None` if this is not a throttling error or if no retry delay was provided.
    pub fn get_retry_after_ms(&self) -> Option<u64> {
        match self {
            Self::Throttling { retry_after_ms, .. } => *retry_after_ms,
            _ => None,
        }
    }
}

/// Reason for execution termination.
///
/// This enum uses `#[repr(u8)]` for compact memory representation (1 byte).
/// Explicit discriminant values ensure stability across versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum TerminationReason {
    /// Unhandled error in user code
    #[default]
    UnhandledError = 0,
    /// Error during Lambda invocation
    InvocationError = 1,
    /// Explicit execution error
    ExecutionError = 2,
    /// Checkpoint operation failed
    CheckpointFailed = 3,
    /// Non-deterministic execution detected
    NonDeterministicExecution = 4,
    /// Step was interrupted
    StepInterrupted = 5,
    /// Callback operation failed
    CallbackError = 6,
    /// Serialization/deserialization failed
    SerializationError = 7,
    /// Size limit exceeded (payload, response, or history)
    SizeLimitExceeded = 8,
}

/// AWS error details for checkpoint failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsError {
    /// The AWS error code
    pub code: String,
    /// The AWS error message
    pub message: String,
    /// The request ID if available
    pub request_id: Option<String>,
}

/// Error object for serialization in Lambda responses.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk::ErrorObject;
///
/// // Basic error object
/// let err = ErrorObject::new("ValidationError", "Invalid input");
/// assert_eq!(err.error_type, "ValidationError");
/// assert_eq!(err.error_message, "Invalid input");
/// assert!(err.stack_trace.is_none());
///
/// // With stack trace
/// let err_with_trace = ErrorObject::with_stack_trace(
///     "RuntimeError",
///     "Something went wrong",
///     "at main.rs:42"
/// );
/// assert!(err_with_trace.stack_trace.is_some());
/// ```
///
/// Serialization:
///
/// ```
/// use aws_durable_execution_sdk::ErrorObject;
///
/// let err = ErrorObject::new("TestError", "Test message");
/// let json = serde_json::to_string(&err).unwrap();
/// assert!(json.contains("ErrorType"));
/// assert!(json.contains("ErrorMessage"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    /// The error type/name
    #[serde(rename = "ErrorType")]
    pub error_type: String,
    /// The error message
    #[serde(rename = "ErrorMessage")]
    pub error_message: String,
    /// Optional stack trace
    #[serde(rename = "StackTrace", skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
}

impl ErrorObject {
    /// Creates a new ErrorObject.
    pub fn new(error_type: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            error_type: error_type.into(),
            error_message: error_message.into(),
            stack_trace: None,
        }
    }

    /// Creates a new ErrorObject with a stack trace.
    pub fn with_stack_trace(
        error_type: impl Into<String>,
        error_message: impl Into<String>,
        stack_trace: impl Into<String>,
    ) -> Self {
        Self {
            error_type: error_type.into(),
            error_message: error_message.into(),
            stack_trace: Some(stack_trace.into()),
        }
    }
}

impl From<&DurableError> for ErrorObject {
    fn from(error: &DurableError) -> Self {
        match error {
            DurableError::Execution { message, .. } => ErrorObject::new("ExecutionError", message),
            DurableError::Invocation { message, .. } => {
                ErrorObject::new("InvocationError", message)
            }
            DurableError::Checkpoint { message, .. } => {
                ErrorObject::new("CheckpointError", message)
            }
            DurableError::Callback { message, .. } => ErrorObject::new("CallbackError", message),
            DurableError::NonDeterministic { message, .. } => {
                ErrorObject::new("NonDeterministicExecutionError", message)
            }
            DurableError::Validation { message } => ErrorObject::new("ValidationError", message),
            DurableError::SerDes { message } => ErrorObject::new("SerDesError", message),
            DurableError::Suspend { .. } => {
                ErrorObject::new("SuspendExecution", "Execution suspended")
            }
            DurableError::OrphanedChild { message, .. } => {
                ErrorObject::new("OrphanedChildError", message)
            }
            DurableError::UserCode {
                message,
                error_type,
                stack_trace,
            } => {
                let mut obj = ErrorObject::new(error_type, message);
                obj.stack_trace = stack_trace.clone();
                obj
            }
            DurableError::SizeLimit {
                message,
                actual_size,
                max_size,
            } => {
                let detailed_message = match (actual_size, max_size) {
                    (Some(actual), Some(max)) => {
                        format!("{} (actual: {} bytes, max: {} bytes)", message, actual, max)
                    }
                    _ => message.clone(),
                };
                ErrorObject::new("SizeLimitExceededError", detailed_message)
            }
            DurableError::Throttling {
                message,
                retry_after_ms,
            } => {
                let detailed_message = match retry_after_ms {
                    Some(ms) => format!("{} (retry after: {}ms)", message, ms),
                    None => message.clone(),
                };
                ErrorObject::new("ThrottlingError", detailed_message)
            }
            DurableError::ResourceNotFound {
                message,
                resource_id,
            } => {
                let detailed_message = match resource_id {
                    Some(id) => format!("{} (resource: {})", message, id),
                    None => message.clone(),
                };
                ErrorObject::new("ResourceNotFoundError", detailed_message)
            }
        }
    }
}

// Implement From conversions for common error types

impl From<serde_json::Error> for DurableError {
    fn from(error: serde_json::Error) -> Self {
        Self::SerDes {
            message: error.to_string(),
        }
    }
}

impl From<std::io::Error> for DurableError {
    fn from(error: std::io::Error) -> Self {
        Self::Execution {
            message: error.to_string(),
            termination_reason: TerminationReason::UnhandledError,
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for DurableError {
    fn from(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::UserCode {
            message: error.to_string(),
            error_type: "UserCodeError".to_string(),
            stack_trace: None,
        }
    }
}

impl From<Box<dyn std::error::Error>> for DurableError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self::UserCode {
            message: error.to_string(),
            error_type: "UserCodeError".to_string(),
            stack_trace: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ============================================================================
    // Proptest Strategies
    // ============================================================================

    /// Strategy for generating non-empty strings (for error messages)
    fn non_empty_string_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_ ]{1,64}".prop_map(|s| s)
    }

    /// Strategy for generating optional non-empty strings
    fn optional_string_strategy() -> impl Strategy<Value = Option<String>> {
        prop_oneof![Just(None), non_empty_string_strategy().prop_map(Some),]
    }

    /// Strategy for generating optional usize values
    fn optional_usize_strategy() -> impl Strategy<Value = Option<usize>> {
        prop_oneof![Just(None), (1usize..10_000_000usize).prop_map(Some),]
    }

    /// Strategy for generating optional u64 values (for retry_after_ms)
    fn optional_u64_strategy() -> impl Strategy<Value = Option<u64>> {
        prop_oneof![Just(None), (1u64..100_000u64).prop_map(Some),]
    }

    /// Strategy for generating DurableError::Execution variants
    fn execution_error_strategy() -> impl Strategy<Value = DurableError> {
        non_empty_string_strategy().prop_map(|message| DurableError::Execution {
            message,
            termination_reason: TerminationReason::ExecutionError,
        })
    }

    /// Strategy for generating DurableError::Invocation variants
    fn invocation_error_strategy() -> impl Strategy<Value = DurableError> {
        non_empty_string_strategy().prop_map(|message| DurableError::Invocation {
            message,
            termination_reason: TerminationReason::InvocationError,
        })
    }

    /// Strategy for generating DurableError::Checkpoint variants
    fn checkpoint_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), any::<bool>()).prop_map(|(message, is_retriable)| {
            DurableError::Checkpoint {
                message,
                is_retriable,
                aws_error: None,
            }
        })
    }

    /// Strategy for generating DurableError::Callback variants
    fn callback_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), optional_string_strategy()).prop_map(
            |(message, callback_id)| DurableError::Callback {
                message,
                callback_id,
            },
        )
    }

    /// Strategy for generating DurableError::NonDeterministic variants
    fn non_deterministic_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), optional_string_strategy()).prop_map(
            |(message, operation_id)| DurableError::NonDeterministic {
                message,
                operation_id,
            },
        )
    }

    /// Strategy for generating DurableError::Validation variants
    fn validation_error_strategy() -> impl Strategy<Value = DurableError> {
        non_empty_string_strategy().prop_map(|message| DurableError::Validation { message })
    }

    /// Strategy for generating DurableError::SerDes variants
    fn serdes_error_strategy() -> impl Strategy<Value = DurableError> {
        non_empty_string_strategy().prop_map(|message| DurableError::SerDes { message })
    }

    /// Strategy for generating DurableError::Suspend variants
    fn suspend_error_strategy() -> impl Strategy<Value = DurableError> {
        prop_oneof![
            // None case - use prop_map to avoid Clone requirement
            Just(()).prop_map(|_| DurableError::Suspend {
                scheduled_timestamp: None
            }),
            (0.0f64..1e15f64).prop_map(|ts| DurableError::Suspend {
                scheduled_timestamp: Some(ts)
            }),
        ]
    }

    /// Strategy for generating DurableError::OrphanedChild variants
    fn orphaned_child_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), non_empty_string_strategy()).prop_map(
            |(message, operation_id)| DurableError::OrphanedChild {
                message,
                operation_id,
            },
        )
    }

    /// Strategy for generating DurableError::UserCode variants
    fn user_code_error_strategy() -> impl Strategy<Value = DurableError> {
        (
            non_empty_string_strategy(),
            non_empty_string_strategy(),
            optional_string_strategy(),
        )
            .prop_map(
                |(message, error_type, stack_trace)| DurableError::UserCode {
                    message,
                    error_type,
                    stack_trace,
                },
            )
    }

    /// Strategy for generating DurableError::SizeLimit variants
    fn size_limit_error_strategy() -> impl Strategy<Value = DurableError> {
        (
            non_empty_string_strategy(),
            optional_usize_strategy(),
            optional_usize_strategy(),
        )
            .prop_map(|(message, actual_size, max_size)| DurableError::SizeLimit {
                message,
                actual_size,
                max_size,
            })
    }

    /// Strategy for generating DurableError::Throttling variants
    fn throttling_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), optional_u64_strategy()).prop_map(
            |(message, retry_after_ms)| DurableError::Throttling {
                message,
                retry_after_ms,
            },
        )
    }

    /// Strategy for generating DurableError::ResourceNotFound variants
    fn resource_not_found_error_strategy() -> impl Strategy<Value = DurableError> {
        (non_empty_string_strategy(), optional_string_strategy()).prop_map(
            |(message, resource_id)| DurableError::ResourceNotFound {
                message,
                resource_id,
            },
        )
    }

    /// Strategy for generating any DurableError variant
    fn durable_error_strategy() -> impl Strategy<Value = DurableError> {
        prop_oneof![
            execution_error_strategy(),
            invocation_error_strategy(),
            checkpoint_error_strategy(),
            callback_error_strategy(),
            non_deterministic_error_strategy(),
            validation_error_strategy(),
            serdes_error_strategy(),
            suspend_error_strategy(),
            orphaned_child_error_strategy(),
            user_code_error_strategy(),
            size_limit_error_strategy(),
            throttling_error_strategy(),
            resource_not_found_error_strategy(),
        ]
    }

    // ============================================================================
    // Property-Based Tests
    // ============================================================================

    proptest! {
        /// Feature: rust-sdk-test-suite, Property 6: DurableError to ErrorObject Conversion
        /// For any DurableError variant, converting to ErrorObject SHALL produce an ErrorObject
        /// with non-empty error_type and error_message fields.
        /// **Validates: Requirements 3.1, 3.2**
        #[test]
        fn prop_durable_error_to_error_object_produces_valid_fields(error in durable_error_strategy()) {
            let error_object: ErrorObject = (&error).into();

            // Verify error_type is non-empty
            prop_assert!(
                !error_object.error_type.is_empty(),
                "ErrorObject.error_type should be non-empty for {:?}",
                error
            );

            // Verify error_message is non-empty
            prop_assert!(
                !error_object.error_message.is_empty(),
                "ErrorObject.error_message should be non-empty for {:?}",
                error
            );
        }

        /// Feature: rust-sdk-test-suite, Property 7: Error Type Classification Consistency (SizeLimit)
        /// For any SizeLimit error, is_size_limit() SHALL return true and is_retriable() SHALL return false.
        /// **Validates: Requirements 3.3**
        #[test]
        fn prop_size_limit_error_classification(error in size_limit_error_strategy()) {
            prop_assert!(
                error.is_size_limit(),
                "SizeLimit error should return true for is_size_limit()"
            );
            prop_assert!(
                !error.is_retriable(),
                "SizeLimit error should return false for is_retriable()"
            );
            prop_assert!(
                !error.is_throttling(),
                "SizeLimit error should return false for is_throttling()"
            );
            prop_assert!(
                !error.is_resource_not_found(),
                "SizeLimit error should return false for is_resource_not_found()"
            );
        }

        /// Feature: rust-sdk-test-suite, Property 7: Error Type Classification Consistency (Throttling)
        /// For any Throttling error, is_throttling() SHALL return true.
        /// **Validates: Requirements 3.4**
        #[test]
        fn prop_throttling_error_classification(error in throttling_error_strategy()) {
            prop_assert!(
                error.is_throttling(),
                "Throttling error should return true for is_throttling()"
            );
            prop_assert!(
                !error.is_size_limit(),
                "Throttling error should return false for is_size_limit()"
            );
            prop_assert!(
                !error.is_resource_not_found(),
                "Throttling error should return false for is_resource_not_found()"
            );
        }

        /// Feature: rust-sdk-test-suite, Property 7: Error Type Classification Consistency (ResourceNotFound)
        /// For any ResourceNotFound error, is_resource_not_found() SHALL return true and is_retriable() SHALL return false.
        /// **Validates: Requirements 3.5**
        #[test]
        fn prop_resource_not_found_error_classification(error in resource_not_found_error_strategy()) {
            prop_assert!(
                error.is_resource_not_found(),
                "ResourceNotFound error should return true for is_resource_not_found()"
            );
            prop_assert!(
                !error.is_retriable(),
                "ResourceNotFound error should return false for is_retriable()"
            );
            prop_assert!(
                !error.is_size_limit(),
                "ResourceNotFound error should return false for is_size_limit()"
            );
            prop_assert!(
                !error.is_throttling(),
                "ResourceNotFound error should return false for is_throttling()"
            );
        }

        /// Feature: rust-sdk-test-suite, Property 7: Error Type Classification Consistency (Retriable Checkpoint)
        /// For any Checkpoint error with is_retriable=true, is_retriable() SHALL return true.
        /// **Validates: Requirements 3.6**
        #[test]
        fn prop_retriable_checkpoint_error_classification(message in non_empty_string_strategy()) {
            let error = DurableError::Checkpoint {
                message,
                is_retriable: true,
                aws_error: None,
            };
            prop_assert!(
                error.is_retriable(),
                "Checkpoint error with is_retriable=true should return true for is_retriable()"
            );
        }

        /// Feature: rust-sdk-test-suite, Property 7: Error Type Classification Consistency (Non-Retriable Checkpoint)
        /// For any Checkpoint error with is_retriable=false, is_retriable() SHALL return false.
        /// **Validates: Requirements 3.6**
        #[test]
        fn prop_non_retriable_checkpoint_error_classification(message in non_empty_string_strategy()) {
            let error = DurableError::Checkpoint {
                message,
                is_retriable: false,
                aws_error: None,
            };
            prop_assert!(
                !error.is_retriable(),
                "Checkpoint error with is_retriable=false should return false for is_retriable()"
            );
        }

        /// Feature: rust-sdk-test-suite, Property 6: ErrorObject Field Validation
        /// For any ErrorObject created from DurableError, the error_type field SHALL match
        /// the expected type name for that error variant.
        /// **Validates: Requirements 3.1**
        #[test]
        fn prop_error_object_type_matches_variant(error in durable_error_strategy()) {
            let error_object: ErrorObject = (&error).into();

            let expected_type = match &error {
                DurableError::Execution { .. } => "ExecutionError",
                DurableError::Invocation { .. } => "InvocationError",
                DurableError::Checkpoint { .. } => "CheckpointError",
                DurableError::Callback { .. } => "CallbackError",
                DurableError::NonDeterministic { .. } => "NonDeterministicExecutionError",
                DurableError::Validation { .. } => "ValidationError",
                DurableError::SerDes { .. } => "SerDesError",
                DurableError::Suspend { .. } => "SuspendExecution",
                DurableError::OrphanedChild { .. } => "OrphanedChildError",
                DurableError::UserCode { error_type, .. } => error_type.as_str(),
                DurableError::SizeLimit { .. } => "SizeLimitExceededError",
                DurableError::Throttling { .. } => "ThrottlingError",
                DurableError::ResourceNotFound { .. } => "ResourceNotFoundError",
            };

            prop_assert_eq!(
                error_object.error_type,
                expected_type,
                "ErrorObject.error_type should match expected type for {:?}",
                error
            );
        }
    }

    // ============================================================================
    // Unit Tests
    // ============================================================================

    #[test]
    fn test_execution_error() {
        let error = DurableError::execution("test error");
        assert!(matches!(error, DurableError::Execution { .. }));
        assert!(!error.is_retriable());
        assert!(!error.is_suspend());
    }

    #[test]
    fn test_checkpoint_retriable() {
        let error = DurableError::checkpoint_retriable("test error");
        assert!(error.is_retriable());
    }

    #[test]
    fn test_checkpoint_non_retriable() {
        let error = DurableError::checkpoint_non_retriable("test error");
        assert!(!error.is_retriable());
    }

    #[test]
    fn test_suspend() {
        let error = DurableError::suspend();
        assert!(error.is_suspend());
    }

    #[test]
    fn test_suspend_until() {
        let error = DurableError::suspend_until(1234567890.0);
        assert!(error.is_suspend());
        if let DurableError::Suspend {
            scheduled_timestamp,
        } = error
        {
            assert_eq!(scheduled_timestamp, Some(1234567890.0));
        }
    }

    #[test]
    fn test_error_object_from_durable_error() {
        let error = DurableError::validation("invalid input");
        let obj: ErrorObject = (&error).into();
        assert_eq!(obj.error_type, "ValidationError");
        assert_eq!(obj.error_message, "invalid input");
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_error = serde_json::from_str::<String>("invalid").unwrap_err();
        let error: DurableError = json_error.into();
        assert!(matches!(error, DurableError::SerDes { .. }));
    }

    #[test]
    fn test_is_invalid_checkpoint_token_with_aws_error() {
        let error = DurableError::Checkpoint {
            message: "Checkpoint API returned 400: Invalid checkpoint token".to_string(),
            is_retriable: true,
            aws_error: Some(AwsError {
                code: "InvalidParameterValueException".to_string(),
                message: "Invalid checkpoint token: token has been consumed".to_string(),
                request_id: None,
            }),
        };
        assert!(error.is_invalid_checkpoint_token());
        assert!(error.is_retriable());
    }

    #[test]
    fn test_is_invalid_checkpoint_token_without_aws_error() {
        let error = DurableError::Checkpoint {
            message: "Invalid checkpoint token: token expired".to_string(),
            is_retriable: true,
            aws_error: None,
        };
        assert!(error.is_invalid_checkpoint_token());
    }

    #[test]
    fn test_is_not_invalid_checkpoint_token() {
        let error = DurableError::Checkpoint {
            message: "Network error".to_string(),
            is_retriable: true,
            aws_error: None,
        };
        assert!(!error.is_invalid_checkpoint_token());
    }

    #[test]
    fn test_is_invalid_checkpoint_token_wrong_error_type() {
        let error = DurableError::Validation {
            message: "Invalid checkpoint token".to_string(),
        };
        assert!(!error.is_invalid_checkpoint_token());
    }

    #[test]
    fn test_is_invalid_checkpoint_token_wrong_aws_error_code() {
        let error = DurableError::Checkpoint {
            message: "Some error".to_string(),
            is_retriable: false,
            aws_error: Some(AwsError {
                code: "ServiceException".to_string(),
                message: "Invalid checkpoint token".to_string(),
                request_id: None,
            }),
        };
        // Should be false because the code is not InvalidParameterValueException
        assert!(!error.is_invalid_checkpoint_token());
    }

    #[test]
    fn test_size_limit_error() {
        let error = DurableError::size_limit("Payload too large");
        assert!(error.is_size_limit());
        assert!(!error.is_retriable());
        assert!(!error.is_throttling());
        assert!(!error.is_resource_not_found());
    }

    #[test]
    fn test_size_limit_error_with_details() {
        let error =
            DurableError::size_limit_with_details("Payload too large", 7_000_000, 6_000_000);
        assert!(error.is_size_limit());
        if let DurableError::SizeLimit {
            actual_size,
            max_size,
            ..
        } = error
        {
            assert_eq!(actual_size, Some(7_000_000));
            assert_eq!(max_size, Some(6_000_000));
        } else {
            panic!("Expected SizeLimit error");
        }
    }

    #[test]
    fn test_throttling_error() {
        let error = DurableError::throttling("Rate limit exceeded");
        assert!(error.is_throttling());
        assert!(!error.is_retriable());
        assert!(!error.is_size_limit());
        assert!(!error.is_resource_not_found());
        assert_eq!(error.get_retry_after_ms(), None);
    }

    #[test]
    fn test_throttling_error_with_retry_delay() {
        let error = DurableError::throttling_with_retry_delay("Rate limit exceeded", 5000);
        assert!(error.is_throttling());
        assert_eq!(error.get_retry_after_ms(), Some(5000));
    }

    #[test]
    fn test_resource_not_found_error() {
        let error = DurableError::resource_not_found("Execution not found");
        assert!(error.is_resource_not_found());
        assert!(!error.is_retriable());
        assert!(!error.is_size_limit());
        assert!(!error.is_throttling());
    }

    #[test]
    fn test_resource_not_found_error_with_id() {
        let error = DurableError::resource_not_found_with_id(
            "Execution not found",
            "arn:aws:lambda:us-east-1:123456789012:function:test",
        );
        assert!(error.is_resource_not_found());
        if let DurableError::ResourceNotFound { resource_id, .. } = error {
            assert_eq!(
                resource_id,
                Some("arn:aws:lambda:us-east-1:123456789012:function:test".to_string())
            );
        } else {
            panic!("Expected ResourceNotFound error");
        }
    }

    #[test]
    fn test_error_object_from_size_limit_error() {
        let error =
            DurableError::size_limit_with_details("Payload too large", 7_000_000, 6_000_000);
        let obj: ErrorObject = (&error).into();
        assert_eq!(obj.error_type, "SizeLimitExceededError");
        assert!(obj.error_message.contains("Payload too large"));
        assert!(obj.error_message.contains("7000000"));
        assert!(obj.error_message.contains("6000000"));
    }

    #[test]
    fn test_error_object_from_throttling_error() {
        let error = DurableError::throttling_with_retry_delay("Rate limit exceeded", 5000);
        let obj: ErrorObject = (&error).into();
        assert_eq!(obj.error_type, "ThrottlingError");
        assert!(obj.error_message.contains("Rate limit exceeded"));
        assert!(obj.error_message.contains("5000ms"));
    }

    #[test]
    fn test_error_object_from_resource_not_found_error() {
        let error = DurableError::resource_not_found_with_id("Execution not found", "test-arn");
        let obj: ErrorObject = (&error).into();
        assert_eq!(obj.error_type, "ResourceNotFoundError");
        assert!(obj.error_message.contains("Execution not found"));
        assert!(obj.error_message.contains("test-arn"));
    }

    #[test]
    fn test_get_retry_after_ms_non_throttling() {
        let error = DurableError::validation("test");
        assert_eq!(error.get_retry_after_ms(), None);
    }

    // Size verification test for enum discriminant optimization
    // Requirements: 6.7 - Verify TerminationReason is 1 byte after optimization

    #[test]
    fn test_termination_reason_size_is_one_byte() {
        assert_eq!(
            std::mem::size_of::<TerminationReason>(),
            1,
            "TerminationReason should be 1 byte with #[repr(u8)]"
        );
    }

    // Serde compatibility tests for enum discriminant optimization
    // Requirements: 6.6 - Verify JSON serialization uses string representations

    #[test]
    fn test_termination_reason_serde_uses_string_representation() {
        // Verify serialization produces string values, not numeric discriminants
        let reason = TerminationReason::UnhandledError;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"UnhandledError\"");

        let reason = TerminationReason::InvocationError;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"InvocationError\"");

        let reason = TerminationReason::ExecutionError;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"ExecutionError\"");

        let reason = TerminationReason::CheckpointFailed;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"CheckpointFailed\"");

        let reason = TerminationReason::NonDeterministicExecution;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"NonDeterministicExecution\"");

        let reason = TerminationReason::StepInterrupted;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"StepInterrupted\"");

        let reason = TerminationReason::CallbackError;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"CallbackError\"");

        let reason = TerminationReason::SerializationError;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"SerializationError\"");

        let reason = TerminationReason::SizeLimitExceeded;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, "\"SizeLimitExceeded\"");
    }

    #[test]
    fn test_termination_reason_serde_round_trip() {
        let reasons = [
            TerminationReason::UnhandledError,
            TerminationReason::InvocationError,
            TerminationReason::ExecutionError,
            TerminationReason::CheckpointFailed,
            TerminationReason::NonDeterministicExecution,
            TerminationReason::StepInterrupted,
            TerminationReason::CallbackError,
            TerminationReason::SerializationError,
            TerminationReason::SizeLimitExceeded,
        ];

        for reason in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            let deserialized: TerminationReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, deserialized, "Round-trip failed for {:?}", reason);
        }
    }
}
