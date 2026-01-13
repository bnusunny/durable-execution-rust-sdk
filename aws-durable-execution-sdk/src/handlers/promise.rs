//! Promise combinator handlers for the AWS Durable Execution SDK.
//!
//! This module implements promise combinators (all, all_settled, race, any)
//! that coordinate multiple durable promises using familiar patterns.
//!
//! All combinators are implemented within a STEP operation to ensure durability.
//!
//! # Requirements
//!
//! - 20.1: THE Promise_Combinators SHALL provide an `all` method that waits for all promises to complete successfully
//! - 20.2: THE Promise_Combinators SHALL provide an `all_settled` method that waits for all promises to settle
//! - 20.3: THE Promise_Combinators SHALL provide a `race` method that returns the first promise to settle
//! - 20.4: THE Promise_Combinators SHALL provide an `any` method that returns the first promise to succeed
//! - 20.5: THE Promise_Combinators SHALL be implemented within a STEP operation to ensure durability

use std::future::Future;
use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::Mutex;

use crate::concurrency::{BatchItem, BatchResult, CompletionReason};
use crate::context::{Logger, LogInfo, OperationIdentifier};
use crate::error::{DurableError, ErrorObject};
use crate::operation::OperationType;
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;

/// Result of a promise combinator operation stored in checkpoint.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct PromiseCombinatorResult<T> {
    /// The results from each promise
    pub results: Vec<PromiseOutcome<T>>,
    /// The index of the winning promise (for race/any)
    pub winner_index: Option<usize>,
    /// The completion reason
    pub completion_reason: PromiseCompletionReason,
}

/// Outcome of a single promise in a combinator.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct PromiseOutcome<T> {
    /// Index of this promise
    pub index: usize,
    /// Whether the promise succeeded
    pub succeeded: bool,
    /// The result value if succeeded
    pub result: Option<T>,
    /// The error if failed
    pub error: Option<ErrorObject>,
}

/// Reason why a promise combinator completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum PromiseCompletionReason {
    /// All promises completed successfully (for `all`)
    AllSucceeded,
    /// All promises settled (for `all_settled`)
    AllSettled,
    /// First promise settled (for `race`)
    FirstSettled,
    /// First promise succeeded (for `any`)
    FirstSucceeded,
    /// All promises failed (for `any` when none succeed)
    AllFailed,
    /// A promise failed (for `all` when one fails)
    OneFailed,
}

impl<T> PromiseOutcome<T> {
    /// Creates a successful outcome.
    pub fn success(index: usize, result: T) -> Self {
        Self {
            index,
            succeeded: true,
            result: Some(result),
            error: None,
        }
    }

    /// Creates a failed outcome.
    pub fn failure(index: usize, error: ErrorObject) -> Self {
        Self {
            index,
            succeeded: false,
            result: None,
            error: Some(error),
        }
    }
}

/// Executes the `all` combinator - waits for all futures to complete successfully.
///
/// Returns all results if all futures succeed, or returns the first error encountered.
/// This is implemented within a STEP operation for durability.
///
/// # Arguments
///
/// * `futures` - Vector of futures to execute
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// `Ok(Vec<T>)` if all futures succeed, or `Err` with the first error.
///
/// # Requirements
///
/// - 20.1: Wait for all promises to complete successfully, return error on first failure
/// - 20.5: Implement within a STEP operation for durability
pub async fn all_handler<T, Fut>(
    futures: Vec<Fut>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<Vec<T>, DurableError>
where
    T: Serialize + DeserializeOwned + Send + Clone + 'static,
    Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
{
    let log_info = create_log_info(state, op_id);
    logger.debug(&format!("Starting 'all' combinator with {} futures", futures.len()), &log_info);

    // Check for replay
    if let Some(result) = check_replay::<Vec<T>>(state, op_id, logger).await? {
        return Ok(result);
    }

    // Checkpoint START
    checkpoint_start(state, op_id).await?;

    let total = futures.len();
    if total == 0 {
        // Empty case - succeed immediately
        checkpoint_succeed(state, op_id, &Vec::<T>::new()).await?;
        return Ok(Vec::new());
    }

    // Execute all futures concurrently using a channel to collect results
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Result<T, DurableError>)>(total);

    for (index, future) in futures.into_iter().enumerate() {
        let tx = tx.clone();
        
        tokio::spawn(async move {
            let result = future.await;
            let _ = tx.send((index, result)).await;
        });
    }
    drop(tx); // Drop the original sender

    // Collect all results
    let mut results: Vec<Option<Result<T, DurableError>>> = (0..total).map(|_| None).collect();
    let mut received = 0;
    
    while let Some((index, result)) = rx.recv().await {
        results[index] = Some(result);
        received += 1;
        if received == total {
            break;
        }
    }

    // Check results and build final vector
    let mut final_results = Vec::with_capacity(total);
    
    for (index, result) in results.into_iter().enumerate() {
        match result {
            Some(Ok(value)) => final_results.push(value),
            Some(Err(e)) => {
                // Checkpoint failure and return error
                let error_obj = ErrorObject::from(&e);
                checkpoint_fail(state, op_id, error_obj).await?;
                return Err(DurableError::UserCode {
                    message: format!("Promise at index {} failed: {}", index, e),
                    error_type: "AllCombinatorError".to_string(),
                    stack_trace: None,
                });
            }
            None => {
                // Should not happen
                let error_obj = ErrorObject::new("InternalError", "Promise result missing");
                checkpoint_fail(state, op_id, error_obj).await?;
                return Err(DurableError::execution("Promise result missing"));
            }
        }
    }    // All succeeded - checkpoint and return
    checkpoint_succeed(state, op_id, &final_results).await?;
    logger.debug("'all' combinator completed successfully", &log_info);
    Ok(final_results)
}

