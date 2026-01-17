//! Core types for the testing utilities crate.
//!
//! This module defines the fundamental types used throughout the testing framework,
//! including execution status, error information, and operation waiting states.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Status of a durable execution.
///
/// Represents the overall state of a durable execution test run.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::ExecutionStatus;
///
/// let status = ExecutionStatus::Succeeded;
/// assert!(status.is_terminal());
/// assert!(status.is_success());
///
/// let running = ExecutionStatus::Running;
/// assert!(!running.is_terminal());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Execution is currently running
    Running,
    /// Execution completed successfully
    Succeeded,
    /// Execution failed with an error
    Failed,
    /// Execution was cancelled
    Cancelled,
    /// Execution timed out
    TimedOut,
}

impl ExecutionStatus {
    /// Returns true if this status represents a terminal state.
    ///
    /// Terminal states are those where the execution has completed
    /// and will not change further.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running)
    }

    /// Returns true if this status represents a successful completion.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Returns true if this status represents a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::TimedOut)
    }
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Succeeded => write!(f, "Succeeded"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::TimedOut => write!(f, "TimedOut"),
        }
    }
}

/// Error information from a failed execution.
///
/// Contains detailed information about an error that occurred during
/// a durable execution, including type, message, optional data, and stack trace.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::TestResultError;
///
/// let error = TestResultError::new("ValidationError", "Invalid input");
/// assert_eq!(error.error_type, Some("ValidationError".to_string()));
/// assert_eq!(error.error_message, Some("Invalid input".to_string()));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResultError {
    /// The type/category of the error
    pub error_type: Option<String>,
    /// Human-readable error message
    pub error_message: Option<String>,
    /// Additional error data (typically JSON)
    pub error_data: Option<String>,
    /// Stack trace if available
    pub stack_trace: Option<Vec<String>>,
}

impl TestResultError {
    /// Creates a new TestResultError with the given type and message.
    pub fn new(error_type: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            error_type: Some(error_type.into()),
            error_message: Some(error_message.into()),
            error_data: None,
            stack_trace: None,
        }
    }

    /// Creates a new TestResultError with just a message.
    pub fn from_message(message: impl Into<String>) -> Self {
        Self {
            error_type: None,
            error_message: Some(message.into()),
            error_data: None,
            stack_trace: None,
        }
    }

    /// Sets the error data.
    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.error_data = Some(data.into());
        self
    }

    /// Sets the stack trace.
    pub fn with_stack_trace(mut self, stack_trace: Vec<String>) -> Self {
        self.stack_trace = Some(stack_trace);
        self
    }
}

impl std::fmt::Display for TestResultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.error_type, &self.error_message) {
            (Some(t), Some(m)) => write!(f, "{}: {}", t, m),
            (None, Some(m)) => write!(f, "{}", m),
            (Some(t), None) => write!(f, "{}", t),
            (None, None) => write!(f, "Unknown error"),
        }
    }
}

impl std::error::Error for TestResultError {}

impl From<aws_durable_execution_sdk::ErrorObject> for TestResultError {
    fn from(error: aws_durable_execution_sdk::ErrorObject) -> Self {
        Self {
            error_type: Some(error.error_type),
            error_message: Some(error.error_message),
            error_data: None,
            stack_trace: error.stack_trace.map(|s| vec![s]),
        }
    }
}

/// Information about a single handler invocation.
///
/// Tracks timing and error information for each time the handler
/// was invoked during a durable execution.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::Invocation;
///
/// let invocation = Invocation::new();
/// assert!(invocation.start_timestamp.is_none());
/// assert!(invocation.error.is_none());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    /// Timestamp when the invocation started
    pub start_timestamp: Option<DateTime<Utc>>,
    /// Timestamp when the invocation ended
    pub end_timestamp: Option<DateTime<Utc>>,
    /// Request ID for this invocation (e.g., Lambda request ID)
    pub request_id: Option<String>,
    /// Error information if the invocation failed
    pub error: Option<TestResultError>,
}

