//! Operation types for the AWS Durable Execution SDK.
//!
//! This module defines the core operation types used for checkpointing
//! and replay in durable execution workflows.

use serde::{Deserialize, Serialize};

use crate::error::ErrorObject;

/// Represents a checkpointed operation in a durable execution.
///
/// Operations are the fundamental unit of state in durable executions.
/// Each operation has a unique ID and tracks its type, status, and result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    /// Unique identifier for this operation
    #[serde(rename = "OperationId")]
    pub operation_id: String,

    /// The type of operation (Step, Wait, Callback, etc.)
    #[serde(rename = "OperationType")]
    pub operation_type: OperationType,

    /// Current status of the operation
    #[serde(rename = "Status")]
    pub status: OperationStatus,

    /// Serialized result if the operation succeeded
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error details if the operation failed
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,

    /// Parent operation ID for nested operations
    #[serde(rename = "ParentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Optional human-readable name for the operation
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Operation {
    /// Creates a new Operation with the given ID and type.
    pub fn new(operation_id: impl Into<String>, operation_type: OperationType) -> Self {
        Self {
            operation_id: operation_id.into(),
            operation_type,
            status: OperationStatus::Started,
            result: None,
            error: None,
            parent_id: None,
            name: None,
        }
    }

    /// Sets the parent ID for this operation.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the name for this operation.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Returns true if the operation has completed (succeeded or failed).
    pub fn is_completed(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Succeeded
                | OperationStatus::Failed
                | OperationStatus::Cancelled
                | OperationStatus::TimedOut
                | OperationStatus::Stopped
        )
    }

    /// Returns true if the operation succeeded.
    pub fn is_succeeded(&self) -> bool {
        matches!(self.status, OperationStatus::Succeeded)
    }

    /// Returns true if the operation failed.
    pub fn is_failed(&self) -> bool {
        matches!(
            self.status,
            OperationStatus::Failed | OperationStatus::Cancelled | OperationStatus::TimedOut
        )
    }
}


/// The type of operation in a durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationType {
    /// The root execution operation
    Execution,
    /// A step operation (unit of work)
    Step,
    /// A wait/sleep operation
    Wait,
    /// A callback operation waiting for external signal
    Callback,
    /// An invoke operation calling another Lambda function
    Invoke,
    /// A context operation for nested child contexts
    Context,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Execution => write!(f, "Execution"),
            Self::Step => write!(f, "Step"),
            Self::Wait => write!(f, "Wait"),
            Self::Callback => write!(f, "Callback"),
            Self::Invoke => write!(f, "Invoke"),
            Self::Context => write!(f, "Context"),
        }
    }
}

/// The status of an operation in a durable execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationStatus {
    /// Operation has started but not completed
    Started,
    /// Operation completed successfully
    Succeeded,
    /// Operation failed with an error
    Failed,
    /// Operation was cancelled
    Cancelled,
    /// Operation timed out
    TimedOut,
    /// Operation was stopped externally
    Stopped,
}

impl OperationStatus {
    /// Returns true if this status represents a terminal state.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Started)
    }

    /// Returns true if this status represents a successful completion.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Returns true if this status represents a failure.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled | Self::TimedOut | Self::Stopped)
    }
}

impl std::fmt::Display for OperationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => write!(f, "Started"),
            Self::Succeeded => write!(f, "Succeeded"),
            Self::Failed => write!(f, "Failed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::TimedOut => write!(f, "TimedOut"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Action to perform on an operation during checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationAction {
    /// Start a new operation
    Start,
    /// Mark operation as succeeded
    Succeed,
    /// Mark operation as failed
    Fail,
}

impl std::fmt::Display for OperationAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Start => write!(f, "Start"),
            Self::Succeed => write!(f, "Succeed"),
            Self::Fail => write!(f, "Fail"),
        }
    }
}