/// Executes the `all_settled` combinator - waits for all futures to settle.
///
/// Returns a BatchResult containing outcomes for all futures, regardless of success or failure.
/// This is implemented within a STEP operation for durability.
///
/// # Arguments
///
/// * `futures` - Vector of futures to execute
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// `BatchResult<T>` containing results for all futures.
///
/// # Requirements
///
/// - 20.2: Wait for all promises to settle (success or failure), return BatchResult with all outcomes
/// - 20.5: Implement within a STEP operation for durability
pub async fn all_settled_handler<T, Fut>(
    futures: Vec<Fut>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<BatchResult<T>, DurableError>
where
    T: Serialize + DeserializeOwned + Send + Clone + 'static,
    Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
{
    let log_info = create_log_info(state, op_id);
    logger.debug(&format!("Starting 'all_settled' combinator with {} futures", futures.len()), &log_info);

    // Check for replay
    if let Some(result) = check_replay_batch::<T>(state, op_id, logger).await? {
        return Ok(result);
    }

    // Checkpoint START
    checkpoint_start(state, op_id).await?;

    let total = futures.len();
    if total == 0 {
        let result = BatchResult::empty();
        checkpoint_succeed_batch(state, op_id, &result).await?;
        return Ok(result);
    }

    // Execute all futures concurrently
    let results: Arc<Mutex<Vec<BatchItem<T>>>> = 
        Arc::new(Mutex::new((0..total).map(BatchItem::pending).collect()));

    let mut handles = Vec::with_capacity(total);
    for (index, future) in futures.into_iter().enumerate() {
        let results = results.clone();
        
        handles.push(tokio::spawn(async move {
            let result = future.await;
            let mut results_guard = results.lock().await;
            
            match result {
                Ok(value) => {
                    results_guard[index] = BatchItem::succeeded(index, value);
                }
                Err(e) => {
                    let error_obj = ErrorObject::from(&e);
                    results_guard[index] = BatchItem::failed(index, error_obj);
                }
            }
        }));
    }

    // Wait for all to complete
    for handle in handles {
        let _ = handle.await;
    }

    // Collect results
    let items = Arc::try_unwrap(results)
        .map_err(|_| DurableError::execution("Failed to unwrap results"))?
        .into_inner();

    let batch_result = BatchResult::new(items, CompletionReason::AllCompleted);
    
    // Checkpoint the result
    checkpoint_succeed_batch(state, op_id, &batch_result).await?;
    logger.debug("'all_settled' combinator completed", &log_info);
    Ok(batch_result)
}

/// Executes the `race` combinator - returns the result of the first future to settle.
///
/// Returns the result (success or failure) of whichever future completes first.
/// This is implemented within a STEP operation for durability.
///
/// # Arguments
///
/// * `futures` - Vector of futures to execute
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result of the first future to settle.
///
/// # Requirements
///
/// - 20.3: Return result of first promise to settle
/// - 20.5: Implement within a STEP operation for durability
pub async fn race_handler<T, Fut>(
    futures: Vec<Fut>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send + Clone + 'static,
    Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
{
    let log_info = create_log_info(state, op_id);
    logger.debug(&format!("Starting 'race' combinator with {} futures", futures.len()), &log_info);

    // Check for replay
    if let Some(result) = check_replay::<T>(state, op_id, logger).await? {
        return Ok(result);
    }

    // Checkpoint START
    checkpoint_start(state, op_id).await?;

    let total = futures.len();
    if total == 0 {
        let error_obj = ErrorObject::new("ValidationError", "race requires at least one future");
        checkpoint_fail(state, op_id, error_obj).await?;
        return Err(DurableError::validation("race requires at least one future"));
    }

    // Use tokio::select! pattern with spawned tasks
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Result<T, DurableError>)>(total);

    for (index, future) in futures.into_iter().enumerate() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = future.await;
            let _ = tx.send((index, result)).await;
        });
    }
    drop(tx); // Drop the original sender

    // Wait for the first result
    if let Some((index, result)) = rx.recv().await {
        match result {
            Ok(value) => {
                checkpoint_succeed(state, op_id, &value).await?;
                logger.debug(&format!("'race' combinator won by future at index {}", index), &log_info);
                Ok(value)
            }
            Err(e) => {
                let error_obj = ErrorObject::from(&e);
                checkpoint_fail(state, op_id, error_obj).await?;
                Err(e)
            }
        }
    } else {
        let error_obj = ErrorObject::new("InternalError", "No futures completed");
        checkpoint_fail(state, op_id, error_obj).await?;
        Err(DurableError::execution("No futures completed"))
    }
}

