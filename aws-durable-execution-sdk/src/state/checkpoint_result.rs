//! Result type for checkpoint queries.
//!
//! This module provides the [`CheckpointedResult`] type for querying the status
//! of previously checkpointed operations during replay.

use crate::error::ErrorObject;
use crate::operation::{Operation, OperationStatus, OperationType};

/// Result of checking for a checkpointed operation.
///
/// This struct provides methods to query the status of a previously
/// checkpointed operation during replay.
#[derive(Debug, Clone)]
pub struct CheckpointedResult {
    /// The operation if it exists in the checkpoint
    operation: Option<Operation>,
}

impl CheckpointedResult {
    /// Creates a new CheckpointedResult with the given operation.
    pub fn new(operation: Option<Operation>) -> Self {
        Self { operation }
    }

    /// Creates an empty CheckpointedResult (no checkpoint exists).
    pub fn empty() -> Self {
        Self { operation: None }
    }

    /// Returns true if a checkpoint exists for this operation.
    pub fn is_existent(&self) -> bool {
        self.operation.is_some()
    }

    /// Returns true if the operation succeeded.
    pub fn is_succeeded(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Succeeded)
            .unwrap_or(false)
    }

    /// Returns true if the operation failed.
    pub fn is_failed(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Failed)
            .unwrap_or(false)
    }

    /// Returns true if the operation was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Cancelled)
            .unwrap_or(false)
    }

    /// Returns true if the operation timed out.
    pub fn is_timed_out(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::TimedOut)
            .unwrap_or(false)
    }

    /// Returns true if the operation was stopped.
    pub fn is_stopped(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Stopped)
            .unwrap_or(false)
    }

    /// Returns true if the operation is pending (waiting for retry).
    /// Requirements: 3.7, 4.7
    pub fn is_pending(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Pending)
            .unwrap_or(false)
    }

    /// Returns true if the operation is ready to resume execution.
    /// Requirements: 3.7
    pub fn is_ready(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Ready)
            .unwrap_or(false)
    }

    /// Returns true if the operation is in a terminal state (completed).
    pub fn is_terminal(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status.is_terminal())
            .unwrap_or(false)
    }

    /// Returns the operation status if the checkpoint exists.
    pub fn status(&self) -> Option<OperationStatus> {
        self.operation.as_ref().map(|op| op.status)
    }

    /// Returns the operation type if the checkpoint exists.
    pub fn operation_type(&self) -> Option<OperationType> {
        self.operation.as_ref().map(|op| op.operation_type)
    }

    /// Returns the serialized result if the operation succeeded.
    /// This checks both type-specific details (e.g., StepDetails.Result) and the legacy Result field.
    pub fn result(&self) -> Option<&str> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_result())
    }

    /// Returns the error if the operation failed.
    pub fn error(&self) -> Option<&ErrorObject> {
        self.operation.as_ref().and_then(|op| op.error.as_ref())
    }

    /// Returns a reference to the underlying operation.
    pub fn operation(&self) -> Option<&Operation> {
        self.operation.as_ref()
    }

    /// Consumes self and returns the underlying operation.
    pub fn into_operation(self) -> Option<Operation> {
        self.operation
    }

    /// Returns the retry payload if this is a STEP operation with a payload.
    ///
    /// This is used for the wait-for-condition pattern where state is passed
    /// between retry attempts via the Payload field.
    ///
    /// # Returns
    ///
    /// The payload string if available, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.9: THE Step_Operation SHALL support RETRY action with Payload for wait-for-condition pattern
    pub fn retry_payload(&self) -> Option<&str> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_retry_payload())
    }

    /// Returns the current attempt number for STEP operations.
    ///
    /// # Returns
    ///
    /// The attempt number (0-indexed) if available, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.8: THE Step_Operation SHALL track attempt numbers in StepDetails.Attempt
    pub fn attempt(&self) -> Option<u32> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_attempt())
    }

    /// Returns the callback ID for CALLBACK operations.
    ///
    /// The callback ID is generated by the Lambda service when a CALLBACK operation
    /// is checkpointed and is stored in CallbackDetails.CallbackId.
    ///
    /// # Returns
    ///
    /// The callback ID if this is a CALLBACK operation with a callback ID, None otherwise.
    pub fn callback_id(&self) -> Option<String> {
        self.operation
            .as_ref()
            .and_then(|op| op.callback_details.as_ref())
            .and_then(|details| details.callback_id.clone())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorObject;

    fn create_operation(status: OperationStatus) -> Operation {
        let mut op = Operation::new("test-op", OperationType::Step);
        op.status = status;
        op
    }

    fn create_succeeded_operation() -> Operation {
        let mut op = Operation::new("test-op", OperationType::Step);
        op.status = OperationStatus::Succeeded;
        op.result = Some(r#"{"value": 42}"#.to_string());
        op
    }

    fn create_failed_operation() -> Operation {
        let mut op = Operation::new("test-op", OperationType::Step);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("TestError", "Something went wrong"));
        op
    }

    #[test]
    fn test_empty_checkpoint_result() {
        let result = CheckpointedResult::empty();
        assert!(!result.is_existent());
        assert!(!result.is_succeeded());
        assert!(!result.is_failed());
        assert!(!result.is_cancelled());
        assert!(!result.is_timed_out());
        assert!(!result.is_stopped());
        assert!(!result.is_terminal());
        assert!(result.status().is_none());
        assert!(result.operation_type().is_none());
        assert!(result.result().is_none());
        assert!(result.error().is_none());
        assert!(result.operation().is_none());
    }

    #[test]
    fn test_succeeded_checkpoint_result() {
        let op = create_succeeded_operation();
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_succeeded());
        assert!(!result.is_failed());
        assert!(result.is_terminal());
        assert_eq!(result.status(), Some(OperationStatus::Succeeded));
        assert_eq!(result.operation_type(), Some(OperationType::Step));
        assert_eq!(result.result(), Some(r#"{"value": 42}"#));
        assert!(result.error().is_none());
    }

    #[test]
    fn test_failed_checkpoint_result() {
        let op = create_failed_operation();
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(!result.is_succeeded());
        assert!(result.is_failed());
        assert!(result.is_terminal());
        assert_eq!(result.status(), Some(OperationStatus::Failed));
        assert!(result.result().is_none());
        assert!(result.error().is_some());
        assert_eq!(result.error().unwrap().error_type, "TestError");
    }

    #[test]
    fn test_cancelled_checkpoint_result() {
        let op = create_operation(OperationStatus::Cancelled);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_cancelled());
        assert!(result.is_terminal());
    }

    #[test]
    fn test_timed_out_checkpoint_result() {
        let op = create_operation(OperationStatus::TimedOut);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_timed_out());
        assert!(result.is_terminal());
    }

    #[test]
    fn test_stopped_checkpoint_result() {
        let op = create_operation(OperationStatus::Stopped);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_stopped());
        assert!(result.is_terminal());
    }

    #[test]
    fn test_pending_checkpoint_result() {
        let op = create_operation(OperationStatus::Pending);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_pending());
        assert!(!result.is_ready());
        assert!(!result.is_terminal());
    }

    #[test]
    fn test_ready_checkpoint_result() {
        let op = create_operation(OperationStatus::Ready);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(result.is_ready());
        assert!(!result.is_pending());
        assert!(!result.is_terminal());
    }

    #[test]
    fn test_started_checkpoint_result() {
        let op = create_operation(OperationStatus::Started);
        let result = CheckpointedResult::new(Some(op));
        
        assert!(result.is_existent());
        assert!(!result.is_succeeded());
        assert!(!result.is_failed());
        assert!(!result.is_terminal());
    }

    #[test]
    fn test_into_operation() {
        let op = create_succeeded_operation();
        let result = CheckpointedResult::new(Some(op));
        
        let operation = result.into_operation();
        assert!(operation.is_some());
        assert_eq!(operation.unwrap().operation_id, "test-op");
    }
}