impl Invocation {
    /// Creates a new empty Invocation.
    pub fn new() -> Self {
        Self {
            start_timestamp: None,
            end_timestamp: None,
            request_id: None,
            error: None,
        }
    }

    /// Creates a new Invocation with the given start timestamp.
    pub fn with_start(start: DateTime<Utc>) -> Self {
        Self {
            start_timestamp: Some(start),
            end_timestamp: None,
            request_id: None,
            error: None,
        }
    }

    /// Sets the end timestamp.
    pub fn with_end(mut self, end: DateTime<Utc>) -> Self {
        self.end_timestamp = Some(end);
        self
    }

    /// Sets the request ID.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Sets the error.
    pub fn with_error(mut self, error: TestResultError) -> Self {
        self.error = Some(error);
        self
    }

    /// Returns the duration of the invocation if both timestamps are available.
    pub fn duration(&self) -> Option<chrono::Duration> {
        match (&self.start_timestamp, &self.end_timestamp) {
            (Some(start), Some(end)) => Some(*end - *start),
            _ => None,
        }
    }
}

impl Default for Invocation {
    fn default() -> Self {
        Self::new()
    }
}

/// Status to wait for in async operations.
///
/// Used with `wait_for_data()` to specify which state an operation
/// should reach before the wait completes.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::WaitingOperationStatus;
///
/// let status = WaitingOperationStatus::Completed;
/// assert_eq!(format!("{}", status), "Completed");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaitingOperationStatus {
    /// Wait for operation to start
    Started,
    /// Wait for operation to be submitted (callbacks only)
    Submitted,
    /// Wait for operation to complete
    Completed,
}

impl std::fmt::Display for WaitingOperationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => write!(f, "Started"),
            Self::Submitted => write!(f, "Submitted"),
            Self::Completed => write!(f, "Completed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_status_terminal() {
        assert!(!ExecutionStatus::Running.is_terminal());
        assert!(ExecutionStatus::Succeeded.is_terminal());
        assert!(ExecutionStatus::Failed.is_terminal());
        assert!(ExecutionStatus::Cancelled.is_terminal());
        assert!(ExecutionStatus::TimedOut.is_terminal());
    }

    #[test]
    fn test_execution_status_success() {
        assert!(ExecutionStatus::Succeeded.is_success());
        assert!(!ExecutionStatus::Failed.is_success());
        assert!(!ExecutionStatus::Running.is_success());
    }

    #[test]
    fn test_execution_status_failure() {
        assert!(!ExecutionStatus::Succeeded.is_failure());
        assert!(ExecutionStatus::Failed.is_failure());
        assert!(ExecutionStatus::Cancelled.is_failure());
        assert!(ExecutionStatus::TimedOut.is_failure());
        assert!(!ExecutionStatus::Running.is_failure());
    }

    #[test]
    fn test_test_result_error_new() {
        let error = TestResultError::new("TestError", "Test message");
        assert_eq!(error.error_type, Some("TestError".to_string()));
        assert_eq!(error.error_message, Some("Test message".to_string()));
        assert!(error.error_data.is_none());
        assert!(error.stack_trace.is_none());
    }

    #[test]
    fn test_test_result_error_display() {
        let error = TestResultError::new("TestError", "Test message");
        assert_eq!(format!("{}", error), "TestError: Test message");

        let error_no_type = TestResultError::from_message("Just a message");
        assert_eq!(format!("{}", error_no_type), "Just a message");
    }

    #[test]
    fn test_invocation_duration() {
        use chrono::TimeZone;
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 5).unwrap();

        let invocation = Invocation::with_start(start).with_end(end);
        let duration = invocation.duration().unwrap();
        assert_eq!(duration.num_seconds(), 5);
    }

    #[test]
    fn test_waiting_operation_status_display() {
        assert_eq!(format!("{}", WaitingOperationStatus::Started), "Started");
        assert_eq!(
            format!("{}", WaitingOperationStatus::Submitted),
            "Submitted"
        );
        assert_eq!(
            format!("{}", WaitingOperationStatus::Completed),
            "Completed"
        );
    }
}
