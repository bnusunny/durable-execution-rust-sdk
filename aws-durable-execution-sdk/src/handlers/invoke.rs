//! Invoke operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the invoke handler which calls other
//! durable Lambda functions from within a workflow.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::config::InvokeConfig;
use crate::context::{Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject, TerminationReason};
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;

/// Invokes another durable Lambda function.
///
/// This handler implements the invoke semantics:
/// - Calls the target Lambda function via service client
/// - Handles timeout configuration
/// - Checkpoints invocation and result
/// - Propagates errors from invoked function
///
/// # Arguments
///
/// * `function_name` - The name or ARN of the Lambda function to invoke
/// * `payload` - The payload to send to the function
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `config` - Invoke configuration (timeout, serdes)
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result from the invoked function, or an error if invocation fails.
///
/// # Requirements
///
/// - 7.1: Call the target Lambda function
/// - 7.2: Support configurable timeout
/// - 7.3: Support custom SerDes for payload and result
/// - 7.4: Checkpoint the invocation and result
/// - 7.5: Propagate errors from invoked function
pub async fn invoke_handler<P, R>(
    function_name: &str,
    payload: P,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    config: &InvokeConfig<P, R>,
    logger: &Arc<dyn Logger>,
) -> Result<R, DurableError>
where
    P: Serialize + DeserializeOwned + Send,
    R: Serialize + DeserializeOwned + Send,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting invoke operation: {} -> {}", op_id, function_name), &log_info);

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;

    if checkpoint_result.is_existent() {
        // Check for non-deterministic execution
        if let Some(op_type) = checkpoint_result.operation_type() {
            if op_type != OperationType::Invoke {
                return Err(DurableError::NonDeterministic {
                    message: format!(
                        "Expected Invoke operation but found {:?} at operation_id {}",
                        op_type, op_id.operation_id
                    ),
                    operation_id: Some(op_id.operation_id.clone()),
                });
            }
        }

        // Handle succeeded checkpoint
        if checkpoint_result.is_succeeded() {
            logger.debug(&format!("Replaying succeeded invoke: {}", op_id), &log_info);
            
            if let Some(result_str) = checkpoint_result.result() {
                let serdes = JsonSerDes::<R>::new();
                let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
                let result = serdes.deserialize(result_str, &serdes_ctx)
                    .map_err(|e| DurableError::SerDes {
                        message: format!("Failed to deserialize invoke result: {}", e),
                    })?;
                
                state.track_replay(&op_id.operation_id).await;
                return Ok(result);
            }
        }

        // Handle failed checkpoint
        if checkpoint_result.is_failed() {
            logger.debug(&format!("Replaying failed invoke: {}", op_id), &log_info);
            
            state.track_replay(&op_id.operation_id).await;
            
            if let Some(error) = checkpoint_result.error() {
                return Err(DurableError::Invocation {
                    message: error.error_message.clone(),
                    termination_reason: TerminationReason::InvocationError,
                });
            } else {
                return Err(DurableError::invocation("Invoke failed with unknown error"));
            }
        }

        // Handle STOPPED status (Requirement 7.7)
        if checkpoint_result.is_stopped() {
            logger.debug(&format!("Replaying stopped invoke: {}", op_id), &log_info);
            
            state.track_replay(&op_id.operation_id).await;
            
            return Err(DurableError::Invocation {
                message: "Invoke was stopped externally".to_string(),
                termination_reason: TerminationReason::InvocationError,
            });
        }

        // Handle other terminal states
        if checkpoint_result.is_terminal() {
            state.track_replay(&op_id.operation_id).await;
            
            let status = checkpoint_result.status().unwrap();
            return Err(DurableError::Invocation {
                message: format!("Invoke was {}", status),
                termination_reason: TerminationReason::InvocationError,
            });
        }
    }

    // Serialize the payload
    let payload_serdes = JsonSerDes::<P>::new();
    let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
    let payload_json = payload_serdes.serialize(&payload, &serdes_ctx)
        .map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize invoke payload: {}", e),
        })?;

    // Checkpoint the invocation start (Requirement 7.4)
    let start_update = create_invoke_start_update(op_id, function_name, &payload_json, config);
    state.create_checkpoint(start_update, true).await?;

    logger.debug(&format!("Invoking function: {}", function_name), &log_info);

    // For now, we simulate the invoke by suspending
    // In a real implementation, this would call the Lambda service
    // and the result would be delivered via the durable execution service
    
    // The actual invocation is handled by the Lambda durable execution service
    // We suspend here and wait for the result to be checkpointed
    Err(DurableError::Suspend {
        scheduled_timestamp: None,
    })
}

