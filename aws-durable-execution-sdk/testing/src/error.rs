//! Error types for the testing utilities crate.
//!
//! This module defines the error types used throughout the testing framework,
//! wrapping SDK errors and adding testing-specific error variants.

use aws_durable_execution_sdk::{DurableError, OperationType};
use thiserror::Error;

use crate::types::WaitingOperationStatus;

/// Errors that can occur during testing.
///
/// This enum covers all possible error conditions that can occur
/// when using the testing utilities.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::TestError;
///
/// // Operation not found
/// let err = TestError::OperationNotFound("my-step".to_string());
/// assert!(err.to_string().contains("my-step"));
///
/// // Type mismatch
/// let err = TestError::OperationTypeMismatch {
///     expected: aws_durable_execution_sdk::OperationType::Step,
///     found: aws_durable_execution_sdk::OperationType::Wait,
/// };
/// assert!(err.to_string().contains("Step"));
/// ```
#[derive(Debug, Error)]
pub enum TestError {
    /// The handler execution failed.
    #[error("Handler execution failed: {0}")]
    ExecutionFailed(#[from] DurableError),

    /// Operation not found.
    #[error("Operation not found: {0}")]
    OperationNotFound(String),

    /// Wrong operation type for the requested details.
    #[error("Operation type mismatch: expected {expected}, found {found}")]
    OperationTypeMismatch {
        /// The expected operation type
        expected: OperationType,
        /// The actual operation type found
        found: OperationType,
    },

    /// Callback operation required but not found.
    #[error("Not a callback operation")]
    NotCallbackOperation,

    /// Function not registered for invoke.
    #[error("Function not registered: {0}")]
    FunctionNotRegistered(String),

    /// Timeout waiting for operation.
    #[error("Timeout waiting for operation: {0}")]
    WaitTimeout(String),

    /// Execution completed before operation reached expected state.
    #[error("Execution completed before operation {0} reached state {1}")]
    ExecutionCompletedEarly(String, WaitingOperationStatus),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// AWS SDK error.
    #[error("AWS SDK error: {0}")]
    AwsError(String),

    /// Test environment not set up.
    #[error("Test environment not set up. Call setup_test_environment() first.")]
    EnvironmentNotSetUp,

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Result not available (execution not complete or failed).
    #[error("Result not available: {0}")]
    ResultNotAvailable(String),

    /// Invalid checkpoint token.
    #[error("Invalid checkpoint token: {0}")]
    InvalidCheckpointToken(String),

    /// Checkpoint server error.
    #[error("Checkpoint server error: {0}")]
    CheckpointServerError(String),

    /// Checkpoint communication error.
    #[error("Checkpoint communication error: {0}")]
    CheckpointCommunicationError(String),

    /// Execution not found.
    #[error("Execution not found: {0}")]
    ExecutionNotFound(String),

    /// Invocation not found.
    #[error("Invocation not found: {0}")]
    InvocationNotFound(String),

    /// Callback already completed.
    #[error("Callback already completed: {0}")]
    CallbackAlreadyCompleted(String),

    /// Callback not found.
    #[error("Callback not found: {0}")]
    CallbackNotFound(String),
}

impl TestError {
    /// Creates a new OperationNotFound error.
    pub fn operation_not_found(name: impl Into<String>) -> Self {
        Self::OperationNotFound(name.into())
    }

    /// Creates a new OperationTypeMismatch error.
    pub fn type_mismatch(expected: OperationType, found: OperationType) -> Self {
        Self::OperationTypeMismatch { expected, found }
    }

    /// Creates a new FunctionNotRegistered error.
    pub fn function_not_registered(name: impl Into<String>) -> Self {
        Self::FunctionNotRegistered(name.into())
    }

    /// Creates a new WaitTimeout error.
    pub fn wait_timeout(operation: impl Into<String>) -> Self {
        Self::WaitTimeout(operation.into())
    }

    /// Creates a new ExecutionCompletedEarly error.
    pub fn execution_completed_early(
        operation: impl Into<String>,
        status: WaitingOperationStatus,
    ) -> Self {
        Self::ExecutionCompletedEarly(operation.into(), status)
    }

    /// Creates a new AwsError.
    pub fn aws_error(message: impl Into<String>) -> Self {
        Self::AwsError(message.into())
    }

    /// Creates a new InvalidConfiguration error.
    pub fn invalid_configuration(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration(message.into())
    }

    /// Creates a new ResultNotAvailable error.
    pub fn result_not_available(message: impl Into<String>) -> Self {
        Self::ResultNotAvailable(message.into())
    }

