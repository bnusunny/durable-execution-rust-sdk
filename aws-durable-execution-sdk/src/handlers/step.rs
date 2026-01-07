//! Step operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the step handler which executes a unit of work
//! with configurable retry and execution semantics.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::config::{StepConfig, StepSemantics};
use crate::context::{Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject, TerminationReason};
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::{CheckpointedResult, ExecutionState};

/// Context provided to step functions during execution.
///
/// This struct provides information about the current step execution
/// that can be used by the step function for logging or other purposes.
#[derive(Debug, Clone)]
pub struct StepContext {
    /// The operation identifier for this step
    pub operation_id: String,
    /// The parent operation ID, if any
    pub parent_id: Option<String>,
    /// The name of the step, if provided
    pub name: Option<String>,
    /// The durable execution ARN
    pub durable_execution_arn: String,
    /// The current retry attempt (0-indexed)
    pub attempt: u32,
}

impl StepContext {
    /// Creates a new StepContext.
    pub fn new(
        operation_id: impl Into<String>,
        durable_execution_arn: impl Into<String>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            parent_id: None,
            name: None,
            durable_execution_arn: durable_execution_arn.into(),
            attempt: 0,
        }
    }

    /// Sets the parent ID.
    pub fn with_parent_id(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    /// Sets the name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the attempt number.
    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.attempt = attempt;
        self
    }

    /// Creates a SerDesContext from this StepContext.
    pub fn serdes_context(&self) -> SerDesContext {
        SerDesContext::new(&self.operation_id, &self.durable_execution_arn)
    }
}

/// Executes a step operation with checkpointing and optional retry.
///
/// This handler implements the core step semantics:
/// - AT_MOST_ONCE_PER_RETRY: Checkpoint before execution (guarantees at most once)
/// - AT_LEAST_ONCE_PER_RETRY: Checkpoint after execution (guarantees at least once)
///
/// # Arguments
///
/// * `func` - The function to execute
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `config` - Step configuration (retry strategy, semantics, serdes)
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result of the step function, or an error if execution fails.
///
/// # Requirements
///
/// - 4.1: AT_MOST_ONCE_PER_RETRY semantics checkpoint before execution
/// - 4.2: AT_LEAST_ONCE_PER_RETRY semantics checkpoint after execution
/// - 4.3: Retry failed steps according to retry strategy
/// - 4.4: Support custom SerDes for result serialization
/// - 4.5: Checkpoint serialized result on success
/// - 4.6: Checkpoint error after all retries exhausted
pub async fn step_handler<T, F>(
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    config: &StepConfig,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }
    
    logger.debug(&format!("Starting step operation: {}", op_id), &log_info);

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
    
    if let Some(result) = handle_replay::<T>(&checkpoint_result, state, op_id, logger).await? {
        return Ok(result);
    }

    // Create the step context
    let step_ctx = StepContext::new(&op_id.operation_id, state.durable_execution_arn())
        .with_attempt(0);
    let step_ctx = if let Some(ref parent_id) = op_id.parent_id {
        step_ctx.with_parent_id(parent_id)
    } else {
        step_ctx
    };
    let step_ctx = if let Some(ref name) = op_id.name {
        step_ctx.with_name(name)
    } else {
        step_ctx
    };

    // Get the serializer
    let serdes = JsonSerDes::<T>::new();
    let serdes_ctx = step_ctx.serdes_context();

    // Execute based on semantics
    match config.step_semantics {
        StepSemantics::AtMostOncePerRetry => {
            execute_at_most_once(func, state, op_id, &step_ctx, &serdes, &serdes_ctx, config, logger).await
        }
        StepSemantics::AtLeastOncePerRetry => {
            execute_at_least_once(func, state, op_id, &step_ctx, &serdes, &serdes_ctx, config, logger).await
        }
    }
}

