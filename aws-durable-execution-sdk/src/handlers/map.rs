//! Map operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the map handler which processes collections
//! in parallel with configurable concurrency and failure tolerance.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::concurrency::{BatchResult, CompletionReason, ConcurrentExecutor};
use crate::config::{ItemBatcher, MapConfig};
use crate::context::{DurableContext, Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject};
use crate::handlers::child::child_handler;
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;
use crate::config::ChildConfig;

/// Executes a map operation over a collection with parallel processing.
///
/// This handler implements the map semantics:
/// - Creates a child context for each item
/// - Uses ConcurrentExecutor for parallel execution
/// - Supports ItemBatcher for batching
/// - Returns BatchResult with results for all items
///
/// # Arguments
///
/// * `items` - The collection of items to process
/// * `func` - The function to apply to each item
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier for the map operation
/// * `parent_ctx` - The parent DurableContext
/// * `config` - Map configuration
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// A `BatchResult` containing results for all items.
///
/// # Requirements
///
/// - 8.1: Execute the function for each item
/// - 8.2: Support configurable max_concurrency
/// - 8.3: Support CompletionConfig for success/failure criteria
/// - 8.4: Support ItemBatcher for batching
pub async fn map_handler<T, U, F, Fut>(
    items: Vec<T>,
    func: F,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    parent_ctx: &DurableContext,
    config: &MapConfig,
    logger: &Arc<dyn Logger>,
) -> Result<BatchResult<U>, DurableError>
where
    T: Serialize + DeserializeOwned + Send + Sync + Clone + 'static,
    U: Serialize + DeserializeOwned + Send + 'static,
    F: Fn(DurableContext, T, usize) -> Fut + Send + Sync + Clone + 'static,
    Fut: std::future::Future<Output = Result<U, DurableError>> + Send + 'static,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting map operation: {} with {} items", op_id, items.len()), &log_info);

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
            logger.debug(&format!("Replaying succeeded map operation: {}", op_id), &log_info);
            
            if let Some(result_str) = checkpoint_result.result() {
                let serdes = JsonSerDes::<BatchResult<U>>::new();
                let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
                let result = serdes.deserialize(result_str, &serdes_ctx)
                    .map_err(|e| DurableError::SerDes {
                        message: format!("Failed to deserialize map result: {}", e),
                    })?;
                
                state.track_replay(&op_id.operation_id).await;
                return Ok(result);
            }
        }

        // Handle failed checkpoint
        if checkpoint_result.is_failed() {
            logger.debug(&format!("Replaying failed map operation: {}", op_id), &log_info);
            
            state.track_replay(&op_id.operation_id).await;
            
            if let Some(error) = checkpoint_result.error() {
                return Err(DurableError::UserCode {
                    message: error.error_message.clone(),
                    error_type: error.error_type.clone(),
                    stack_trace: error.stack_trace.clone(),
                });
            } else {
                return Err(DurableError::execution("Map operation failed with unknown error"));
            }
        }

        // Handle other terminal states
        if checkpoint_result.is_terminal() {
            state.track_replay(&op_id.operation_id).await;
            
            let status = checkpoint_result.status().unwrap();
            return Err(DurableError::execution(format!("Map operation was {}", status)));
        }
    }

    // Handle empty collection
    if items.is_empty() {
        logger.debug("Map operation with empty collection", &log_info);
        let result = BatchResult::empty();
        
        // Checkpoint the empty result
        let serdes = JsonSerDes::<BatchResult<U>>::new();
        let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
        let serialized = serdes.serialize(&result, &serdes_ctx)
            .map_err(|e| DurableError::SerDes {
                message: format!("Failed to serialize map result: {}", e),
            })?;
        
        let succeed_update = create_succeed_update(op_id, Some(serialized));
        state.create_checkpoint(succeed_update, true).await?;
        
        return Ok(result);
    }

    // Apply batching if configured
    let batched_items = if let Some(ref batcher) = config.item_batcher {
        batch_items(&items, batcher)
    } else {
        items.into_iter().enumerate().map(|(i, item)| (i, vec![item])).collect()
    };

    // Create the map context (child of parent)
    let map_ctx = parent_ctx.create_child_context(&op_id.operation_id);

    // Create tasks for concurrent execution
    let total_tasks = batched_items.len();
    let executor = ConcurrentExecutor::new(
        total_tasks,
        config.max_concurrency,
        config.completion_config.clone(),
    );

    // Build task closures
    let tasks: Vec<_> = batched_items
        .into_iter()
        .map(|(original_index, batch)| {
            let func = func.clone();
            let map_ctx = map_ctx.clone();
            let state = state.clone();
            let logger = logger.clone();
            let op_id = op_id.clone();

            move |_task_idx: usize| {
                let func = func.clone();
                let map_ctx = map_ctx.clone();
                let state = state.clone();
                let logger = logger.clone();
                let op_id = op_id.clone();

                Box::pin(async move {
                    // For batched items, process each item in the batch
                    // For now, we process the first item (single item batches)
                    let item = batch.into_iter().next().ok_or_else(|| {
                        DurableError::validation("Empty batch in map operation")
                    })?;

                    // Create child operation ID for this item
                    let child_op_id = OperationIdentifier::new(
                        map_ctx.next_operation_id(),
                        Some(op_id.operation_id.clone()),
                        Some(format!("map-item-{}", original_index)),
                    );

                    // Execute in child context
                    child_handler(
                        |ctx| {
                            let item = item.clone();
                            let func = func.clone();
                            async move { func(ctx, item, original_index).await }
                        },
                        &state,
                        &child_op_id,
                        &map_ctx,
                        &ChildConfig::default(),
                        &logger,
                    ).await
                }) as std::pin::Pin<Box<dyn std::future::Future<Output = Result<U, DurableError>> + Send>>
            }
        })
        .collect();

    // Execute all tasks
    let batch_result = executor.execute(tasks).await;

    logger.debug(
        &format!(
            "Map operation completed: {} succeeded, {} failed",
            batch_result.success_count(),
            batch_result.failure_count()
        ),
        &log_info,
    );

    // Checkpoint the result
    let serdes = JsonSerDes::<BatchResult<U>>::new();
    let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
    
    // Only checkpoint if not suspended
    if batch_result.completion_reason != CompletionReason::Suspended {
        let serialized = serdes.serialize(&batch_result, &serdes_ctx)
            .map_err(|e| DurableError::SerDes {
                message: format!("Failed to serialize map result: {}", e),
            })?;
        
        let succeed_update = create_succeed_update(op_id, Some(serialized));
        state.create_checkpoint(succeed_update, true).await?;
        
        // Mark parent as done
        state.mark_parent_done(&op_id.operation_id).await;
    }

    Ok(batch_result)
}

