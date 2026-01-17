//! Checkpoint batching for efficient API calls.
//!
//! This module provides the checkpoint batching system that collects checkpoint
//! requests and sends them in batches to reduce API calls to the Lambda service.
//!
//! ## Checkpoint Token Management
//!
//! The batcher manages checkpoint tokens according to the following rules:
//!
//! 1. **First Checkpoint**: Uses the initial `CheckpointToken` from the Lambda invocation input
//! 2. **Subsequent Checkpoints**: Uses the token returned from the previous checkpoint response
//! 3. **Token Consumption**: Each token can only be used once; the batcher automatically
//!    updates to the new token after each successful checkpoint
//! 4. **Error Handling**: If a checkpoint fails with "Invalid checkpoint token", the error
//!    is marked as retriable so Lambda can retry with a fresh token
//!
//! ## Requirements
//!
//! - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
//! - 2.10: THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
//! - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry
//! - 2.12: WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
//! - 2.13: THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::{timeout, Instant};

use crate::client::SharedDurableServiceClient;
use crate::error::DurableError;
use crate::operation::{OperationAction, OperationType, OperationUpdate};

/// Configuration for the checkpoint batcher.
#[derive(Debug, Clone)]
pub struct CheckpointBatcherConfig {
    /// Maximum size of a batch in bytes (default: 750KB)
    pub max_batch_size_bytes: usize,
    /// Maximum time to wait before sending a batch (default: 1 second)
    pub max_batch_time_ms: u64,
    /// Maximum number of operations per batch (default: unlimited/usize::MAX)
    pub max_batch_operations: usize,
}

impl Default for CheckpointBatcherConfig {
    fn default() -> Self {
        Self {
            max_batch_size_bytes: 750 * 1024, // 750KB
            max_batch_time_ms: 1000,          // 1 second
            max_batch_operations: usize::MAX, // unlimited
        }
    }
}

/// A request to checkpoint an operation.
///
/// This struct is sent through the checkpoint queue to the batcher.
/// It includes an optional completion channel for synchronous checkpoints.
#[derive(Debug)]
pub struct CheckpointRequest {
    /// The operation update to checkpoint
    pub operation: OperationUpdate,
    /// Optional channel to signal completion (for sync checkpoints)
    pub completion: Option<oneshot::Sender<Result<(), DurableError>>>,
}

impl CheckpointRequest {
    /// Creates a new synchronous checkpoint request.
    ///
    /// Returns the request and a receiver to wait for completion.
    pub fn sync(operation: OperationUpdate) -> (Self, oneshot::Receiver<Result<(), DurableError>>) {
        let (tx, rx) = oneshot::channel();
        (
            Self {
                operation,
                completion: Some(tx),
            },
            rx,
        )
    }

    /// Creates a new asynchronous (fire-and-forget) checkpoint request.
    pub fn async_request(operation: OperationUpdate) -> Self {
        Self {
            operation,
            completion: None,
        }
    }

    /// Returns true if this is a synchronous request.
    pub fn is_sync(&self) -> bool {
        self.completion.is_some()
    }

    /// Estimates the size of this request in bytes for batching.
    pub fn estimated_size(&self) -> usize {
        // Estimate based on JSON serialization
        // operation_id + operation_type + action + result + error + parent_id + name
        let base_size = 100; // Base overhead for JSON structure
        let op = &self.operation;

        base_size
            + op.operation_id.len()
            + op.result.as_ref().map(|r| r.len()).unwrap_or(0)
            + op.error
                .as_ref()
                .map(|e| e.error_message.len() + e.error_type.len())
                .unwrap_or(0)
            + op.parent_id.as_ref().map(|p| p.len()).unwrap_or(0)
            + op.name.as_ref().map(|n| n.len()).unwrap_or(0)
    }
}

/// Result of processing a batch of checkpoints.
#[derive(Debug)]
pub struct BatchResult {
    /// Whether the batch was successfully sent
    pub success: bool,
    /// The new checkpoint token if successful
    pub new_token: Option<String>,
    /// Error if the batch failed
    pub error: Option<DurableError>,
}

/// The checkpoint batcher collects checkpoint requests and sends them in batches.
///
/// This improves efficiency by reducing the number of API calls to the Lambda service.
/// The batcher sends a batch when any of these conditions are met:
/// - The batch reaches the maximum size in bytes
/// - The batch reaches the maximum number of operations
/// - The maximum batch time has elapsed
pub struct CheckpointBatcher {
    /// Configuration for batching behavior
    config: CheckpointBatcherConfig,
    /// Receiver for checkpoint requests
    queue_rx: mpsc::Receiver<CheckpointRequest>,
    /// Service client for sending checkpoints
    service_client: SharedDurableServiceClient,
    /// Reference to the execution state for updating tokens
    durable_execution_arn: String,
    /// Current checkpoint token (shared with ExecutionState)
    checkpoint_token: Arc<RwLock<String>>,
    /// Tracks whether the initial token has been consumed
    initial_token_consumed: AtomicBool,
}