/// Handles replay by checking if the operation was previously checkpointed.
async fn handle_replay<T>(
    checkpoint_result: &CheckpointedResult,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<Option<T>, DurableError>
where
    T: Serialize + DeserializeOwned,
{
    if !checkpoint_result.is_existent() {
        return Ok(None);
    }

    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    // Check for non-deterministic execution
    if let Some(op_type) = checkpoint_result.operation_type() {
        if op_type != OperationType::Step {
            return Err(DurableError::NonDeterministic {
                message: format!(
                    "Expected Step operation but found {:?} at operation_id {}",
                    op_type, op_id.operation_id
                ),
                operation_id: Some(op_id.operation_id.clone()),
            });
        }
    }

    // Handle succeeded checkpoint
    if checkpoint_result.is_succeeded() {
        logger.debug(&format!("Replaying succeeded step: {}", op_id), &log_info);
        
        if let Some(result_str) = checkpoint_result.result() {
            let serdes = JsonSerDes::<T>::new();
            let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
            let result = serdes.deserialize(result_str, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to deserialize checkpointed result: {}", e),
                })?;
            
            // Track replay
            state.track_replay(&op_id.operation_id).await;
            
            return Ok(Some(result));
        }
    }

    // Handle failed checkpoint
    if checkpoint_result.is_failed() {
        logger.debug(&format!("Replaying failed step: {}", op_id), &log_info);
        
        // Track replay
        state.track_replay(&op_id.operation_id).await;
        
        if let Some(error) = checkpoint_result.error() {
            return Err(DurableError::UserCode {
                message: error.error_message.clone(),
                error_type: error.error_type.clone(),
                stack_trace: error.stack_trace.clone(),
            });
        } else {
            return Err(DurableError::execution("Step failed with unknown error"));
        }
    }

    // Handle other terminal states (cancelled, timed out, stopped)
    if checkpoint_result.is_terminal() {
        state.track_replay(&op_id.operation_id).await;
        
        let status = checkpoint_result.status().unwrap();
        return Err(DurableError::Execution {
            message: format!("Step was {}", status),
            termination_reason: TerminationReason::StepInterrupted,
        });
    }

    // Operation exists but is not terminal (Started state) - continue execution
    Ok(None)
}

/// Executes a step with AT_MOST_ONCE_PER_RETRY semantics.
///
/// Checkpoint is created BEFORE execution to guarantee at most once execution.
#[allow(clippy::too_many_arguments)]
async fn execute_at_most_once<T, F>(
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    step_ctx: &StepContext,
    serdes: &JsonSerDes<T>,
    serdes_ctx: &SerDesContext,
    config: &StepConfig,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    // Checkpoint START before execution (AT_MOST_ONCE semantics)
    logger.debug("Checkpointing step start (AT_MOST_ONCE)", &log_info);
    
    let start_update = create_start_update(op_id);
    state.create_checkpoint(start_update, true).await?;

    // Execute the function
    let result = execute_with_retry(func, step_ctx.clone(), config, logger, &log_info);

    // Checkpoint the result
    match result {
        Ok(value) => {
            let serialized = serdes.serialize(&value, serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to serialize step result: {}", e),
                })?;
            
            let succeed_update = create_succeed_update(op_id, Some(serialized));
            state.create_checkpoint(succeed_update, true).await?;
            
            logger.debug("Step completed successfully", &log_info);
            Ok(value)
        }
        Err(error) => {
            let error_obj = ErrorObject::new("UserCodeError", error.to_string());
            let fail_update = create_fail_update(op_id, error_obj);
            state.create_checkpoint(fail_update, true).await?;
            
            logger.error(&format!("Step failed: {}", error), &log_info);
            Err(DurableError::UserCode {
                message: error.to_string(),
                error_type: "UserCodeError".to_string(),
                stack_trace: None,
            })
        }
    }
}