/// Represents an update to be checkpointed for an operation.
///
/// This struct is used to send checkpoint requests to the Lambda service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationUpdate {
    /// Unique identifier for this operation
    #[serde(rename = "OperationId")]
    pub operation_id: String,

    /// The action to perform (Start, Succeed, Fail)
    #[serde(rename = "Action")]
    pub action: OperationAction,

    /// The type of operation
    #[serde(rename = "OperationType")]
    pub operation_type: OperationType,

    /// Serialized result if succeeding
    #[serde(rename = "Result", skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Error details if failing
    #[serde(rename = "Error", skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,

    /// Parent operation ID for nested operations
    #[serde(rename = "ParentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,

    /// Optional human-readable name for the operation
    #[serde(rename = "Name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl OperationUpdate {
    /// Creates a new OperationUpdate to start an operation.
    pub fn start(
        operation_id: impl Into<String>,
        operation_type: OperationType,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Start,
            operation_type,
            result: None,
            error: None,
            parent_id: None,
            name: None,
        }
    }

    /// Creates a new OperationUpdate to mark an operation as succeeded.
    pub fn succeed(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        result: Option<String>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Succeed,
            operation_type,
            result,
            error: None,
            parent_id: None,
            name: None,
        }
    }

    /// Creates a new OperationUpdate to mark an operation as failed.
    pub fn fail(
        operation_id: impl Into<String>,
        operation_type: OperationType,
        error: ErrorObject,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            action: OperationAction::Fail,
            operation_type,
            result: None,
            error: Some(error),
            parent_id: None,
            name: None,
        }
    }

    /// Sets the parent ID for this operation update.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the name for this operation update.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_new() {
        let op = Operation::new("op-123", OperationType::Step);
        assert_eq!(op.operation_id, "op-123");
        assert_eq!(op.operation_type, OperationType::Step);
        assert_eq!(op.status, OperationStatus::Started);
        assert!(op.result.is_none());
        assert!(op.error.is_none());
        assert!(op.parent_id.is_none());
        assert!(op.name.is_none());
    }

    #[test]
    fn test_operation_with_parent_and_name() {
        let op = Operation::new("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        assert_eq!(op.parent_id, Some("parent-456".to_string()));
        assert_eq!(op.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_is_completed() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_completed());

        op.status = OperationStatus::Succeeded;
        assert!(op.is_completed());

        op.status = OperationStatus::Failed;
        assert!(op.is_completed());

        op.status = OperationStatus::Cancelled;
        assert!(op.is_completed());

        op.status = OperationStatus::TimedOut;
        assert!(op.is_completed());

        op.status = OperationStatus::Stopped;
        assert!(op.is_completed());
    }

    #[test]
    fn test_operation_is_succeeded() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_succeeded());

        op.status = OperationStatus::Succeeded;
        assert!(op.is_succeeded());

        op.status = OperationStatus::Failed;
        assert!(!op.is_succeeded());
    }

    #[test]
    fn test_operation_is_failed() {
        let mut op = Operation::new("op-123", OperationType::Step);
        assert!(!op.is_failed());

        op.status = OperationStatus::Failed;
        assert!(op.is_failed());

        op.status = OperationStatus::Cancelled;
        assert!(op.is_failed());

        op.status = OperationStatus::TimedOut;
        assert!(op.is_failed());

        op.status = OperationStatus::Succeeded;
        assert!(!op.is_failed());
    }

    #[test]
    fn test_operation_type_display() {
        assert_eq!(OperationType::Execution.to_string(), "Execution");
        assert_eq!(OperationType::Step.to_string(), "Step");
        assert_eq!(OperationType::Wait.to_string(), "Wait");
        assert_eq!(OperationType::Callback.to_string(), "Callback");
        assert_eq!(OperationType::Invoke.to_string(), "Invoke");
        assert_eq!(OperationType::Context.to_string(), "Context");
    }

    #[test]
    fn test_operation_status_is_terminal() {
        assert!(!OperationStatus::Started.is_terminal());
        assert!(OperationStatus::Succeeded.is_terminal());
        assert!(OperationStatus::Failed.is_terminal());
        assert!(OperationStatus::Cancelled.is_terminal());
        assert!(OperationStatus::TimedOut.is_terminal());
        assert!(OperationStatus::Stopped.is_terminal());
    }

    #[test]
    fn test_operation_status_is_success() {
        assert!(!OperationStatus::Started.is_success());
        assert!(OperationStatus::Succeeded.is_success());
        assert!(!OperationStatus::Failed.is_success());
    }

    #[test]
    fn test_operation_status_is_failure() {
        assert!(!OperationStatus::Started.is_failure());
        assert!(!OperationStatus::Succeeded.is_failure());
        assert!(OperationStatus::Failed.is_failure());
        assert!(OperationStatus::Cancelled.is_failure());
        assert!(OperationStatus::TimedOut.is_failure());
        assert!(OperationStatus::Stopped.is_failure());
    }

    #[test]
    fn test_operation_update_start() {
        let update = OperationUpdate::start("op-123", OperationType::Step);
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Start);
        assert_eq!(update.operation_type, OperationType::Step);
        assert!(update.result.is_none());
        assert!(update.error.is_none());
    }

    #[test]
    fn test_operation_update_succeed() {
        let update = OperationUpdate::succeed(
            "op-123",
            OperationType::Step,
            Some(r#"{"value": 42}"#.to_string()),
        );
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Succeed);
        assert_eq!(update.result, Some(r#"{"value": 42}"#.to_string()));
        assert!(update.error.is_none());
    }

    #[test]
    fn test_operation_update_fail() {
        let error = ErrorObject::new("TestError", "Something went wrong");
        let update = OperationUpdate::fail("op-123", OperationType::Step, error);
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.action, OperationAction::Fail);
        assert!(update.result.is_none());
        assert!(update.error.is_some());
        assert_eq!(update.error.as_ref().unwrap().error_type, "TestError");
    }

    #[test]
    fn test_operation_update_with_parent_and_name() {
        let update = OperationUpdate::start("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_serialization() {
        let op = Operation::new("op-123", OperationType::Step)
            .with_parent_id("parent-456")
            .with_name("my-step");
        
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("\"OperationId\":\"op-123\""));
        assert!(json.contains("\"OperationType\":\"Step\""));
        assert!(json.contains("\"Status\":\"Started\""));
        assert!(json.contains("\"ParentId\":\"parent-456\""));
        assert!(json.contains("\"Name\":\"my-step\""));
    }

    #[test]
    fn test_operation_deserialization() {
        let json = r#"{
            "OperationId": "op-123",
            "OperationType": "Step",
            "Status": "Succeeded",
            "Result": "{\"value\": 42}",
            "ParentId": "parent-456",
            "Name": "my-step"
        }"#;
        
        let op: Operation = serde_json::from_str(json).unwrap();
        assert_eq!(op.operation_id, "op-123");
        assert_eq!(op.operation_type, OperationType::Step);
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some(r#"{"value": 42}"#.to_string()));
        assert_eq!(op.parent_id, Some("parent-456".to_string()));
        assert_eq!(op.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_operation_update_serialization() {
        let update = OperationUpdate::succeed(
            "op-123",
            OperationType::Step,
            Some(r#"{"value": 42}"#.to_string()),
        ).with_parent_id("parent-456");
        
        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("\"OperationId\":\"op-123\""));
        assert!(json.contains("\"Action\":\"Succeed\""));
        assert!(json.contains("\"OperationType\":\"Step\""));
        assert!(json.contains("\"Result\":\"{\\\"value\\\": 42}\""));
        assert!(json.contains("\"ParentId\":\"parent-456\""));
    }
}
