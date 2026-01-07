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

    /// Returns true if this is a Checkpoint error that is retriable.
    pub fn is_retriable(&self) -> bool {
        matches!(self, Self::Checkpoint { is_retriable: true, .. })
    }

    /// Returns true if this is a Suspend signal.
    pub fn is_suspend(&self) -> bool {
        matches!(self, Self::Suspend { .. })
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
}