    /// Creates a new InvalidCheckpointToken error.
    pub fn invalid_checkpoint_token(message: impl Into<String>) -> Self {
        Self::InvalidCheckpointToken(message.into())
    }

    /// Creates a new CheckpointServerError.
    pub fn checkpoint_server_error(message: impl Into<String>) -> Self {
        Self::CheckpointServerError(message.into())
    }

    /// Creates a new CheckpointCommunicationError.
    pub fn checkpoint_communication_error(message: impl Into<String>) -> Self {
        Self::CheckpointCommunicationError(message.into())
    }

    /// Creates a new ExecutionNotFound error.
    pub fn execution_not_found(message: impl Into<String>) -> Self {
        Self::ExecutionNotFound(message.into())
    }

    /// Creates a new InvocationNotFound error.
    pub fn invocation_not_found(message: impl Into<String>) -> Self {
        Self::InvocationNotFound(message.into())
    }

    /// Creates a new CallbackAlreadyCompleted error.
    pub fn callback_already_completed(message: impl Into<String>) -> Self {
        Self::CallbackAlreadyCompleted(message.into())
    }

    /// Creates a new CallbackNotFound error.
    pub fn callback_not_found(message: impl Into<String>) -> Self {
        Self::CallbackNotFound(message.into())
    }

    /// Returns true if this error is retriable.
    pub fn is_retriable(&self) -> bool {
        match self {
            Self::ExecutionFailed(e) => e.is_retriable(),
            Self::WaitTimeout(_) => true,
            _ => false,
        }
    }
}

/// Result type for testing operations.
pub type TestResult<T> = Result<T, TestError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_not_found() {
        let err = TestError::operation_not_found("my-step");
        assert!(matches!(err, TestError::OperationNotFound(_)));
        assert!(err.to_string().contains("my-step"));
    }

    #[test]
    fn test_type_mismatch() {
        let err = TestError::type_mismatch(OperationType::Step, OperationType::Wait);
        assert!(matches!(err, TestError::OperationTypeMismatch { .. }));
        assert!(err.to_string().contains("Step"));
        assert!(err.to_string().contains("Wait"));
    }

    #[test]
    fn test_not_callback_operation() {
        let err = TestError::NotCallbackOperation;
        assert!(err.to_string().contains("callback"));
    }

    #[test]
    fn test_function_not_registered() {
        let err = TestError::function_not_registered("my-function");
        assert!(matches!(err, TestError::FunctionNotRegistered(_)));
        assert!(err.to_string().contains("my-function"));
    }

    #[test]
    fn test_wait_timeout() {
        let err = TestError::wait_timeout("my-operation");
        assert!(matches!(err, TestError::WaitTimeout(_)));
        assert!(err.to_string().contains("my-operation"));
    }

    #[test]
    fn test_execution_completed_early() {
        let err =
            TestError::execution_completed_early("my-operation", WaitingOperationStatus::Completed);
        assert!(matches!(err, TestError::ExecutionCompletedEarly(_, _)));
        assert!(err.to_string().contains("my-operation"));
        assert!(err.to_string().contains("Completed"));
    }

    #[test]
    fn test_aws_error() {
        let err = TestError::aws_error("Connection failed");
        assert!(matches!(err, TestError::AwsError(_)));
        assert!(err.to_string().contains("Connection failed"));
    }

    #[test]
    fn test_environment_not_set_up() {
        let err = TestError::EnvironmentNotSetUp;
        assert!(err.to_string().contains("setup_test_environment"));
    }

    #[test]
    fn test_from_durable_error() {
        let durable_err = DurableError::validation("Invalid input");
        let test_err: TestError = durable_err.into();
        assert!(matches!(test_err, TestError::ExecutionFailed(_)));
    }

    #[test]
    fn test_from_serde_error() {
        let json_err = serde_json::from_str::<String>("invalid").unwrap_err();
        let test_err: TestError = json_err.into();
        assert!(matches!(test_err, TestError::SerializationError(_)));
    }

    #[test]
    fn test_is_retriable() {
        let timeout_err = TestError::wait_timeout("op");
        assert!(timeout_err.is_retriable());

        let not_found_err = TestError::operation_not_found("op");
        assert!(!not_found_err.is_retriable());

        let retriable_durable =
            TestError::ExecutionFailed(DurableError::checkpoint_retriable("temp error"));
        assert!(retriable_durable.is_retriable());

        let non_retriable_durable =
            TestError::ExecutionFailed(DurableError::validation("bad input"));
        assert!(!non_retriable_durable.is_retriable());
    }
}
