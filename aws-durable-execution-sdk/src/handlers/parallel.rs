//! Parallel operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the parallel handler which executes multiple
//! independent operations concurrently.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::concurrency::{BatchResult, CompletionReason, ConcurrentExecutor};
use crate::config::ParallelConfig;
use crate::context::{DurableContext, Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject};
use crate::handlers::child::child_handler;
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;
use crate::config::ChildConfig;

/// Executes multiple operations in parallel.
///
/// This handler implements the parallel semantics:
/// - Creates a child context for each branch
/// - Uses ConcurrentExecutor for parallel execution
/// - Returns BatchResult with results for all branches
///
/// # Arguments
///
/// * `branches` - The list of functions to execute in parallel
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier for the parallel operation
/// * `parent_ctx` - The parent DurableContext
/// * `config` - Parallel configuration
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// A `BatchResult` containing results for all branches.
///
/// # Requirements
///
/// - 9.1: Execute closures concurrently
/// - 9.2: Support configurable max_concurrency
/// - 9.5: Handle branch suspension appropriately
pub async fn parallel_handler<T, F, Fut>(
    branches: Vec<F>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    parent_ctx: &DurableContext,
    config: &ParallelConfig,
    logger: &Arc<dyn Logger>,
) -> Result<BatchResult<T>, DurableError>
where
    T: Serialize + DeserializeOwned + Send + 'static,
    F: FnOnce(DurableContext) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, DurableError>> + Send + 'static,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting parallel operation: {} with {} branches", op_id, branches.len()), &log_info);

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
            logger.debug(&format!("Replaying succeeded parallel operation: {}", op_id), &log_info);
            
            if let Some(result_str) = checkpoint_result.result() {
                let serdes = JsonSerDes::<BatchResult<T>>::new();
                let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
                let result = serdes.deserialize(result_str, &serdes_ctx)
                    .map_err(|e| DurableError::SerDes {
                        message: format!("Failed to deserialize parallel result: {}", e),
                    })?;
                
                state.track_replay(&op_id.operation_id).await;
                return Ok(result);
            }
        }

        // Handle failed checkpoint
        if checkpoint_result.is_failed() {
            logger.debug(&format!("Replaying failed parallel operation: {}", op_id), &log_info);
            
            state.track_replay(&op_id.operation_id).await;
            
            if let Some(error) = checkpoint_result.error() {
                return Err(DurableError::UserCode {
                    message: error.error_message.clone(),
                    error_type: error.error_type.clone(),
                    stack_trace: error.stack_trace.clone(),
                });
            } else {
                return Err(DurableError::execution("Parallel operation failed with unknown error"));
            }
        }

        // Handle other terminal states
        if checkpoint_result.is_terminal() {
            state.track_replay(&op_id.operation_id).await;
            
            let status = checkpoint_result.status().unwrap();
            return Err(DurableError::execution(format!("Parallel operation was {}", status)));
        }
    }

    // Handle empty branches
    if branches.is_empty() {
        logger.debug("Parallel operation with no branches", &log_info);
        let result = BatchResult::empty();
        
        // Checkpoint the empty result
        let serdes = JsonSerDes::<BatchResult<T>>::new();
        let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
        let serialized = serdes.serialize(&result, &serdes_ctx)
            .map_err(|e| DurableError::SerDes {
                message: format!("Failed to serialize parallel result: {}", e),
            })?;
        
        let succeed_update = create_succeed_update(op_id, Some(serialized));
        state.create_checkpoint(succeed_update, true).await?;
        
        return Ok(result);
    }

    // Create the parallel context (child of parent)
    let parallel_ctx = parent_ctx.create_child_context(&op_id.operation_id);

    // Create the executor
    let total_branches = branches.len();
    let executor = ConcurrentExecutor::new(
        total_branches,
        config.max_concurrency,
        config.completion_config.clone(),
    );

    // Build task closures
    let tasks: Vec<_> = branches
        .into_iter()
        .enumerate()
        .map(|(index, branch)| {
            let parallel_ctx = parallel_ctx.clone();
            let state = state.clone();
            let logger = logger.clone();
            let op_id = op_id.clone();

            move |_task_idx: usize| {
                let parallel_ctx = parallel_ctx.clone();
                let state = state.clone();
                let logger = logger.clone();
                let op_id = op_id.clone();

                Box::pin(async move {
                    // Create child operation ID for this branch
                    let child_op_id = OperationIdentifier::new(
                        parallel_ctx.next_operation_id(),
                        Some(op_id.operation_id.clone()),
                        Some(format!("parallel-branch-{}", index)),
                    );

                    // Execute in child context
                    child_handler(
                        branch,
                        &state,
                        &child_op_id,
                        &parallel_ctx,
                        &ChildConfig::default(),
                        &logger,
                    ).await
                }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, DurableError>> + Send>>
            }
        })
        .collect();

    // Execute all branches
    let batch_result = executor.execute(tasks).await;

    logger.debug(
        &format!(
            "Parallel operation completed: {} succeeded, {} failed",
            batch_result.success_count(),
            batch_result.failure_count()
        ),
        &log_info,
    );

    // Checkpoint the result (only if not suspended)
    if batch_result.completion_reason != CompletionReason::Suspended {
        let serdes = JsonSerDes::<BatchResult<T>>::new();
        let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
        let serialized = serdes.serialize(&batch_result, &serdes_ctx)
            .map_err(|e| DurableError::SerDes {
                message: format!("Failed to serialize parallel result: {}", e),
            })?;
        
        let succeed_update = create_succeed_update(op_id, Some(serialized));
        state.create_checkpoint(succeed_update, true).await?;
        
        // Mark parent as done
        state.mark_parent_done(&op_id.operation_id).await;
    }

    Ok(batch_result)
}

