//! Error types for the AWS Durable Execution SDK.
//!
//! This module defines a comprehensive error hierarchy for handling
//! different failure modes in durable execution workflows.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The main error type for the AWS Durable Execution SDK.
///
/// This enum covers all possible error conditions that can occur
/// during durable execution workflows.
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
        matches!(self, Self::Checkpoint { is_retriable: true, .. })
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
            Self::Checkpoint { aws_error: Some(aws_error), .. } => {
                aws_error.code == "InvalidParameterValueException" 
                    && aws_error.message.contains("Invalid checkpoint token")
            }
            Self::Checkpoint { message, .. } => {
                message.contains("Invalid checkpoint token")
            }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TerminationReason {
    /// Unhandled error in user code
    #[default]
    UnhandledError,
    /// Error during Lambda invocation
    InvocationError,
    /// Explicit execution error
    ExecutionError,
    /// Checkpoint operation failed
    CheckpointFailed,
    /// Non-deterministic execution detected
    NonDeterministicExecution,
    /// Step was interrupted
    StepInterrupted,
    /// Callback operation failed
    CallbackError,
    /// Serialization/deserialization failed
    SerializationError,
    /// Size limit exceeded (payload, response, or history)
    SizeLimitExceeded,
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
            DurableError::Execution { message, .. } => {
                ErrorObject::new("ExecutionError", message)
            }
            DurableError::Invocation { message, .. } => {
                ErrorObject::new("InvocationError", message)
            }
            DurableError::Checkpoint { message, .. } => {
                ErrorObject::new("CheckpointError", message)
            }
            DurableError::Callback { message, .. } => {
                ErrorObject::new("CallbackError", message)
            }
            DurableError::NonDeterministic { message, .. } => {
                ErrorObject::new("NonDeterministicExecutionError", message)
            }
            DurableError::Validation { message } => {
                ErrorObject::new("ValidationError", message)
            }
            DurableError::SerDes { message } => {
                ErrorObject::new("SerDesError", message)
            }
            DurableError::Suspend { .. } => {
                ErrorObject::new("SuspendExecution", "Execution suspended")
            }
            DurableError::OrphanedChild { message, .. } => {
                ErrorObject::new("OrphanedChildError", message)
            }
            DurableError::UserCode { message, error_type, stack_trace } => {
                let mut obj = ErrorObject::new(error_type, message);
                obj.stack_trace = stack_trace.clone();
                obj
            }
            DurableError::SizeLimit { message, actual_size, max_size } => {
                let detailed_message = match (actual_size, max_size) {
                    (Some(actual), Some(max)) => {
                        format!("{} (actual: {} bytes, max: {} bytes)", message, actual, max)
                    }
                    _ => message.clone(),
                };
                ErrorObject::new("SizeLimitExceededError", detailed_message)
            }
            DurableError::Throttling { message, retry_after_ms } => {
                let detailed_message = match retry_after_ms {
                    Some(ms) => format!("{} (retry after: {}ms)", message, ms),
                    None => message.clone(),
                };
                ErrorObject::new("ThrottlingError", detailed_message)
            }
            DurableError::ResourceNotFound { message, resource_id } => {
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
        if let DurableError::Suspend { scheduled_timestamp } = error {
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
        let error = DurableError::size_limit_with_details("Payload too large", 7_000_000, 6_000_000);
        assert!(error.is_size_limit());
        if let DurableError::SizeLimit { actual_size, max_size, .. } = error {
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
        let error = DurableError::size_limit_with_details("Payload too large", 7_000_000, 6_000_000);
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
}