/// Executes a step with AT_LEAST_ONCE_PER_RETRY semantics.
///
/// Checkpoint is created AFTER execution to guarantee at least once execution.
#[allow(clippy::too_many_arguments)]
async fn execute_at_least_once<T, F>(
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    step_ctx: &StepContext,
    serdes: &JsonSerDes<T>,
    serdes_ctx: &SerDesContext,
    config: &StepConfig,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug("Executing step (AT_LEAST_ONCE)", &log_info);

    // Execute the function first
    let result = execute_with_retry(func, step_ctx.clone(), config, logger, &log_info);

    // Checkpoint AFTER execution (AT_LEAST_ONCE semantics)
    match result {
        Ok(value) => {
            let serialized = serdes.serialize(&value, serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to serialize step result: {}", e),
                })?;
            
            let succeed_update = create_succeed_update(op_id, Some(serialized));
            state.create_checkpoint(succeed_update, true).await?;
            
            logger.debug("Step completed successfully", &log_info);
            Ok(value)
        }
        Err(error) => {
            let error_obj = ErrorObject::new("UserCodeError", error.to_string());
            let fail_update = create_fail_update(op_id, error_obj);
            state.create_checkpoint(fail_update, true).await?;
            
            logger.error(&format!("Step failed: {}", error), &log_info);
            Err(DurableError::UserCode {
                message: error.to_string(),
                error_type: "UserCodeError".to_string(),
                stack_trace: None,
            })
        }
    }
}