/// Executes the `any` combinator - returns the result of the first future to succeed.
///
/// Returns the result of the first future to succeed. If all futures fail,
/// returns an error containing all the failures.
/// This is implemented within a STEP operation for durability.
///
/// # Arguments
///
/// * `futures` - Vector of futures to execute
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result of the first future to succeed, or an error if all fail.
///
/// # Requirements
///
/// - 20.4: Return result of first promise to succeed, return error only if all fail
/// - 20.5: Implement within a STEP operation for durability
pub async fn any_handler<T, Fut>(
    futures: Vec<Fut>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send + Clone + 'static,
    Fut: Future<Output = Result<T, DurableError>> + Send + 'static,
{
    let log_info = create_log_info(state, op_id);
    logger.debug(&format!("Starting 'any' combinator with {} futures", futures.len()), &log_info);

    // Check for replay
    if let Some(result) = check_replay::<T>(state, op_id, logger).await? {
        return Ok(result);
    }

    // Checkpoint START
    checkpoint_start(state, op_id).await?;

    let total = futures.len();
    if total == 0 {
        let error_obj = ErrorObject::new("ValidationError", "any requires at least one future");
        checkpoint_fail(state, op_id, error_obj).await?;
        return Err(DurableError::validation("any requires at least one future"));
    }

    // Track results
    let (success_tx, mut success_rx) = tokio::sync::mpsc::channel::<(usize, T)>(1);
    let errors: Arc<Mutex<Vec<(usize, DurableError)>>> = Arc::new(Mutex::new(Vec::new()));
    let completed_count: Arc<std::sync::atomic::AtomicUsize> = 
        Arc::new(std::sync::atomic::AtomicUsize::new(0));

    for (index, future) in futures.into_iter().enumerate() {
        let success_tx = success_tx.clone();
        let errors = errors.clone();
        let completed_count = completed_count.clone();
        
        tokio::spawn(async move {
            let result = future.await;
            match result {
                Ok(value) => {
                    // Try to send success - only first one wins
                    let _ = success_tx.send((index, value)).await;
                }
                Err(e) => {
                    let mut errors_guard = errors.lock().await;
                    errors_guard.push((index, e));
                    drop(errors_guard);
                    
                    // Check if all have failed
                    // Relaxed ordering is sufficient: we only need atomic increment.
                    // The count comparison with total is a simple threshold check.
                    // Requirements: 4.1, 4.6
                    let count = completed_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if count == total {
                        // All failed - close the channel to unblock receiver
                        drop(success_tx);
                    }
                }
            }
        });
    }
    drop(success_tx); // Drop the original sender

    // Wait for first success or all failures
    if let Some((index, value)) = success_rx.recv().await {
        checkpoint_succeed(state, op_id, &value).await?;
        logger.debug(&format!("'any' combinator succeeded with future at index {}", index), &log_info);
        Ok(value)
    } else {
        // All failed
        let errors_guard = errors.lock().await;
        let error_messages: Vec<String> = errors_guard
            .iter()
            .map(|(i, e)| format!("Future {}: {}", i, e))
            .collect();
        let combined_message = format!("All {} futures failed: {}", total, error_messages.join("; "));
        
        let error_obj = ErrorObject::new("AnyCombinatorError", &combined_message);
        checkpoint_fail(state, op_id, error_obj).await?;
        
        logger.debug("'any' combinator failed - all futures failed", &log_info);
        Err(DurableError::UserCode {
            message: combined_message,
            error_type: "AnyCombinatorError".to_string(),
            stack_trace: None,
        })
    }
}

// Helper functions

fn create_log_info(state: &Arc<ExecutionState>, op_id: &OperationIdentifier) -> LogInfo {
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }
    log_info
}