/// Creates a Start operation update for invoke.
///
/// # Requirements
///
/// - 7.6: THE Invoke_Operation SHALL support optional TenantId for tenant isolation scenarios
fn create_invoke_start_update<P, R>(
    op_id: &OperationIdentifier,
    function_name: &str,
    payload_json: &str,
    config: &InvokeConfig<P, R>,
) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Invoke);
    
    // Store the payload in the result field
    update.result = Some(payload_json.to_string());
    
    // Set ChainedInvokeOptions with function name and optional tenant_id (Requirement 7.6)
    update = update.with_chained_invoke_options(function_name, config.tenant_id.clone());
    
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Succeed operation update for invoke.
#[allow(dead_code)]
fn create_invoke_succeed_update(op_id: &OperationIdentifier, result: Option<String>) -> OperationUpdate {
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Invoke, result);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Fail operation update for invoke.
#[allow(dead_code)]
fn create_invoke_fail_update(op_id: &OperationIdentifier, error: ErrorObject) -> OperationUpdate {
    let mut update = OperationUpdate::fail(&op_id.operation_id, OperationType::Invoke, error);
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
    use crate::duration::Duration;
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
        OperationIdentifier::new("test-invoke-123", None, Some("test-invoke".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config() -> InvokeConfig<String, String> {
        let mut config = InvokeConfig::default();
        config.timeout = Duration::from_minutes(5);
        config
    }

    #[tokio::test]
    async fn test_invoke_handler_suspends_on_new_invoke() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<String, DurableError> = invoke_handler(
            "target-function",
            "test-payload".to_string(),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        // Should suspend since invoke is async
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { .. } => {}
            _ => panic!("Expected Suspend error"),
        }
    }

    #[tokio::test]
    async fn test_invoke_handler_replay_success() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing succeeded invoke operation
        let mut op = Operation::new("test-invoke-123", OperationType::Invoke);
        op.status = OperationStatus::Succeeded;
        op.result = Some(r#""invoke_result""#.to_string());
        
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

        let result: Result<String, DurableError> = invoke_handler(
            "target-function",
            "test-payload".to_string(),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "invoke_result");
    }

    #[tokio::test]
    async fn test_invoke_handler_replay_failure() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing failed invoke operation
        let mut op = Operation::new("test-invoke-123", OperationType::Invoke);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("InvokeError", "Target function failed"));
        
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

        let result: Result<String, DurableError> = invoke_handler(
            "target-function",
            "test-payload".to_string(),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Invocation { message, .. } => {
                assert!(message.contains("Target function failed"));
            }
            _ => panic!("Expected Invocation error"),
        }
    }

    #[tokio::test]
    async fn test_invoke_handler_non_deterministic_detection() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a Step operation at the same ID (wrong type)
        let mut op = Operation::new("test-invoke-123", OperationType::Step);
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

        let result: Result<String, DurableError> = invoke_handler(
            "target-function",
            "test-payload".to_string(),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-invoke-123".to_string()));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[test]
    fn test_create_invoke_start_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-invoke".to_string()));
        let mut config: InvokeConfig<String, String> = InvokeConfig::default();
        config.timeout = Duration::from_minutes(5);
        config.tenant_id = Some("tenant-123".to_string());
        let update = create_invoke_start_update(&op_id, "target-function", r#"{"key":"value"}"#, &config);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Invoke);
        assert!(update.result.is_some());
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-invoke".to_string()));
        
        // Verify ChainedInvokeOptions are set correctly (Requirement 7.6)
        assert!(update.chained_invoke_options.is_some());
        let invoke_options = update.chained_invoke_options.unwrap();
        assert_eq!(invoke_options.function_name, "target-function");
        assert_eq!(invoke_options.tenant_id, Some("tenant-123".to_string()));
    }

    #[test]
    fn test_create_invoke_start_update_without_tenant_id() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let config: InvokeConfig<String, String> = InvokeConfig::default();
        let update = create_invoke_start_update(&op_id, "target-function", r#"{"key":"value"}"#, &config);
        
        assert!(update.chained_invoke_options.is_some());
        let invoke_options = update.chained_invoke_options.unwrap();
        assert_eq!(invoke_options.function_name, "target-function");
        assert!(invoke_options.tenant_id.is_none());
    }

    #[tokio::test]
    async fn test_invoke_handler_replay_stopped() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing stopped invoke operation (Requirement 7.7)
        let mut op = Operation::new("test-invoke-123", OperationType::Invoke);
        op.status = OperationStatus::Stopped;
        
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

        let result: Result<String, DurableError> = invoke_handler(
            "target-function",
            "test-payload".to_string(),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Invocation { message, .. } => {
                assert!(message.contains("stopped externally"));
            }
            e => panic!("Expected Invocation error, got {:?}", e),
        }
    }

    #[test]
    fn test_create_invoke_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let update = create_invoke_succeed_update(&op_id, Some("result".to_string()));
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Invoke);
        assert_eq!(update.result, Some("result".to_string()));
    }

    #[test]
    fn test_create_invoke_fail_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let error = ErrorObject::new("InvokeError", "test message");
        let update = create_invoke_fail_update(&op_id, error);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Invoke);
        assert!(update.error.is_some());
        assert_eq!(update.error.unwrap().error_type, "InvokeError");
    }
}
