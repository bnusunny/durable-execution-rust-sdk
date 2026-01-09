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
            + op.error.as_ref().map(|e| e.error_message.len() + e.error_type.len()).unwrap_or(0)
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
        let batch_deadline = Instant::now() + StdDuration::from_millis(self.config.max_batch_time_ms);

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
                    if batch_size + request_size > self.config.max_batch_size_bytes && !batch.is_empty() {
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
            .map(|req| (req.operation.operation_id.clone(), req.operation.parent_id.clone()))
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
        let mut indexed_batch: Vec<(usize, CheckpointRequest)> = batch
            .into_iter()
            .enumerate()
            .collect();

        indexed_batch.sort_by(|(idx_a, a), (idx_b, b)| {
            // Priority 1: EXECUTION completion must be last
            let a_is_exec_completion = a.operation.operation_type == OperationType::Execution
                && matches!(a.operation.action, OperationAction::Succeed | OperationAction::Fail);
            let b_is_exec_completion = b.operation.operation_type == OperationType::Execution
                && matches!(b.operation.action, OperationAction::Succeed | OperationAction::Fail);

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
        let result = self
            .checkpoint_with_retry(&token, operations)
            .await;

        // Handle the result
        match result {
            Ok(response) => {
                // Mark that we've consumed the initial token
                self.initial_token_consumed.store(true, Ordering::SeqCst);
                
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
    pub async fn checkpoint(&self, operation: OperationUpdate, is_sync: bool) -> Result<(), DurableError> {
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
pub fn create_checkpoint_queue(buffer_size: usize) -> (CheckpointSender, mpsc::Receiver<CheckpointRequest>) {
    let (tx, rx) = mpsc::channel(buffer_size);
    (CheckpointSender::new(tx), rx)
}