async fn check_replay<T>(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<Option<T>, DurableError>
where
    T: Serialize + DeserializeOwned,
{
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
    
    if !checkpoint_result.is_existent() {
        return Ok(None);
    }

    let log_info = create_log_info(state, op_id);

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
        logger.debug(&format!("Replaying succeeded promise combinator: {}", op_id), &log_info);
        state.track_replay(&op_id.operation_id).await;
        
        if let Some(result_str) = checkpoint_result.result() {
            let serdes = JsonSerDes::<T>::new();
            let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
            let result = serdes.deserialize(result_str, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to deserialize checkpointed result: {}", e),
                })?;
            return Ok(Some(result));
        }
    }

    // Handle failed checkpoint
    if checkpoint_result.is_failed() {
        logger.debug(&format!("Replaying failed promise combinator: {}", op_id), &log_info);
        state.track_replay(&op_id.operation_id).await;
        
        if let Some(error) = checkpoint_result.error() {
            return Err(DurableError::UserCode {
                message: error.error_message.clone(),
                error_type: error.error_type.clone(),
                stack_trace: error.stack_trace.clone(),
            });
        } else {
            return Err(DurableError::execution("Promise combinator failed with unknown error"));
        }
    }

    Ok(None)
}

async fn check_replay_batch<T>(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<Option<BatchResult<T>>, DurableError>
where
    T: Serialize + DeserializeOwned,
{
    check_replay::<BatchResult<T>>(state, op_id, logger).await
}

async fn checkpoint_start(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
) -> Result<(), DurableError> {
    use crate::operation::OperationUpdate;
    
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Step);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update = update.with_sub_type("PromiseCombinator");
    
    state.create_checkpoint(update, true).await
}

async fn checkpoint_succeed<T>(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    result: &T,
) -> Result<(), DurableError>
where
    T: Serialize + DeserializeOwned,
{
    use crate::operation::OperationUpdate;
    
    let serdes = JsonSerDes::<T>::new();
    let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
    let serialized = serdes.serialize(result, &serdes_ctx)
        .map_err(|e| DurableError::SerDes {
            message: format!("Failed to serialize result: {}", e),
        })?;
    
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Step, Some(serialized));
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    
    state.create_checkpoint(update, true).await
}

async fn checkpoint_succeed_batch<T>(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    result: &BatchResult<T>,
) -> Result<(), DurableError>
where
    T: Serialize + DeserializeOwned,
{
    checkpoint_succeed(state, op_id, result).await
}