impl CheckpointBatcher {
    /// Creates a new CheckpointBatcher.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for batching behavior
    /// * `queue_rx` - Receiver for checkpoint requests
    /// * `service_client` - Service client for sending checkpoints
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The initial checkpoint token from Lambda invocation input
    ///
    /// # Requirements
    ///
    /// - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
    pub fn new(
        config: CheckpointBatcherConfig,
        queue_rx: mpsc::Receiver<CheckpointRequest>,
        service_client: SharedDurableServiceClient,
        durable_execution_arn: String,
        checkpoint_token: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            config,
            queue_rx,
            service_client,
            durable_execution_arn,
            checkpoint_token,
            initial_token_consumed: AtomicBool::new(false),
        }
    }

    /// Runs the batcher loop, processing checkpoint requests.
    ///
    /// This method runs until the queue is closed (sender dropped).
    pub async fn run(&mut self) {
        loop {
            let batch = self.collect_batch().await;
            if batch.is_empty() {
                // Queue closed, exit
                break;
            }
            self.process_batch(batch).await;
        }
    }

    /// Collects a batch of checkpoint requests.
    ///
    /// Returns when any batch limit is reached or the queue is closed.
    async fn collect_batch(&mut self) -> Vec<CheckpointRequest> {
        let mut batch = Vec::new();
        let mut batch_size = 0usize;
        let batch_deadline =
            Instant::now() + StdDuration::from_millis(self.config.max_batch_time_ms);

        // Wait for the first request (blocking)
        match self.queue_rx.recv().await {
            Some(request) => {
                batch_size += request.estimated_size();
                batch.push(request);
            }
            None => return batch, // Queue closed
        }

        // Collect more requests until limits are reached
        loop {
            // Check if we've hit operation count limit
            if batch.len() >= self.config.max_batch_operations {
                break;
            }

            // Calculate remaining time until deadline
            let now = Instant::now();
            if now >= batch_deadline {
                break;
            }
            let remaining = batch_deadline - now;

            // Try to receive more requests with timeout
            match timeout(remaining, self.queue_rx.recv()).await {
                Ok(Some(request)) => {
                    let request_size = request.estimated_size();

                    // Check if adding this request would exceed size limit
                    if batch_size + request_size > self.config.max_batch_size_bytes
                        && !batch.is_empty()
                    {
                        // Include this request and break - next batch will handle any overflow
                        batch.push(request);
                        break;
                    }

                    batch_size += request_size;
                    batch.push(request);
                }
                Ok(None) => break, // Queue closed
                Err(_) => break,   // Timeout reached
            }
        }

        batch
    }

    /// Sorts checkpoint requests according to the ordering rules.
    ///
    /// The ordering rules are:
    /// 1. Operations are checkpointed in execution order (preserving original order)
    /// 2. EXECUTION completion (SUCCEED/FAIL on EXECUTION type) must be last in the batch
    /// 3. Child operations must come after their parent CONTEXT starts
    /// 4. START and completion (SUCCEED/FAIL) for the same operation can be in the same batch
    ///
    /// # Requirements
    ///
    /// - 2.12: WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
    /// - 2.13: THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch
    pub fn sort_checkpoint_batch(batch: Vec<CheckpointRequest>) -> Vec<CheckpointRequest> {
        if batch.len() <= 1 {
            return batch;
        }

        // Step 1: Identify CONTEXT START operations and build parent-child relationships
        let context_starts: HashSet<String> = batch
            .iter()
            .filter(|req| {
                req.operation.operation_type == OperationType::Context
                    && req.operation.action == OperationAction::Start
            })
            .map(|req| req.operation.operation_id.clone())
            .collect();

        // Build a map of operation_id -> parent_id for operations in this batch
        let parent_map: HashMap<String, Option<String>> = batch
            .iter()
            .map(|req| {
                (
                    req.operation.operation_id.clone(),
                    req.operation.parent_id.clone(),
                )
            })
            .collect();

        // Helper function to check if an operation is an ancestor of another
        let is_ancestor = |operation_id: &str, ancestor_id: &str| -> bool {
            let mut current = parent_map.get(operation_id).and_then(|p| p.as_ref());
            while let Some(parent_id) = current {
                if parent_id == ancestor_id {
                    return true;
                }
                current = parent_map.get(parent_id).and_then(|p| p.as_ref());
            }
            false
        };

        // Step 2: Sort with custom comparator
        let mut indexed_batch: Vec<(usize, CheckpointRequest)> =
            batch.into_iter().enumerate().collect();

        indexed_batch.sort_by(|(idx_a, a), (idx_b, b)| {
            // Priority 1: EXECUTION completion must be last
            let a_is_exec_completion = a.operation.operation_type == OperationType::Execution
                && matches!(
                    a.operation.action,
                    OperationAction::Succeed | OperationAction::Fail
                );
            let b_is_exec_completion = b.operation.operation_type == OperationType::Execution
                && matches!(
                    b.operation.action,
                    OperationAction::Succeed | OperationAction::Fail
                );

            if a_is_exec_completion && !b_is_exec_completion {
                return std::cmp::Ordering::Greater;
            }
            if !a_is_exec_completion && b_is_exec_completion {
                return std::cmp::Ordering::Less;
            }

            // Priority 2: Parent CONTEXT START must come before child operations
            if b.operation.action == OperationAction::Start
                && context_starts.contains(&b.operation.operation_id)
            {
                if let Some(ref parent_id) = a.operation.parent_id {
                    if *parent_id == b.operation.operation_id
                        || is_ancestor(&a.operation.operation_id, &b.operation.operation_id)
                    {
                        return std::cmp::Ordering::Greater;
                    }
                }
            }
            if a.operation.action == OperationAction::Start
                && context_starts.contains(&a.operation.operation_id)
            {
                if let Some(ref parent_id) = b.operation.parent_id {
                    if *parent_id == a.operation.operation_id
                        || is_ancestor(&b.operation.operation_id, &a.operation.operation_id)
                    {
                        return std::cmp::Ordering::Less;
                    }
                }
            }

            // Priority 3: For same operation_id, START comes before completion
            if a.operation.operation_id == b.operation.operation_id {
                let a_is_start = a.operation.action == OperationAction::Start;
                let b_is_start = b.operation.action == OperationAction::Start;
                if a_is_start && !b_is_start {
                    return std::cmp::Ordering::Less;
                }
                if !a_is_start && b_is_start {
                    return std::cmp::Ordering::Greater;
                }
            }

            // Priority 4: Preserve original order (stable sort)
            idx_a.cmp(idx_b)
        });

        // Extract the sorted requests
        indexed_batch.into_iter().map(|(_, req)| req).collect()
    }

    /// Processes a batch of checkpoint requests.
    ///
    /// # Requirements
    ///
    /// - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
    /// - 2.10: THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
    /// - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry
    /// - 2.12: WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
    /// - 2.13: THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    async fn process_batch(&self, batch: Vec<CheckpointRequest>) {
        if batch.is_empty() {
            return;
        }

        // Sort the batch according to checkpoint ordering rules
        let sorted_batch = Self::sort_checkpoint_batch(batch);

        // Extract operations and completion channels
        let (operations, completions): (Vec<_>, Vec<_>) = sorted_batch
            .into_iter()
            .map(|req| (req.operation, req.completion))
            .unzip();

        // Get current checkpoint token
        let token = self.checkpoint_token.read().await.clone();

        // Send the batch to the service with retry for throttling
        let result = self.checkpoint_with_retry(&token, operations).await;

        // Handle the result
        match result {
            Ok(response) => {
                // Mark that we've consumed the initial token.
                // Release ordering ensures that the checkpoint_token write (via RwLock)
                // is visible to any thread that subsequently reads initial_token_consumed.
                // This is a one-way transition (false -> true) that never reverts.
                //
                // Requirements: 4.4, 4.6
                self.initial_token_consumed.store(true, Ordering::Release);

                // Update the checkpoint token with the new token from the response
                {
                    let mut token_guard = self.checkpoint_token.write().await;
                    *token_guard = response.checkpoint_token;
                }

                // Signal success to all waiting callers
                for completion in completions.into_iter().flatten() {
                    let _ = completion.send(Ok(()));
                }
            }
            Err(error) => {
                // Check if this is an invalid checkpoint token error
                let is_invalid_token = error.is_invalid_checkpoint_token();

                // Signal failure to all waiting callers
                for completion in completions.into_iter().flatten() {
                    let error_msg = if is_invalid_token {
                        format!(
                            "Invalid checkpoint token - token may have been consumed. \
                             Lambda will retry with a fresh token. Original error: {}",
                            error
                        )
                    } else {
                        error.to_string()
                    };

                    let _ = completion.send(Err(DurableError::Checkpoint {
                        message: error_msg,
                        is_retriable: error.is_retriable(),
                        aws_error: None,
                    }));
                }
            }
        }
    }

    /// Sends a checkpoint request with retry for throttling errors.
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    async fn checkpoint_with_retry(
        &self,
        token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<crate::client::CheckpointResponse, DurableError> {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_DELAY_MS: u64 = 100;
        const MAX_DELAY_MS: u64 = 10_000;
        const BACKOFF_MULTIPLIER: u64 = 2;

        let mut attempt = 0;
        let mut delay_ms = INITIAL_DELAY_MS;

        loop {
            let result = self
                .service_client
                .checkpoint(&self.durable_execution_arn, token, operations.clone())
                .await;

            match result {
                Ok(response) => return Ok(response),
                Err(error) if error.is_throttling() => {
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        tracing::warn!(
                            attempt = attempt,
                            "Checkpoint throttling: max retries exceeded"
                        );
                        return Err(error);
                    }

                    let actual_delay = error.get_retry_after_ms().unwrap_or(delay_ms);

                    tracing::debug!(
                        attempt = attempt,
                        delay_ms = actual_delay,
                        "Checkpoint throttled, retrying with exponential backoff"
                    );

                    tokio::time::sleep(StdDuration::from_millis(actual_delay)).await;
                    delay_ms = (delay_ms * BACKOFF_MULTIPLIER).min(MAX_DELAY_MS);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// Handle for sending checkpoint requests to the batcher.
#[derive(Clone)]
pub struct CheckpointSender {
    /// Channel for sending requests to the batcher
    pub tx: mpsc::Sender<CheckpointRequest>,
}

impl CheckpointSender {
    /// Creates a new CheckpointSender.
    pub fn new(tx: mpsc::Sender<CheckpointRequest>) -> Self {
        Self { tx }
    }

    /// Sends a synchronous checkpoint request and waits for completion.
    ///
    /// This method blocks until the checkpoint is confirmed or fails.
    pub async fn checkpoint_sync(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        let (request, rx) = CheckpointRequest::sync(operation);

        self.tx
            .send(request)
            .await
            .map_err(|_| DurableError::Checkpoint {
                message: "Checkpoint queue closed".to_string(),
                is_retriable: false,
                aws_error: None,
            })?;

        rx.await.map_err(|_| DurableError::Checkpoint {
            message: "Checkpoint completion channel closed".to_string(),
            is_retriable: false,
            aws_error: None,
        })?
    }

    /// Sends an asynchronous checkpoint request (fire-and-forget).
    ///
    /// This method returns immediately without waiting for confirmation.
    pub async fn checkpoint_async(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        let request = CheckpointRequest::async_request(operation);

        self.tx
            .send(request)
            .await
            .map_err(|_| DurableError::Checkpoint {
                message: "Checkpoint queue closed".to_string(),
                is_retriable: false,
                aws_error: None,
            })
    }

    /// Sends a checkpoint request with configurable sync/async behavior.
    pub async fn checkpoint(
        &self,
        operation: OperationUpdate,
        is_sync: bool,
    ) -> Result<(), DurableError> {
        if is_sync {
            self.checkpoint_sync(operation).await
        } else {
            self.checkpoint_async(operation).await
        }
    }
}

/// Creates a checkpoint queue with the given buffer size.
///
/// Returns a sender for submitting checkpoint requests and a receiver
/// for the batcher to process them.
pub fn create_checkpoint_queue(
    buffer_size: usize,
) -> (CheckpointSender, mpsc::Receiver<CheckpointRequest>) {
    let (tx, rx) = mpsc::channel(buffer_size);
    (CheckpointSender::new(tx), rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient};
    use crate::error::ErrorObject;

    fn create_test_update(id: &str) -> OperationUpdate {
        OperationUpdate::start(id, OperationType::Step)
    }

    fn create_start_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::start(id, op_type)
    }

    fn create_succeed_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::succeed(id, op_type, Some("result".to_string()))
    }

    fn create_fail_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::fail(id, op_type, ErrorObject::new("Error", "message"))
    }

    fn create_request(update: OperationUpdate) -> CheckpointRequest {
        CheckpointRequest::async_request(update)
    }

    // Checkpoint request tests
    #[test]
    fn test_checkpoint_request_sync() {
        let update = create_test_update("op-1");
        let (request, _rx) = CheckpointRequest::sync(update);

        assert!(request.is_sync());
        assert_eq!(request.operation.operation_id, "op-1");
    }

    #[test]
    fn test_checkpoint_request_async() {
        let update = create_test_update("op-1");
        let request = CheckpointRequest::async_request(update);

        assert!(!request.is_sync());
        assert_eq!(request.operation.operation_id, "op-1");
    }

    #[test]
    fn test_checkpoint_request_estimated_size() {
        let update = create_test_update("op-1");
        let request = CheckpointRequest::async_request(update);

        let size = request.estimated_size();
        assert!(size > 0);
        assert!(size >= 100);
    }

    #[test]
    fn test_checkpoint_request_estimated_size_with_result() {
        let mut update = create_test_update("op-1");
        update.result = Some("a".repeat(1000));
        let request = CheckpointRequest::async_request(update);

        let size = request.estimated_size();
        assert!(size >= 1100);
    }

    #[test]
    fn test_create_checkpoint_queue() {
        let (sender, _rx) = create_checkpoint_queue(100);
        drop(sender);
    }

    #[tokio::test]
    async fn test_checkpoint_sender_sync() {
        let (sender, mut rx) = create_checkpoint_queue(10);

        let handle = tokio::spawn(async move {
            if let Some(request) = rx.recv().await {
                assert!(request.is_sync());
                if let Some(completion) = request.completion {
                    let _ = completion.send(Ok(()));
                }
            }
        });

        let update = create_test_update("op-1");
        let result = sender.checkpoint_sync(update).await;
        assert!(result.is_ok());

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_checkpoint_sender_async() {
        let (sender, mut rx) = create_checkpoint_queue(10);

        let update = create_test_update("op-1");
        let result = sender.checkpoint_async(update).await;
        assert!(result.is_ok());

        let request = rx.recv().await.unwrap();
        assert!(!request.is_sync());
        assert_eq!(request.operation.operation_id, "op-1");
    }

    #[tokio::test]
    async fn test_checkpoint_sender_queue_closed() {
        let (sender, rx) = create_checkpoint_queue(10);
        drop(rx);

        let update = create_test_update("op-1");
        let result = sender.checkpoint_async(update).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_processes_batch() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

        let (sender, rx) = create_checkpoint_queue(10);
        let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

        let config = CheckpointBatcherConfig {
            max_batch_time_ms: 10,
            ..Default::default()
        };

        let mut batcher = CheckpointBatcher::new(
            config,
            rx,
            client,
            "arn:test".to_string(),
            checkpoint_token.clone(),
        );

        let update = create_test_update("op-1");
        let (request, completion_rx) = CheckpointRequest::sync(update);
        sender.tx.send(request).await.unwrap();

        drop(sender);
        batcher.run().await;

        let result = completion_rx.await.unwrap();
        assert!(result.is_ok());

        let token = checkpoint_token.read().await;
        assert_eq!(*token, "new-token");
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_handles_error() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Test error"))),
        );

        let (sender, rx) = create_checkpoint_queue(10);
        let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

        let config = CheckpointBatcherConfig {
            max_batch_time_ms: 10,
            ..Default::default()
        };

        let mut batcher = CheckpointBatcher::new(
            config,
            rx,
            client,
            "arn:test".to_string(),
            checkpoint_token.clone(),
        );

        let update = create_test_update("op-1");
        let (request, completion_rx) = CheckpointRequest::sync(update);
        sender.tx.send(request).await.unwrap();

        drop(sender);
        batcher.run().await;

        let result = completion_rx.await.unwrap();
        assert!(result.is_err());

        let token = checkpoint_token.read().await;
        assert_eq!(*token, "initial-token");
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_batches_multiple_requests() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

        let (sender, rx) = create_checkpoint_queue(10);
        let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

        let config = CheckpointBatcherConfig {
            max_batch_time_ms: 50,
            max_batch_operations: 3,
            ..Default::default()
        };

        let mut batcher = CheckpointBatcher::new(
            config,
            rx,
            client,
            "arn:test".to_string(),
            checkpoint_token.clone(),
        );

        for i in 0..3 {
            let update = create_test_update(&format!("op-{}", i));
            sender
                .tx
                .send(CheckpointRequest::async_request(update))
                .await
                .unwrap();
        }

        drop(sender);
        batcher.run().await;

        let token = checkpoint_token.read().await;
        assert_eq!(*token, "new-token");
    }

    // Batch ordering tests
    #[test]
    fn test_execution_completion_is_last() {
        let batch = vec![
            create_request(create_succeed_update("exec-1", OperationType::Execution)),
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_succeed_update("step-2", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[2].operation.operation_id, "exec-1");
        assert_eq!(sorted[2].operation.action, OperationAction::Succeed);
        assert_eq!(sorted[2].operation.operation_type, OperationType::Execution);
    }

    #[test]
    fn test_execution_fail_is_last() {
        let batch = vec![
            create_request(create_fail_update("exec-1", OperationType::Execution)),
            create_request(create_start_update("step-1", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[1].operation.operation_id, "exec-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Fail);
    }

    #[test]
    fn test_child_after_parent_context_start() {
        let mut child_update = create_start_update("child-1", OperationType::Step);
        child_update.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(child_update),
            create_request(create_start_update("parent-ctx", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "parent-ctx");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "child-1");
    }

    #[test]
    fn test_start_and_completion_same_operation() {
        let batch = vec![
            create_request(create_succeed_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-1", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
    }

    #[test]
    fn test_context_start_and_completion_same_batch() {
        let batch = vec![
            create_request(create_succeed_update("ctx-1", OperationType::Context)),
            create_request(create_start_update("ctx-1", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "ctx-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "ctx-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
    }

    #[test]
    fn test_step_start_and_fail_same_batch() {
        let batch = vec![
            create_request(create_fail_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-1", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Fail);
    }

    #[test]
    fn test_complex_ordering_scenario() {
        let mut child1 = create_start_update("child-1", OperationType::Step);
        child1.parent_id = Some("parent-ctx".to_string());

        let mut child2 = create_succeed_update("child-2", OperationType::Step);
        child2.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(create_succeed_update("exec-1", OperationType::Execution)),
            create_request(child1),
            create_request(create_succeed_update("parent-ctx", OperationType::Context)),
            create_request(create_start_update("parent-ctx", OperationType::Context)),
            create_request(child2),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 5);

        let parent_start_pos = sorted
            .iter()
            .position(|r| {
                r.operation.operation_id == "parent-ctx"
                    && r.operation.action == OperationAction::Start
            })
            .unwrap();
        let parent_succeed_pos = sorted
            .iter()
            .position(|r| {
                r.operation.operation_id == "parent-ctx"
                    && r.operation.action == OperationAction::Succeed
            })
            .unwrap();
        let child1_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "child-1")
            .unwrap();
        let child2_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "child-2")
            .unwrap();
        let exec_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "exec-1")
            .unwrap();

        assert!(parent_start_pos < parent_succeed_pos);
        assert!(parent_start_pos < child1_pos);
        assert!(parent_start_pos < child2_pos);
        assert_eq!(exec_pos, 4);
    }

    #[test]
    fn test_empty_batch() {
        let batch: Vec<CheckpointRequest> = vec![];
        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_single_item_batch() {
        let batch = vec![create_request(create_start_update(
            "step-1",
            OperationType::Step,
        ))];
        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
    }

    #[test]
    fn test_preserves_original_order() {
        let batch = vec![
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-2", OperationType::Step)),
            create_request(create_start_update("step-3", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.operation_id, "step-2");
        assert_eq!(sorted[2].operation.operation_id, "step-3");
    }

    #[test]
    fn test_multiple_children_same_parent() {
        let mut child1 = create_start_update("child-1", OperationType::Step);
        child1.parent_id = Some("parent-ctx".to_string());

        let mut child2 = create_start_update("child-2", OperationType::Step);
        child2.parent_id = Some("parent-ctx".to_string());

        let mut child3 = create_start_update("child-3", OperationType::Step);
        child3.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(child1),
            create_request(child2),
            create_request(create_start_update("parent-ctx", OperationType::Context)),
            create_request(child3),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted[0].operation.operation_id, "parent-ctx");

        for i in 1..4 {
            assert!(sorted[i].operation.parent_id.as_deref() == Some("parent-ctx"));
        }
    }

    #[test]
    fn test_nested_contexts() {
        let mut parent_ctx = create_start_update("parent-ctx", OperationType::Context);
        parent_ctx.parent_id = Some("grandparent-ctx".to_string());

        let mut child = create_start_update("child-1", OperationType::Step);
        child.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(child),
            create_request(parent_ctx),
            create_request(create_start_update(
                "grandparent-ctx",
                OperationType::Context,
            )),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        let grandparent_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "grandparent-ctx")
            .unwrap();
        let parent_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "parent-ctx")
            .unwrap();
        let child_pos = sorted
            .iter()
            .position(|r| r.operation.operation_id == "child-1")
            .unwrap();

        assert!(grandparent_pos < parent_pos);
        assert!(parent_pos < child_pos);
    }

    #[test]
    fn test_execution_start_not_affected() {
        let batch = vec![
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_start_update("exec-1", OperationType::Execution)),
            create_request(create_start_update("step-2", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.operation_id, "exec-1");
        assert_eq!(sorted[2].operation.operation_id, "step-2");
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::client::{CheckpointResponse, DurableServiceClient, GetOperationsResponse};
    use async_trait::async_trait;
    use proptest::prelude::*;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    struct CountingMockClient {
        checkpoint_count: Arc<AtomicUsize>,
    }

    impl CountingMockClient {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    checkpoint_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl DurableServiceClient for CountingMockClient {
        async fn checkpoint(
            &self,
            _durable_execution_arn: &str,
            _checkpoint_token: &str,
            _operations: Vec<OperationUpdate>,
        ) -> Result<CheckpointResponse, DurableError> {
            self.checkpoint_count.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(CheckpointResponse::new(format!(
                "token-{}",
                self.checkpoint_count.load(AtomicOrdering::SeqCst)
            )))
        }

        async fn get_operations(
            &self,
            _durable_execution_arn: &str,
            _next_marker: &str,
        ) -> Result<GetOperationsResponse, DurableError> {
            Ok(GetOperationsResponse {
                operations: vec![],
                next_marker: None,
            })
        }
    }

    fn create_test_update_with_size(id: &str, result_size: usize) -> OperationUpdate {
        let mut update = OperationUpdate::start(id, OperationType::Step);
        if result_size > 0 {
            update.result = Some("x".repeat(result_size));
        }
        update
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn prop_checkpoint_batching_respects_operation_count_limit(
            num_requests in 1usize..20,
            max_ops_per_batch in 1usize..10,
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let (client, call_count) = CountingMockClient::new();
                let client = Arc::new(client);

                let (sender, rx) = create_checkpoint_queue(100);
                let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

                let config = CheckpointBatcherConfig {
                    max_batch_time_ms: 10,
                    max_batch_operations: max_ops_per_batch,
                    max_batch_size_bytes: usize::MAX,
                };

                let mut batcher = CheckpointBatcher::new(
                    config,
                    rx,
                    client,
                    "arn:test".to_string(),
                    checkpoint_token.clone(),
                );

                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), 0);
                    sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
                }

                drop(sender);
                batcher.run().await;

                let expected_max_calls = (num_requests + max_ops_per_batch - 1) / max_ops_per_batch;
                let actual_calls = call_count.load(AtomicOrdering::SeqCst);

                if actual_calls > expected_max_calls {
                    return Err(TestCaseError::fail(format!(
                        "Expected at most {} API calls for {} requests with batch size {}, got {}",
                        expected_max_calls, num_requests, max_ops_per_batch, actual_calls
                    )));
                }

                if actual_calls < 1 {
                    return Err(TestCaseError::fail(format!(
                        "Expected at least 1 API call for {} requests, got {}",
                        num_requests, actual_calls
                    )));
                }

                Ok(())
            });
            result?;
        }

        #[test]
        fn prop_checkpoint_batching_respects_size_limit(
            num_requests in 1usize..10,
            result_size in 100usize..500,
            max_batch_size in 500usize..2000,
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let (client, call_count) = CountingMockClient::new();
                let client = Arc::new(client);

                let (sender, rx) = create_checkpoint_queue(100);
                let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

                let config = CheckpointBatcherConfig {
                    max_batch_time_ms: 10,
                    max_batch_operations: usize::MAX,
                    max_batch_size_bytes: max_batch_size,
                };

                let mut batcher = CheckpointBatcher::new(
                    config,
                    rx,
                    client,
                    "arn:test".to_string(),
                    checkpoint_token.clone(),
                );

                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), result_size);
                    sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
                }

                drop(sender);
                batcher.run().await;

                let estimated_request_size = 100 + result_size;
                let total_size = num_requests * estimated_request_size;
                let expected_max_calls = (total_size + max_batch_size - 1) / max_batch_size;
                let actual_calls = call_count.load(AtomicOrdering::SeqCst);

                if actual_calls > expected_max_calls.max(1) * 2 {
                    return Err(TestCaseError::fail(format!(
                        "Expected at most ~{} API calls for {} requests of size {}, got {}",
                        expected_max_calls, num_requests, estimated_request_size, actual_calls
                    )));
                }

                if actual_calls < 1 {
                    return Err(TestCaseError::fail(format!(
                        "Expected at least 1 API call, got {}",
                        actual_calls
                    )));
                }

                Ok(())
            });
            result?;
        }

        #[test]
        fn prop_all_requests_are_processed(
            num_requests in 1usize..20,
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let (client, _call_count) = CountingMockClient::new();
                let client = Arc::new(client);

                let (sender, rx) = create_checkpoint_queue(100);
                let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));

                let config = CheckpointBatcherConfig {
                    max_batch_time_ms: 10,
                    max_batch_operations: 5,
                    ..Default::default()
                };

                let mut batcher = CheckpointBatcher::new(
                    config,
                    rx,
                    client,
                    "arn:test".to_string(),
                    checkpoint_token.clone(),
                );

                let mut receivers = Vec::new();

                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), 0);
                    let (request, rx) = CheckpointRequest::sync(update);
                    sender.tx.send(request).await.unwrap();
                    receivers.push(rx);
                }

                drop(sender);
                batcher.run().await;

                let mut success_count = 0;
                for rx in receivers {
                    if let Ok(result) = rx.await {
                        if result.is_ok() {
                            success_count += 1;
                        }
                    }
                }

                if success_count != num_requests {
                    return Err(TestCaseError::fail(format!(
                        "Expected all {} requests to succeed, got {}",
                        num_requests, success_count
                    )));
                }

                Ok(())
            });
            result?;
        }
    }

    // ============================================================================
    // Property Tests for Batch Ordering (Properties 13, 14, 15)
    // Feature: rust-sdk-test-suite
    // ============================================================================

    /// Strategy for generating valid operation IDs
    fn operation_id_strategy() -> impl Strategy<Value = String> {
        "[a-z]{1,8}-[0-9]{1,4}".prop_map(|s| s)
    }

    /// Strategy for generating operation types
    fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
        prop_oneof![
            Just(OperationType::Execution),
            Just(OperationType::Step),
            Just(OperationType::Wait),
            Just(OperationType::Callback),
            Just(OperationType::Invoke),
            Just(OperationType::Context),
        ]
    }

    /// Strategy for generating operation actions
    fn operation_action_strategy() -> impl Strategy<Value = OperationAction> {
        prop_oneof![
            Just(OperationAction::Start),
            Just(OperationAction::Succeed),
            Just(OperationAction::Fail),
        ]
    }

    /// Creates a checkpoint request with the given parameters
    fn create_checkpoint_request(
        id: &str,
        op_type: OperationType,
        action: OperationAction,
        parent_id: Option<String>,
    ) -> CheckpointRequest {
        let mut update = match action {
            OperationAction::Start => OperationUpdate::start(id, op_type),
            OperationAction::Succeed => {
                OperationUpdate::succeed(id, op_type, Some("result".to_string()))
            }
            OperationAction::Fail => OperationUpdate::fail(
                id,
                op_type,
                crate::error::ErrorObject::new("Error", "message"),
            ),
            _ => OperationUpdate::start(id, op_type),
        };
        update.parent_id = parent_id;
        CheckpointRequest::async_request(update)
    }

    /// Strategy for generating a batch with parent-child relationships
    /// Generates a CONTEXT START and some child operations that reference it
    fn parent_child_batch_strategy() -> impl Strategy<Value = Vec<CheckpointRequest>> {
        (
            operation_id_strategy(),                                // parent context id
            prop::collection::vec(operation_id_strategy(), 1..5),   // child ids
            prop::collection::vec(operation_type_strategy(), 1..5), // child types (will be filtered)
        )
            .prop_map(|(parent_id, child_ids, child_types)| {
                let mut batch = Vec::new();

                // Add children first (in random order, they should be sorted after parent)
                for (child_id, child_type) in child_ids.iter().zip(child_types.iter()) {
                    // Skip if child_type is Execution (can't have parent)
                    if *child_type != OperationType::Execution {
                        batch.push(create_checkpoint_request(
                            child_id,
                            *child_type,
                            OperationAction::Start,
                            Some(parent_id.clone()),
                        ));
                    }
                }

                // Add parent CONTEXT START (should be sorted to come first)
                batch.push(create_checkpoint_request(
                    &parent_id,
                    OperationType::Context,
                    OperationAction::Start,
                    None,
                ));

                batch
            })
    }

    /// Strategy for generating a batch with EXECUTION completion
    /// Generates some operations and an EXECUTION SUCCEED/FAIL that should be last
    fn execution_completion_batch_strategy() -> impl Strategy<Value = Vec<CheckpointRequest>> {
        (
            operation_id_strategy(), // execution id
            prop::collection::vec((operation_id_strategy(), operation_type_strategy()), 1..5), // other operations
            prop::bool::ANY, // succeed or fail
        )
            .prop_map(|(exec_id, other_ops, succeed)| {
                let mut batch = Vec::new();

                // Add EXECUTION completion first (should be sorted to come last)
                let exec_action = if succeed {
                    OperationAction::Succeed
                } else {
                    OperationAction::Fail
                };
                batch.push(create_checkpoint_request(
                    &exec_id,
                    OperationType::Execution,
                    exec_action,
                    None,
                ));

                // Add other operations
                for (op_id, op_type) in other_ops {
                    // Skip Execution type to avoid conflicts
                    if op_type != OperationType::Execution {
                        batch.push(create_checkpoint_request(
                            &op_id,
                            op_type,
                            OperationAction::Start,
                            None,
                        ));
                    }
                }

                batch
            })
    }

    /// Strategy for generating a batch with potential duplicate operation IDs
    /// (START and completion for same operation)
    fn same_operation_batch_strategy() -> impl Strategy<Value = Vec<CheckpointRequest>> {
        (
            operation_id_strategy(),
            prop_oneof![Just(OperationType::Step), Just(OperationType::Context),],
            prop::bool::ANY, // succeed or fail
        )
            .prop_map(|(op_id, op_type, succeed)| {
                let mut batch = Vec::new();

                // Add completion first (should be sorted after START)
                let completion_action = if succeed {
                    OperationAction::Succeed
                } else {
                    OperationAction::Fail
                };
                batch.push(create_checkpoint_request(
                    &op_id,
                    op_type,
                    completion_action,
                    None,
                ));

                // Add START (should be sorted to come first)
                batch.push(create_checkpoint_request(
                    &op_id,
                    op_type,
                    OperationAction::Start,
                    None,
                ));

                batch
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property 13: Parent-before-child ordering
        /// For any batch of operation updates, child operations SHALL appear after their parent CONTEXT start
        /// Feature: rust-sdk-test-suite, Property 13: Parent-before-child ordering
        /// Validates: Requirements 8.1
        #[test]
        fn prop_parent_context_start_before_children(batch in parent_child_batch_strategy()) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Find the parent CONTEXT START position
            let parent_pos = sorted.iter().position(|r| {
                r.operation.operation_type == OperationType::Context
                    && r.operation.action == OperationAction::Start
                    && r.operation.parent_id.is_none()
            });

            // If there's a parent CONTEXT START, all children must come after it
            if let Some(parent_idx) = parent_pos {
                let parent_id = &sorted[parent_idx].operation.operation_id;

                for (idx, req) in sorted.iter().enumerate() {
                    if let Some(ref req_parent_id) = req.operation.parent_id {
                        if req_parent_id == parent_id {
                            prop_assert!(
                                idx > parent_idx,
                                "Child operation {} at index {} should come after parent {} at index {}",
                                req.operation.operation_id,
                                idx,
                                parent_id,
                                parent_idx
                            );
                        }
                    }
                }
            }
        }

        /// Property 14: Execution-last ordering
        /// For any batch containing EXECUTION completion, the EXECUTION update SHALL be last
        /// Feature: rust-sdk-test-suite, Property 14: Execution-last ordering
        /// Validates: Requirements 8.2
        #[test]
        fn prop_execution_completion_is_last(batch in execution_completion_batch_strategy()) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Find EXECUTION completion (SUCCEED or FAIL)
            let exec_completion_pos = sorted.iter().position(|r| {
                r.operation.operation_type == OperationType::Execution
                    && matches!(r.operation.action, OperationAction::Succeed | OperationAction::Fail)
            });

            // If there's an EXECUTION completion, it must be last
            if let Some(exec_idx) = exec_completion_pos {
                prop_assert_eq!(
                    exec_idx,
                    sorted.len() - 1,
                    "EXECUTION completion should be at index {} (last), but found at index {}",
                    sorted.len() - 1,
                    exec_idx
                );
            }
        }

        /// Property 15: Operation ID uniqueness (with exception for START+completion)
        /// For any batch, each operation_id SHALL appear at most once, except for STEP/CONTEXT
        /// operations which may have both START and completion in the same batch
        /// Feature: rust-sdk-test-suite, Property 15: Operation ID uniqueness
        /// Validates: Requirements 8.3
        #[test]
        fn prop_operation_id_uniqueness_with_start_completion_exception(
            batch in same_operation_batch_strategy()
        ) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Track operation_id occurrences with their actions
            let mut seen: std::collections::HashMap<String, Vec<OperationAction>> = std::collections::HashMap::new();

            for req in &sorted {
                seen.entry(req.operation.operation_id.clone())
                    .or_default()
                    .push(req.operation.action);
            }

            // Verify uniqueness rules
            for (op_id, actions) in &seen {
                if actions.len() > 1 {
                    // Multiple occurrences are only allowed for START + completion
                    let has_start = actions.contains(&OperationAction::Start);
                    let has_completion = actions.iter().any(|a| {
                        matches!(a, OperationAction::Succeed | OperationAction::Fail | OperationAction::Retry)
                    });

                    prop_assert!(
                        has_start && has_completion && actions.len() == 2,
                        "Operation {} appears {} times with actions {:?}. \
                         Multiple occurrences only allowed for START + completion pair.",
                        op_id,
                        actions.len(),
                        actions
                    );
                }
            }
        }

        /// Property 15 (continued): For same operation, START must come before completion
        /// Feature: rust-sdk-test-suite, Property 15: START before completion ordering
        /// Validates: Requirements 8.3
        #[test]
        fn prop_start_before_completion_for_same_operation(
            batch in same_operation_batch_strategy()
        ) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Find START and completion positions for each operation
            let mut start_positions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            let mut completion_positions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

            for (idx, req) in sorted.iter().enumerate() {
                let op_id = &req.operation.operation_id;
                match req.operation.action {
                    OperationAction::Start => {
                        start_positions.insert(op_id.clone(), idx);
                    }
                    OperationAction::Succeed | OperationAction::Fail | OperationAction::Retry => {
                        completion_positions.insert(op_id.clone(), idx);
                    }
                    _ => {}
                }
            }

            // Verify START comes before completion for each operation
            for (op_id, start_idx) in &start_positions {
                if let Some(completion_idx) = completion_positions.get(op_id) {
                    prop_assert!(
                        start_idx < completion_idx,
                        "For operation {}, START at index {} should come before completion at index {}",
                        op_id,
                        start_idx,
                        completion_idx
                    );
                }
            }
        }
    }

    // ============================================================================
    // Property Test for Batch Round-Trip Preservation
    // Feature: rust-sdk-test-suite
    // ============================================================================

    /// Strategy for generating a random batch of operations
    fn random_batch_strategy() -> impl Strategy<Value = Vec<CheckpointRequest>> {
        prop::collection::vec(
            (
                operation_id_strategy(),
                operation_type_strategy(),
                operation_action_strategy(),
            ),
            1..10,
        )
        .prop_map(|ops| {
            ops.into_iter()
                .filter(|(_, op_type, _)| *op_type != OperationType::Execution) // Avoid execution complications
                .map(|(id, op_type, action)| create_checkpoint_request(&id, op_type, action, None))
                .collect()
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property: Batch sorting preserves all operations
        /// For any valid operation update sequence, sorting SHALL preserve all operations
        /// Feature: rust-sdk-test-suite, Property: Batch preservation
        /// Validates: Requirements 8.4
        #[test]
        fn prop_batch_sorting_preserves_all_operations(batch in random_batch_strategy()) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            // Collect original operation IDs and actions
            let original_ops: HashSet<(String, String)> = batch.iter()
                .map(|r| (r.operation.operation_id.clone(), format!("{:?}", r.operation.action)))
                .collect();

            let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Collect sorted operation IDs and actions
            let sorted_ops: HashSet<(String, String)> = sorted.iter()
                .map(|r| (r.operation.operation_id.clone(), format!("{:?}", r.operation.action)))
                .collect();

            // Verify all operations are preserved
            prop_assert_eq!(
                original_ops.len(),
                sorted_ops.len(),
                "Sorting changed the number of operations: original {}, sorted {}",
                original_ops.len(),
                sorted_ops.len()
            );

            prop_assert_eq!(
                original_ops,
                sorted_ops,
                "Sorting changed the set of operations"
            );
        }

        /// Property: Sorting is idempotent
        /// Sorting an already sorted batch should produce the same result
        /// Feature: rust-sdk-test-suite, Property: Sorting idempotence
        #[test]
        fn prop_sorting_is_idempotent(batch in random_batch_strategy()) {
            // Skip empty batches
            if batch.is_empty() {
                return Ok(());
            }

            let sorted_once = CheckpointBatcher::sort_checkpoint_batch(batch);

            // Clone the sorted batch for second sort
            let sorted_once_clone: Vec<CheckpointRequest> = sorted_once.iter()
                .map(|r| CheckpointRequest::async_request(r.operation.clone()))
                .collect();

            let sorted_twice = CheckpointBatcher::sort_checkpoint_batch(sorted_once_clone);

            // Verify the order is the same
            prop_assert_eq!(
                sorted_once.len(),
                sorted_twice.len(),
                "Double sorting changed the number of operations"
            );

            for (idx, (first, second)) in sorted_once.iter().zip(sorted_twice.iter()).enumerate() {
                prop_assert_eq!(
                    &first.operation.operation_id,
                    &second.operation.operation_id,
                    "Operation ID mismatch at index {}: {} vs {}",
                    idx,
                    &first.operation.operation_id,
                    &second.operation.operation_id
                );
                prop_assert_eq!(
                    first.operation.action,
                    second.operation.action,
                    "Action mismatch at index {} for operation {}: {:?} vs {:?}",
                    idx,
                    &first.operation.operation_id,
                    first.operation.action,
                    second.operation.action
                );
            }
        }
    }
}
