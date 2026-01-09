//! Tests for EXECUTION operation handling.

use std::sync::Arc;

use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
use crate::error::ErrorObject;
use crate::lambda::InitialExecutionState;
use crate::operation::{ExecutionDetails, Operation, OperationStatus, OperationType};
use crate::state::{CheckpointBatcherConfig, ExecutionState};

fn create_execution_operation(input_payload: Option<&str>) -> Operation {
    let mut op = Operation::new("exec-123", OperationType::Execution);
    op.status = OperationStatus::Started;
    op.execution_details = Some(ExecutionDetails {
        input_payload: input_payload.map(|s| s.to_string()),
    });
    op
}

fn create_mock_client() -> SharedDurableServiceClient {
    Arc::new(MockDurableServiceClient::new())
}

#[tokio::test]
async fn test_execution_operation_recognized() {
    let client = create_mock_client();
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    let step_op = Operation::new("step-1", OperationType::Step);
    
    let initial_state = InitialExecutionState::with_operations(vec![exec_op, step_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    assert!(state.execution_operation().is_some());
    let exec = state.execution_operation().unwrap();
    assert_eq!(exec.operation_type, OperationType::Execution);
    assert_eq!(exec.operation_id, "exec-123");
}

#[tokio::test]
async fn test_execution_operation_not_present() {
    let client = create_mock_client();
    let step_op = Operation::new("step-1", OperationType::Step);
    
    let initial_state = InitialExecutionState::with_operations(vec![step_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    assert!(state.execution_operation().is_none());
}

#[tokio::test]
async fn test_get_original_input_raw() {
    let client = create_mock_client();
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let input = state.get_original_input_raw();
    assert!(input.is_some());
    assert_eq!(input.unwrap(), r#"{"order_id": "123"}"#);
}

#[tokio::test]
async fn test_get_original_input_raw_no_payload() {
    let client = create_mock_client();
    let exec_op = create_execution_operation(None);
    
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let input = state.get_original_input_raw();
    assert!(input.is_none());
}

#[tokio::test]
async fn test_get_original_input_raw_no_execution_operation() {
    let client = create_mock_client();
    let step_op = Operation::new("step-1", OperationType::Step);
    
    let initial_state = InitialExecutionState::with_operations(vec![step_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let input = state.get_original_input_raw();
    assert!(input.is_none());
}

#[tokio::test]
async fn test_execution_operation_id() {
    let client = create_mock_client();
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let id = state.execution_operation_id();
    assert!(id.is_some());
    assert_eq!(id.unwrap(), "exec-123");
}

#[tokio::test]
async fn test_complete_execution_success() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let result = state.complete_execution_success(Some(r#"{"status": "completed"}"#.to_string())).await;
    assert!(result.is_ok());
    
    let op = state.get_operation("exec-123").await.unwrap();
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert_eq!(op.result, Some(r#"{"status": "completed"}"#.to_string()));
}

#[tokio::test]
async fn test_complete_execution_success_no_execution_operation() {
    let client = create_mock_client();
    let step_op = Operation::new("step-1", OperationType::Step);
    
    let initial_state = InitialExecutionState::with_operations(vec![step_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let result = state.complete_execution_success(Some("result".to_string())).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::DurableError::Validation { message } => {
            assert!(message.contains("no EXECUTION operation"));
        }
        _ => panic!("Expected Validation error"),
    }
}

#[tokio::test]
async fn test_complete_execution_failure() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let error = ErrorObject::new("ProcessingError", "Order processing failed");
    let result = state.complete_execution_failure(error).await;
    assert!(result.is_ok());
    
    let op = state.get_operation("exec-123").await.unwrap();
    assert_eq!(op.status, OperationStatus::Failed);
    assert!(op.error.is_some());
    assert_eq!(op.error.as_ref().unwrap().error_type, "ProcessingError");
}

#[tokio::test]
async fn test_complete_execution_failure_no_execution_operation() {
    let client = create_mock_client();
    let step_op = Operation::new("step-1", OperationType::Step);
    
    let initial_state = InitialExecutionState::with_operations(vec![step_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let error = ErrorObject::new("TestError", "Test message");
    let result = state.complete_execution_failure(error).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::DurableError::Validation { message } => {
            assert!(message.contains("no EXECUTION operation"));
        }
        _ => panic!("Expected Validation error"),
    }
}

#[tokio::test]
async fn test_with_batcher_recognizes_execution_operation() {
    let client = Arc::new(MockDurableServiceClient::new());
    let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
    
    let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
    let (state, _batcher) = ExecutionState::with_batcher(
        "arn:test",
        "token-123",
        initial_state,
        client,
        CheckpointBatcherConfig::default(),
        100,
    );
    
    assert!(state.execution_operation().is_some());
    let exec = state.execution_operation().unwrap();
    assert_eq!(exec.operation_type, OperationType::Execution);
    assert_eq!(state.get_original_input_raw(), Some(r#"{"order_id": "123"}"#));
}
