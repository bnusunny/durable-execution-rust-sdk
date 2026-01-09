//! Tests for CheckpointedResult.

use crate::error::ErrorObject;
use crate::operation::{Operation, OperationStatus, OperationType};
use crate::state::CheckpointedResult;

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