/// Creates a Succeed operation update for parallel operation.
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

/// Creates a Fail operation update for parallel operation.
#[allow(dead_code)]
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
    use crate::client::{MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_responses(10)
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
        OperationIdentifier::new("test-parallel-123", Some("parent-op".to_string()), Some("test-parallel".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config() -> ParallelConfig {
        ParallelConfig::default()
    }

    fn create_test_parent_ctx(state: Arc<ExecutionState>) -> DurableContext {
        DurableContext::new(state)
    }

    #[tokio::test]
    async fn test_parallel_handler_empty_branches() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![];
        let result = parallel_handler(
            branches,
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let batch_result = result.unwrap();
        assert!(batch_result.items.is_empty());
        assert_eq!(batch_result.completion_reason, CompletionReason::AllCompleted);
    }

    #[tokio::test]
    async fn test_parallel_handler_single_branch() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
            Box::new(|_ctx| Box::pin(async { Ok(42) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
        ];
        
        let result = parallel_handler(
            branches,
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let batch_result = result.unwrap();
        assert_eq!(batch_result.total_count(), 1);
        assert_eq!(batch_result.success_count(), 1);
    }

    #[tokio::test]
    async fn test_parallel_handler_multiple_branches() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
            Box::new(|_ctx| Box::pin(async { Ok(1) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            Box::new(|_ctx| Box::pin(async { Ok(2) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            Box::new(|_ctx| Box::pin(async { Ok(3) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
        ];
        
        let result = parallel_handler(
            branches,
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let batch_result = result.unwrap();
        assert_eq!(batch_result.total_count(), 3);
        assert_eq!(batch_result.success_count(), 3);
    }

    #[tokio::test]
    async fn test_parallel_handler_with_concurrency_limit() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = ParallelConfig {
            max_concurrency: Some(2),
            ..Default::default()
        };
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let branches: Vec<Box<dyn FnOnce(DurableContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>> + Send>> = vec![
            Box::new(|_ctx| Box::pin(async { Ok(1) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            Box::new(|_ctx| Box::pin(async { Ok(2) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            Box::new(|_ctx| Box::pin(async { Ok(3) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
            Box::new(|_ctx| Box::pin(async { Ok(4) }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<i32, DurableError>> + Send>>),
        ];
        
        let result = parallel_handler(
            branches,
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let batch_result = result.unwrap();
        assert_eq!(batch_result.total_count(), 4);
    }

    #[test]
    fn test_create_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-parallel".to_string()));
        let update = create_succeed_update(&op_id, Some("result".to_string()));
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Context);
        assert_eq!(update.result, Some("result".to_string()));
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-parallel".to_string()));
    }
}