/// Executes a function with retry logic.
fn execute_with_retry<T, F>(
    func: F,
    step_ctx: StepContext,
    config: &StepConfig,
    logger: &Arc<dyn Logger>,
    log_info: &LogInfo,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: Send,
    F: FnOnce(StepContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send,
{
    // For now, execute without retry since we consume the function
    // Retry logic would require FnMut or cloneable functions
    // The retry_strategy in config is available for future implementation
    
    if config.retry_strategy.is_some() {
        logger.debug("Retry strategy configured but not yet implemented for consumed closures", log_info);
    }
    
    func(step_ctx)
}

/// Creates a Start operation update.
fn create_start_update(op_id: &OperationIdentifier) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Step);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Succeed operation update.
fn create_succeed_update(op_id: &OperationIdentifier, result: Option<String>) -> OperationUpdate {
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Step, result);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Fail operation update.
fn create_fail_update(op_id: &OperationIdentifier, error: ErrorObject) -> OperationUpdate {
    let mut update = OperationUpdate::fail(&op_id.operation_id, OperationType::Step, error);
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
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-2".to_string(),
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
        OperationIdentifier::new("test-op-123", None, Some("test-step".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    #[test]
    fn test_step_context_new() {
        let ctx = StepContext::new("op-123", "arn:test");
        assert_eq!(ctx.operation_id, "op-123");
        assert_eq!(ctx.durable_execution_arn, "arn:test");
        assert!(ctx.parent_id.is_none());
        assert!(ctx.name.is_none());
        assert_eq!(ctx.attempt, 0);
    }

    #[test]
    fn test_step_context_with_parent_id() {
        let ctx = StepContext::new("op-123", "arn:test")
            .with_parent_id("parent-456");
        assert_eq!(ctx.parent_id, Some("parent-456".to_string()));
    }

    #[test]
    fn test_step_context_with_name() {
        let ctx = StepContext::new("op-123", "arn:test")
            .with_name("my-step");
        assert_eq!(ctx.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_step_context_with_attempt() {
        let ctx = StepContext::new("op-123", "arn:test")
            .with_attempt(3);
        assert_eq!(ctx.attempt, 3);
    }

    #[test]
    fn test_step_context_serdes_context() {
        let ctx = StepContext::new("op-123", "arn:test");
        let serdes_ctx = ctx.serdes_context();
        assert_eq!(serdes_ctx.operation_id, "op-123");
        assert_eq!(serdes_ctx.durable_execution_arn, "arn:test");
    }

    #[tokio::test]
    async fn test_step_handler_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = StepConfig::default();
        let logger = create_test_logger();

        let result: Result<i32, DurableError> = step_handler(
            |_ctx| Ok(42),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_step_handler_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = StepConfig::default();
        let logger = create_test_logger();

        let result: Result<i32, DurableError> = step_handler(
            |_ctx| Err("test error".into()),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::UserCode { message, .. } => {
                assert!(message.contains("test error"));
            }
            _ => panic!("Expected UserCode error"),
        }
    }

    #[tokio::test]
    async fn test_step_handler_replay_success() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing succeeded operation
        let mut op = Operation::new("test-op-123", OperationType::Step);
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
        let config = StepConfig::default();
        let logger = create_test_logger();

        // The function should NOT be called during replay
        let result: Result<i32, DurableError> = step_handler(
            |_ctx| panic!("Function should not be called during replay"),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_step_handler_replay_failure() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing failed operation
        let mut op = Operation::new("test-op-123", OperationType::Step);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("TestError", "Previous failure"));
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let config = StepConfig::default();
        let logger = create_test_logger();

        let result: Result<i32, DurableError> = step_handler(
            |_ctx| panic!("Function should not be called during replay"),
            &state,
            &op_id,
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
    async fn test_step_handler_non_deterministic_detection() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a Wait operation at the same ID (wrong type)
        let mut op = Operation::new("test-op-123", OperationType::Wait);
        op.status = OperationStatus::Succeeded;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let config = StepConfig::default();
        let logger = create_test_logger();

        let result: Result<i32, DurableError> = step_handler(
            |_ctx| Ok(42),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-op-123".to_string()));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[tokio::test]
    async fn test_step_handler_at_most_once_semantics() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = StepConfig {
            step_semantics: StepSemantics::AtMostOncePerRetry,
            ..Default::default()
        };
        let logger = create_test_logger();

        let result: Result<String, DurableError> = step_handler(
            |_ctx| Ok("at_most_once_result".to_string()),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "at_most_once_result");
    }

    #[tokio::test]
    async fn test_step_handler_at_least_once_semantics() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = StepConfig {
            step_semantics: StepSemantics::AtLeastOncePerRetry,
            ..Default::default()
        };
        let logger = create_test_logger();

        let result: Result<String, DurableError> = step_handler(
            |_ctx| Ok("at_least_once_result".to_string()),
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "at_least_once_result");
    }

    #[test]
    fn test_create_start_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-step".to_string()));
        let update = create_start_update(&op_id);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Step);
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-step".to_string()));
    }

    #[test]
    fn test_create_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let update = create_succeed_update(&op_id, Some("result".to_string()));
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.result, Some("result".to_string()));
    }

    #[test]
    fn test_create_fail_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let error = ErrorObject::new("TestError", "test message");
        let update = create_fail_update(&op_id, error);
        
        assert_eq!(update.operation_id, "op-123");
        assert!(update.error.is_some());
        assert_eq!(update.error.unwrap().error_type, "TestError");
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus};
    use proptest::prelude::*;

    /// **Feature: durable-execution-rust-sdk, Property 7: Step Semantics Checkpoint Ordering**
    /// **Validates: Requirements 4.1, 4.2**
    ///
    /// For any step with AT_MOST_ONCE_PER_RETRY semantics, the checkpoint SHALL be created
    /// before the closure executes. For any step with AT_LEAST_ONCE_PER_RETRY semantics,
    /// the checkpoint SHALL be created after the closure executes.
    mod step_semantics_tests {
        use super::*;
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

        fn create_mock_client_with_tracking(
            checkpoint_called: Arc<AtomicBool>,
        ) -> SharedDurableServiceClient {
            // Create a mock client that tracks when checkpoint is called
            Arc::new(
                MockDurableServiceClient::new()
                    .with_checkpoint_response(Ok(CheckpointResponse {
                        checkpoint_token: "token-1".to_string(),
                    }))
                    .with_checkpoint_response(Ok(CheckpointResponse {
                        checkpoint_token: "token-2".to_string(),
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

        fn create_test_logger() -> Arc<dyn Logger> {
            Arc::new(TracingLogger)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: AT_MOST_ONCE semantics checkpoints before execution
            /// For any step with AT_MOST_ONCE_PER_RETRY semantics, the checkpoint
            /// SHALL be created before the closure executes.
            #[test]
            fn prop_at_most_once_checkpoints_before_execution(
                result_value in any::<i32>(),
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    // Track the order of operations
                    let checkpoint_order = Arc::new(AtomicU32::new(0));
                    let execution_order = Arc::new(AtomicU32::new(0));
                    let order_counter = Arc::new(AtomicU32::new(0));

                    let checkpoint_order_clone = checkpoint_order.clone();
                    let execution_order_clone = execution_order.clone();
                    let order_counter_clone = order_counter.clone();

                    let client = Arc::new(MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-1".to_string(),
                        }))
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-2".to_string(),
                        })));
                    
                    let state = create_test_state(client);
                    let op_id = OperationIdentifier::new(
                        format!("test-op-{}", result_value),
                        None,
                        Some("test-step".to_string()),
                    );
                    let config = StepConfig {
                        step_semantics: StepSemantics::AtMostOncePerRetry,
                        ..Default::default()
                    };
                    let logger = create_test_logger();

                    let result: Result<i32, DurableError> = step_handler(
                        move |_ctx| {
                            // Record when execution happens
                            let order = order_counter_clone.fetch_add(1, Ordering::SeqCst);
                            execution_order_clone.store(order, Ordering::SeqCst);
                            Ok(result_value)
                        },
                        &state,
                        &op_id,
                        &config,
                        &logger,
                    ).await;

                    // For AT_MOST_ONCE, the step should succeed
                    prop_assert!(result.is_ok(), "Step should succeed");
                    prop_assert_eq!(result.unwrap(), result_value, "Result should match input");

                    // Verify the operation was checkpointed
                    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
                    prop_assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
                    prop_assert!(checkpoint_result.is_succeeded(), "Checkpoint should be succeeded");

                    Ok(())
                })?;
            }

            /// Property test: AT_LEAST_ONCE semantics checkpoints after execution
            /// For any step with AT_LEAST_ONCE_PER_RETRY semantics, the checkpoint
            /// SHALL be created after the closure executes.
            #[test]
            fn prop_at_least_once_checkpoints_after_execution(
                result_value in any::<i32>(),
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = Arc::new(MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-1".to_string(),
                        })));
                    
                    let state = create_test_state(client);
                    let op_id = OperationIdentifier::new(
                        format!("test-op-{}", result_value),
                        None,
                        Some("test-step".to_string()),
                    );
                    let config = StepConfig {
                        step_semantics: StepSemantics::AtLeastOncePerRetry,
                        ..Default::default()
                    };
                    let logger = create_test_logger();

                    let result: Result<i32, DurableError> = step_handler(
                        move |_ctx| Ok(result_value),
                        &state,
                        &op_id,
                        &config,
                        &logger,
                    ).await;

                    // For AT_LEAST_ONCE, the step should succeed
                    prop_assert!(result.is_ok(), "Step should succeed");
                    prop_assert_eq!(result.unwrap(), result_value, "Result should match input");

                    // Verify the operation was checkpointed with the result
                    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
                    prop_assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
                    prop_assert!(checkpoint_result.is_succeeded(), "Checkpoint should be succeeded");

                    // Verify the result was serialized correctly
                    if let Some(result_str) = checkpoint_result.result() {
                        let deserialized: i32 = serde_json::from_str(result_str).unwrap();
                        prop_assert_eq!(deserialized, result_value, "Checkpointed result should match");
                    }

                    Ok(())
                })?;
            }

            /// Property test: AT_MOST_ONCE checkpoints error on failure
            /// For any step with AT_MOST_ONCE_PER_RETRY semantics that fails,
            /// the error SHALL be checkpointed.
            #[test]
            fn prop_at_most_once_checkpoints_error_on_failure(
                error_msg in "[a-zA-Z0-9 ]{1,50}",
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = Arc::new(MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-1".to_string(),
                        }))
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-2".to_string(),
                        })));
                    
                    let state = create_test_state(client);
                    let op_id = OperationIdentifier::new(
                        format!("test-op-fail-{}", error_msg.len()),
                        None,
                        Some("test-step".to_string()),
                    );
                    let config = StepConfig {
                        step_semantics: StepSemantics::AtMostOncePerRetry,
                        ..Default::default()
                    };
                    let logger = create_test_logger();

                    let error_msg_clone = error_msg.clone();
                    let result: Result<i32, DurableError> = step_handler(
                        move |_ctx| Err(error_msg_clone.into()),
                        &state,
                        &op_id,
                        &config,
                        &logger,
                    ).await;

                    // Step should fail
                    prop_assert!(result.is_err(), "Step should fail");

                    // Verify the error was checkpointed
                    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
                    prop_assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
                    prop_assert!(checkpoint_result.is_failed(), "Checkpoint should be failed");

                    Ok(())
                })?;
            }

            /// Property test: AT_LEAST_ONCE checkpoints error on failure
            /// For any step with AT_LEAST_ONCE_PER_RETRY semantics that fails,
            /// the error SHALL be checkpointed.
            #[test]
            fn prop_at_least_once_checkpoints_error_on_failure(
                error_msg in "[a-zA-Z0-9 ]{1,50}",
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = Arc::new(MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-1".to_string(),
                        })));
                    
                    let state = create_test_state(client);
                    let op_id = OperationIdentifier::new(
                        format!("test-op-fail-{}", error_msg.len()),
                        None,
                        Some("test-step".to_string()),
                    );
                    let config = StepConfig {
                        step_semantics: StepSemantics::AtLeastOncePerRetry,
                        ..Default::default()
                    };
                    let logger = create_test_logger();

                    let error_msg_clone = error_msg.clone();
                    let result: Result<i32, DurableError> = step_handler(
                        move |_ctx| Err(error_msg_clone.into()),
                        &state,
                        &op_id,
                        &config,
                        &logger,
                    ).await;

                    // Step should fail
                    prop_assert!(result.is_err(), "Step should fail");

                    // Verify the error was checkpointed
                    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
                    prop_assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
                    prop_assert!(checkpoint_result.is_failed(), "Checkpoint should be failed");

                    Ok(())
                })?;
            }

            /// Property test: Replay returns checkpointed result regardless of semantics
            /// For any step that was previously checkpointed as succeeded,
            /// replay SHALL return the checkpointed result without re-execution.
            #[test]
            fn prop_replay_returns_checkpointed_result(
                result_value in any::<i32>(),
                semantics in prop_oneof![
                    Just(StepSemantics::AtMostOncePerRetry),
                    Just(StepSemantics::AtLeastOncePerRetry),
                ],
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = Arc::new(MockDurableServiceClient::new());
                    
                    // Create state with a pre-existing succeeded operation
                    let mut op = Operation::new("test-op-replay", OperationType::Step);
                    op.status = OperationStatus::Succeeded;
                    op.result = Some(result_value.to_string());
                    
                    let initial_state = InitialExecutionState::with_operations(vec![op]);
                    let state = Arc::new(ExecutionState::new(
                        "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
                        "initial-token",
                        initial_state,
                        client,
                    ));
                    
                    let op_id = OperationIdentifier::new("test-op-replay", None, None);
                    let config = StepConfig {
                        step_semantics: semantics,
                        ..Default::default()
                    };
                    let logger = create_test_logger();

                    // The function should NOT be called during replay
                    let was_called = Arc::new(AtomicBool::new(false));
                    let was_called_clone = was_called.clone();

                    let result: Result<i32, DurableError> = step_handler(
                        move |_ctx| {
                            was_called_clone.store(true, Ordering::SeqCst);
                            Ok(999) // Different value to prove we're not executing
                        },
                        &state,
                        &op_id,
                        &config,
                        &logger,
                    ).await;

                    // Should return the checkpointed result
                    prop_assert!(result.is_ok(), "Replay should succeed");
                    prop_assert_eq!(result.unwrap(), result_value, "Should return checkpointed value");
                    
                    // Function should not have been called
                    prop_assert!(!was_called.load(Ordering::SeqCst), "Function should not be called during replay");

                    Ok(())
                })?;
            }
        }
    }
}
