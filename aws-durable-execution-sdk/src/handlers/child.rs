//! Child context operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the child context handler which creates
//! isolated nested workflows within a parent execution.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::config::ChildConfig;
use crate::context::{DurableContext, Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject};
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;

/// Executes a function in a child context.
///
/// This handler implements the child context semantics:
/// - Creates a child context with the parent's operation_id
/// - Executes the function in the child context
/// - Checkpoints the child result
/// - Propagates errors to the parent
///
/// # Arguments
///
/// * `func` - The function to execute in the child context
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier for the child context
/// * `parent_ctx` - The parent DurableContext
/// * `config` - Child context configuration
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result of the child function, or an error if execution fails.
///
/// # Requirements
///
/// - 10.1: Create a new context with the parent's operation_id
/// - 10.2: Checkpoint the child context result when complete
/// - 10.3: Support custom SerDes for result serialization
/// - 10.4: Propagate errors to the parent
pub async fn child_handler<T, F, Fut>(
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    parent_ctx: &DurableContext,
    _config: &ChildConfig,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send,
    F: FnOnce(DurableContext) -> Fut + Send,
    Fut: std::future::Future<Output = Result<T, DurableError>> + Send,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting child context: {}", op_id), &log_info);

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;

    if checkpoint_result.is_existent() {
        // Check for non-deterministic execution
        if let Some(op_type) = checkpoint_result.operation_type() {
            if op_type != OperationType::Context {
                return Err(DurableError::NonDeterministic {
                    message: format!(
                        "Expected Context operation but found {:?} at operation_id {}",
                        op_type, op_id.operation_id
                    ),
                    operation_id: Some(op_id.operation_id.clone()),
                });
            }
        }

        // Handle succeeded checkpoint
        if checkpoint_result.is_succeeded() {
            logger.debug(&format!("Replaying succeeded child context: {}", op_id), &log_info);
            
            if let Some(result_str) = checkpoint_result.result() {
                let serdes = JsonSerDes::<T>::new();
                let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
                let result = serdes.deserialize(result_str, &serdes_ctx)
                    .map_err(|e| DurableError::SerDes {
                        message: format!("Failed to deserialize child context result: {}", e),
                    })?;
                
                state.track_replay(&op_id.operation_id).await;
                return Ok(result);
            }
        }

        // Handle failed checkpoint
        if checkpoint_result.is_failed() {
            logger.debug(&format!("Replaying failed child context: {}", op_id), &log_info);
            
            state.track_replay(&op_id.operation_id).await;
            
            if let Some(error) = checkpoint_result.error() {
                return Err(DurableError::UserCode {
                    message: error.error_message.clone(),
                    error_type: error.error_type.clone(),
                    stack_trace: error.stack_trace.clone(),
                });
            } else {
                return Err(DurableError::execution("Child context failed with unknown error"));
            }
        }

        // Handle other terminal states
        if checkpoint_result.is_terminal() {
            state.track_replay(&op_id.operation_id).await;
            
            let status = checkpoint_result.status().unwrap();
            return Err(DurableError::execution(format!("Child context was {}", status)));
        }
    }

    // Create child context (Requirement 10.1)
    let child_ctx = parent_ctx.create_child_context(&op_id.operation_id);

    logger.debug(&format!("Executing child context: {}", op_id), &log_info);

    // Execute the function in the child context
    let result = func(child_ctx).await;

    // Get the serializer
    let serdes = JsonSerDes::<T>::new();
    let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());

    // Checkpoint the result (Requirement 10.2)
    match result {
        Ok(value) => {
            let serialized = serdes.serialize(&value, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to serialize child context result: {}", e),
                })?;
            
            let succeed_update = create_succeed_update(op_id, Some(serialized));
            state.create_checkpoint(succeed_update, true).await?;
            
            // Mark parent as done to prevent orphaned children
            state.mark_parent_done(&op_id.operation_id).await;
            
            logger.debug("Child context completed successfully", &log_info);
            Ok(value)
        }
        Err(error) => {
            // Check if this is a suspend - don't checkpoint suspends
            if error.is_suspend() {
                return Err(error);
            }
            
            // Checkpoint the error (Requirement 10.4)
            let error_obj = ErrorObject::from(&error);
            let fail_update = create_fail_update(op_id, error_obj);
            state.create_checkpoint(fail_update, true).await?;
            
            // Mark parent as done to prevent orphaned children
            state.mark_parent_done(&op_id.operation_id).await;
            
            logger.error(&format!("Child context failed: {}", error), &log_info);
            Err(error)
        }
    }
}

/// Creates a Start operation update for child context.
#[allow(dead_code)]
fn create_start_update(op_id: &OperationIdentifier) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Context);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Succeed operation update for child context.
fn create_succeed_update(op_id: &OperationIdentifier, result: Option<String>) -> OperationUpdate {
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Context, result);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Fail operation update for child context.
fn create_fail_update(op_id: &OperationIdentifier, error: ErrorObject) -> OperationUpdate {
    let mut update = OperationUpdate::fail(&op_id.operation_id, OperationType::Context, error);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
        )
    }

    fn create_test_state(client: SharedDurableServiceClient) -> Arc<ExecutionState> {
        Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            InitialExecutionState::new(),
            client,
        ))
    }

    fn create_test_op_id() -> OperationIdentifier {
        OperationIdentifier::new("test-child-123", Some("parent-op".to_string()), Some("test-child".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config() -> ChildConfig {
        ChildConfig::default()
    }

    fn create_test_parent_ctx(state: Arc<ExecutionState>) -> DurableContext {
        DurableContext::new(state)
    }

    #[tokio::test]
    async fn test_child_handler_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { Ok(42) },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_child_handler_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { Err(DurableError::execution("child error")) },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Execution { message, .. } => {
                assert!(message.contains("child error"));
            }
            _ => panic!("Expected Execution error"),
        }
    }

    #[tokio::test]
    async fn test_child_handler_replay_success() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing succeeded child context
        let mut op = Operation::new("test-child-123", OperationType::Context);
        op.status = OperationStatus::Succeeded;
        op.result = Some("42".to_string());
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { panic!("Function should not be called during replay") },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_child_handler_replay_failure() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing failed child context
        let mut op = Operation::new("test-child-123", OperationType::Context);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("ChildError", "Previous failure"));
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { panic!("Function should not be called during replay") },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::UserCode { message, .. } => {
                assert!(message.contains("Previous failure"));
            }
            _ => panic!("Expected UserCode error"),
        }
    }

    #[tokio::test]
    async fn test_child_handler_non_deterministic_detection() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a Step operation at the same ID (wrong type)
        let mut op = Operation::new("test-child-123", OperationType::Step);
        op.status = OperationStatus::Succeeded;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { Ok(42) },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-child-123".to_string()));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[tokio::test]
    async fn test_child_handler_suspend_not_checkpointed() {
        let client = Arc::new(MockDurableServiceClient::new());
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let result: Result<i32, DurableError> = child_handler(
            |_ctx| async { Err(DurableError::suspend()) },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        // Should propagate suspend without checkpointing
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { .. } => {}
            _ => panic!("Expected Suspend error"),
        }
    }

    #[test]
    fn test_create_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-child".to_string()));
        let update = create_succeed_update(&op_id, Some("result".to_string()));
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Context);
        assert_eq!(update.result, Some("result".to_string()));
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-child".to_string()));
    }

    #[test]
    fn test_create_fail_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let error = ErrorObject::new("ChildError", "test message");
        let update = create_fail_update(&op_id, error);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Context);
        assert!(update.error.is_some());
        assert_eq!(update.error.unwrap().error_type, "ChildError");
    }
}