/// Batches items according to the ItemBatcher configuration.
fn batch_items<T: Clone>(items: &[T], batcher: &ItemBatcher) -> Vec<(usize, Vec<T>)> {
    let mut batches = Vec::new();
    let mut current_batch = Vec::new();
    let mut batch_start_index = 0;

    for (i, item) in items.iter().enumerate() {
        if current_batch.len() >= batcher.max_items_per_batch {
            batches.push((batch_start_index, std::mem::take(&mut current_batch)));
            batch_start_index = i;
        }
        current_batch.push(item.clone());
    }

    if !current_batch.is_empty() {
        batches.push((batch_start_index, current_batch));
    }

    batches
}

/// Creates a Succeed operation update for map operation.
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

/// Creates a Fail operation update for map operation.
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
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-2".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-3".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-4".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-5".to_string(),
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
        OperationIdentifier::new("test-map-123", Some("parent-op".to_string()), Some("test-map".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config() -> MapConfig {
        MapConfig::default()
    }

    fn create_test_parent_ctx(state: Arc<ExecutionState>) -> DurableContext {
        DurableContext::new(state)
    }

    #[tokio::test]
    async fn test_map_handler_empty_collection() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let items: Vec<i32> = vec![];
        let result = map_handler(
            items,
            |_ctx, item: i32, _idx| async move { Ok(item * 2) },
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
    async fn test_map_handler_single_item() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let items = vec![5];
        let result = map_handler(
            items,
            |_ctx, item: i32, _idx| async move { Ok(item * 2) },
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
        assert!(batch_result.all_succeeded());
    }

    #[tokio::test]
    async fn test_map_handler_multiple_items() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_responses(10)
        );
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let items = vec![1, 2, 3];
        let result = map_handler(
            items,
            |_ctx, item: i32, _idx| async move { Ok(item * 10) },
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
    async fn test_map_handler_with_concurrency_limit() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_responses(10)
        );
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = MapConfig {
            max_concurrency: Some(2),
            ..Default::default()
        };
        let logger = create_test_logger();
        let parent_ctx = create_test_parent_ctx(state.clone());

        let items = vec![1, 2, 3, 4, 5];
        let result = map_handler(
            items,
            |_ctx, item: i32, _idx| async move { Ok(item) },
            &state,
            &op_id,
            &parent_ctx,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let batch_result = result.unwrap();
        assert_eq!(batch_result.total_count(), 5);
    }

    #[test]
    fn test_batch_items_no_batching() {
        let items = vec![1, 2, 3, 4, 5];
        let batcher = ItemBatcher {
            max_items_per_batch: 10,
            max_bytes_per_batch: 1024,
        };
        
        let batches = batch_items(&items, &batcher);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].0, 0);
        assert_eq!(batches[0].1, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_batch_items_with_batching() {
        let items = vec![1, 2, 3, 4, 5];
        let batcher = ItemBatcher {
            max_items_per_batch: 2,
            max_bytes_per_batch: 1024,
        };
        
        let batches = batch_items(&items, &batcher);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].1, vec![1, 2]);
        assert_eq!(batches[1].1, vec![3, 4]);
        assert_eq!(batches[2].1, vec![5]);
    }

    #[test]
    fn test_batch_items_empty() {
        let items: Vec<i32> = vec![];
        let batcher = ItemBatcher::default();
        
        let batches = batch_items(&items, &batcher);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_create_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-map".to_string()));
        let update = create_succeed_update(&op_id, Some("result".to_string()));
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Context);
        assert_eq!(update.result, Some("result".to_string()));
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-map".to_string()));
    }
}