async fn checkpoint_fail(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    error: ErrorObject,
) -> Result<(), DurableError> {
    use crate::operation::OperationUpdate;
    
    let mut update = OperationUpdate::fail(&op_id.operation_id, OperationType::Step, error);
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    
    state.create_checkpoint(update, true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-3")))
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

    fn create_test_op_id(name: &str) -> OperationIdentifier {
        OperationIdentifier::new(format!("test-op-{}", name), None, Some(name.to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    #[tokio::test]
    async fn test_all_handler_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("all-success");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
            Box::pin(async { Ok(3) }),
        ];

        let result = all_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_all_handler_failure() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("all-failure");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(3) }),
        ];

        let result = all_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_all_handler_empty() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("all-empty");
        let logger = create_test_logger();

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![];

        let result = all_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_all_settled_handler_mixed() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("all-settled-mixed");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("test error")) }),
            Box::pin(async { Ok(3) }),
        ];

        let result = all_settled_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        let batch = result.unwrap();
        assert_eq!(batch.items.len(), 3);
        assert_eq!(batch.success_count(), 2);
        assert_eq!(batch.failure_count(), 1);
    }

    #[tokio::test]
    async fn test_all_settled_handler_all_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("all-settled-success");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Ok(2) }),
        ];

        let result = all_settled_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        let batch = result.unwrap();
        assert!(batch.all_succeeded());
    }

    #[tokio::test]
    async fn test_race_handler_first_wins() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("race-first");
        let logger = create_test_logger();

        // First future completes immediately
        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(2)
            }),
        ];

        let result = race_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        // First one should win
        assert_eq!(result.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_race_handler_error_wins() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("race-error");
        let logger = create_test_logger();

        // Error completes first
        let futures = vec![
            Box::pin(async { Err::<i32, _>(DurableError::execution("fast error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                Ok(2)
            }),
        ];

        let result = race_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_race_handler_empty() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("race-empty");
        let logger = create_test_logger();

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![];

        let result = race_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_any_handler_first_success() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("any-first");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Ok::<_, DurableError>(1) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("error")) }),
            Box::pin(async { Ok(3) }),
        ];

        let result = any_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        // Either 1 or 3 should win (first success)
        let value = result.unwrap();
        assert!(value == 1 || value == 3);
    }

    #[tokio::test]
    async fn test_any_handler_all_fail() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("any-all-fail");
        let logger = create_test_logger();

        let futures = vec![
            Box::pin(async { Err::<i32, _>(DurableError::execution("error 1")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("error 2")) }),
        ];

        let result = any_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_err());
        if let Err(DurableError::UserCode { message, .. }) = result {
            assert!(message.contains("All 2 futures failed"));
        } else {
            panic!("Expected UserCode error");
        }
    }

    #[tokio::test]
    async fn test_any_handler_empty() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("any-empty");
        let logger = create_test_logger();

        let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = vec![];

        let result = any_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_any_handler_success_after_failures() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id("any-success-after-fail");
        let logger = create_test_logger();

        // First two fail immediately, third succeeds after delay
        let futures = vec![
            Box::pin(async { Err::<i32, _>(DurableError::execution("error 1")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>,
            Box::pin(async { Err(DurableError::execution("error 2")) }),
            Box::pin(async {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                Ok(42)
            }),
        ];

        let result = any_handler(futures, &state, &op_id, &logger).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use std::pin::Pin;
    use proptest::prelude::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::lambda::InitialExecutionState;

    fn create_mock_client_with_responses(count: usize) -> SharedDurableServiceClient {
        let mut client = MockDurableServiceClient::new();
        for i in 0..count {
            client = client.with_checkpoint_response(Ok(CheckpointResponse::new(format!("token-{}", i))));
        }
        Arc::new(client)
    }

    fn create_test_state(client: SharedDurableServiceClient) -> Arc<ExecutionState> {
        Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            InitialExecutionState::new(),
            client,
        ))
    }

    fn create_test_op_id(name: &str) -> OperationIdentifier {
        OperationIdentifier::new(format!("test-op-{}", name), None, Some(name.to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    /// **Feature: durable-execution-rust-sdk, Property 13: Promise Combinator Correctness**
    /// **Validates: Requirements 20.1, 20.2, 20.3, 20.4**
    ///
    /// For any set of futures passed to promise combinators:
    /// - `all` SHALL return all results only when all futures succeed, or fail on first error
    /// - `all_settled` SHALL return results for all futures regardless of success/failure
    /// - `race` SHALL return the result of the first future to settle
    /// - `any` SHALL return the result of the first future to succeed
    mod promise_combinator_tests {
        use super::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: `all` returns all results when all futures succeed
            /// For any number of successful futures, `all` returns a vector with all results.
            #[test]
            fn prop_all_returns_all_results_on_success(
                values in prop::collection::vec(0i32..1000, 1..10),
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = create_mock_client_with_responses(10);
                    let state = create_test_state(client);
                    let op_id = create_test_op_id(&format!("all-prop-{}", values.len()));
                    let logger = create_test_logger();

                    let expected = values.clone();
                    let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = 
                        values.into_iter()
                            .map(|v| Box::pin(async move { Ok(v) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>)
                            .collect();

                    let result = all_handler(futures, &state, &op_id, &logger).await;
                    
                    prop_assert!(result.is_ok(), "all should succeed when all futures succeed");
                    prop_assert_eq!(result.unwrap(), expected, "all should return all results in order");
                    Ok(())
                })?;
            }

            /// Property test: `all` fails on first error
            /// For any set of futures where at least one fails, `all` returns an error.
            #[test]
            fn prop_all_fails_on_any_error(
                success_count in 0usize..5,
                error_index in 0usize..10,
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let total = success_count + 1; // At least one error
                    let error_index = error_index % total;
                    
                    let client = create_mock_client_with_responses(10);
                    let state = create_test_state(client);
                    let op_id = create_test_op_id(&format!("all-fail-prop-{}", total));
                    let logger = create_test_logger();

                    let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = 
                        (0..total)
                            .map(|i| {
                                if i == error_index {
                                    Box::pin(async move { Err(DurableError::execution("test error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>
                                } else {
                                    Box::pin(async move { Ok(i as i32) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>
                                }
                            })
                            .collect();

                    let result = all_handler(futures, &state, &op_id, &logger).await;
                    
                    prop_assert!(result.is_err(), "all should fail when any future fails");
                    Ok(())
                })?;
            }

            /// Property test: `all_settled` returns results for all futures
            /// For any mix of successes and failures, `all_settled` returns a BatchResult
            /// with the correct count of successes and failures.
            #[test]
            fn prop_all_settled_returns_all_outcomes(
                success_indices in prop::collection::vec(0usize..10, 0..10),
                total in 1usize..10,
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = create_mock_client_with_responses(10);
                    let state = create_test_state(client);
                    let op_id = create_test_op_id(&format!("all-settled-prop-{}", total));
                    let logger = create_test_logger();

                    let success_set: std::collections::HashSet<usize> = 
                        success_indices.into_iter().filter(|&i| i < total).collect();
                    let expected_successes = success_set.len();
                    let expected_failures = total - expected_successes;

                    let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = 
                        (0..total)
                            .map(|i| {
                                if success_set.contains(&i) {
                                    Box::pin(async move { Ok(i as i32) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>
                                } else {
                                    Box::pin(async move { Err(DurableError::execution("test error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>
                                }
                            })
                            .collect();

                    let result = all_settled_handler(futures, &state, &op_id, &logger).await;
                    
                    prop_assert!(result.is_ok(), "all_settled should always succeed");
                    let batch = result.unwrap();
                    prop_assert_eq!(batch.total_count(), total, "all_settled should return all items");
                    prop_assert_eq!(batch.success_count(), expected_successes, "success count should match");
                    prop_assert_eq!(batch.failure_count(), expected_failures, "failure count should match");
                    Ok(())
                })?;
            }

            /// Property test: `any` succeeds if at least one future succeeds
            /// For any set of futures where at least one succeeds, `any` returns a success.
            #[test]
            fn prop_any_succeeds_if_one_succeeds(
                failure_count in 0usize..5,
                success_value in 0i32..1000,
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = create_mock_client_with_responses(10);
                    let state = create_test_state(client);
                    let op_id = create_test_op_id(&format!("any-prop-{}", failure_count));
                    let logger = create_test_logger();

                    // Create futures: some failures followed by one success
                    let mut futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = 
                        (0..failure_count)
                            .map(|_| Box::pin(async { Err(DurableError::execution("test error")) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>)
                            .collect();
                    
                    // Add the success
                    futures.push(Box::pin(async move { Ok(success_value) }));

                    let result = any_handler(futures, &state, &op_id, &logger).await;
                    
                    prop_assert!(result.is_ok(), "any should succeed when at least one future succeeds");
                    prop_assert_eq!(result.unwrap(), success_value, "any should return the successful value");
                    Ok(())
                })?;
            }

            /// Property test: `any` fails only when all futures fail
            /// For any set of futures where all fail, `any` returns an error.
            #[test]
            fn prop_any_fails_when_all_fail(
                failure_count in 1usize..10,
            ) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    let client = create_mock_client_with_responses(10);
                    let state = create_test_state(client);
                    let op_id = create_test_op_id(&format!("any-all-fail-prop-{}", failure_count));
                    let logger = create_test_logger();

                    let futures: Vec<Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>> = 
                        (0..failure_count)
                            .map(|i| Box::pin(async move { Err(DurableError::execution(format!("error {}", i))) }) as Pin<Box<dyn Future<Output = Result<i32, DurableError>> + Send>>)
                            .collect();

                    let result = any_handler(futures, &state, &op_id, &logger).await;
                    
                    prop_assert!(result.is_err(), "any should fail when all futures fail");
                    if let Err(DurableError::UserCode { message, .. }) = result {
                        prop_assert!(message.contains(&format!("All {} futures failed", failure_count)), 
                            "Error message should indicate all futures failed");
                    }
                    Ok(())
                })?;
            }
        }
    }
}
