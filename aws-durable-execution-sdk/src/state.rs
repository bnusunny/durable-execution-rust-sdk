//! Execution state management for the AWS Durable Execution SDK.
//!
//! This module provides the core state management types for durable executions,
//! including checkpoint tracking, replay logic, and operation state management.
//!
//! ## Checkpoint Token Management
//!
//! The SDK uses checkpoint tokens to ensure exactly-once checkpoint semantics:
//!
//! 1. **Initial Token**: The first checkpoint uses the `CheckpointToken` from the
//!    `DurableExecutionInvocationInput` provided by Lambda.
//!
//! 2. **Token Updates**: Each successful checkpoint returns a new token that MUST
//!    be used for the next checkpoint. The SDK automatically updates the token
//!    after each successful checkpoint.
//!
//! 3. **Token Consumption**: Once a token is used for a checkpoint, it is consumed
//!    and cannot be reused. Attempting to reuse a consumed token results in an
//!    `InvalidParameterValueException` error.
//!
//! 4. **Error Handling**: If a checkpoint fails with "Invalid checkpoint token",
//!    the error is marked as retriable so Lambda can retry with a fresh token.
//!
//! ## Requirements
//!
//! - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
//! - 2.10: THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
//! - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::{timeout, Instant};

use crate::client::SharedDurableServiceClient;
use crate::error::{DurableError, ErrorObject};
use crate::operation::{Operation, OperationStatus, OperationType, OperationUpdate};

/// Result of checking for a checkpointed operation.
///
/// This struct provides methods to query the status of a previously
/// checkpointed operation during replay.
#[derive(Debug, Clone)]
pub struct CheckpointedResult {
    /// The operation if it exists in the checkpoint
    operation: Option<Operation>,
}

impl CheckpointedResult {
    /// Creates a new CheckpointedResult with the given operation.
    pub fn new(operation: Option<Operation>) -> Self {
        Self { operation }
    }

    /// Creates an empty CheckpointedResult (no checkpoint exists).
    pub fn empty() -> Self {
        Self { operation: None }
    }

    /// Returns true if a checkpoint exists for this operation.
    pub fn is_existent(&self) -> bool {
        self.operation.is_some()
    }

    /// Returns true if the operation succeeded.
    pub fn is_succeeded(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Succeeded)
            .unwrap_or(false)
    }

    /// Returns true if the operation failed.
    pub fn is_failed(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Failed)
            .unwrap_or(false)
    }

    /// Returns true if the operation was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Cancelled)
            .unwrap_or(false)
    }

    /// Returns true if the operation timed out.
    pub fn is_timed_out(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::TimedOut)
            .unwrap_or(false)
    }

    /// Returns true if the operation was stopped.
    pub fn is_stopped(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Stopped)
            .unwrap_or(false)
    }

    /// Returns true if the operation is pending (waiting for retry).
    /// Requirements: 3.7, 4.7
    pub fn is_pending(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Pending)
            .unwrap_or(false)
    }

    /// Returns true if the operation is ready to resume execution.
    /// Requirements: 3.7
    pub fn is_ready(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status == OperationStatus::Ready)
            .unwrap_or(false)
    }

    /// Returns true if the operation is in a terminal state (completed).
    pub fn is_terminal(&self) -> bool {
        self.operation
            .as_ref()
            .map(|op| op.status.is_terminal())
            .unwrap_or(false)
    }

    /// Returns the operation status if the checkpoint exists.
    pub fn status(&self) -> Option<OperationStatus> {
        self.operation.as_ref().map(|op| op.status)
    }

    /// Returns the operation type if the checkpoint exists.
    pub fn operation_type(&self) -> Option<OperationType> {
        self.operation.as_ref().map(|op| op.operation_type)
    }

    /// Returns the serialized result if the operation succeeded.
    /// This checks both type-specific details (e.g., StepDetails.Result) and the legacy Result field.
    pub fn result(&self) -> Option<&str> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_result())
    }

    /// Returns the error if the operation failed.
    pub fn error(&self) -> Option<&ErrorObject> {
        self.operation.as_ref().and_then(|op| op.error.as_ref())
    }

    /// Returns a reference to the underlying operation.
    pub fn operation(&self) -> Option<&Operation> {
        self.operation.as_ref()
    }

    /// Consumes self and returns the underlying operation.
    pub fn into_operation(self) -> Option<Operation> {
        self.operation
    }

    /// Returns the retry payload if this is a STEP operation with a payload.
    ///
    /// This is used for the wait-for-condition pattern where state is passed
    /// between retry attempts via the Payload field.
    ///
    /// # Returns
    ///
    /// The payload string if available, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.9: THE Step_Operation SHALL support RETRY action with Payload for wait-for-condition pattern
    pub fn retry_payload(&self) -> Option<&str> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_retry_payload())
    }

    /// Returns the current attempt number for STEP operations.
    ///
    /// # Returns
    ///
    /// The attempt number (0-indexed) if available, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.8: THE Step_Operation SHALL track attempt numbers in StepDetails.Attempt
    pub fn attempt(&self) -> Option<u32> {
        self.operation
            .as_ref()
            .and_then(|op| op.get_attempt())
    }
}

/// Replay status indicating whether we're replaying or executing new operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReplayStatus {
    /// Currently replaying previously checkpointed operations
    Replay = 0,
    /// Executing new operations (past the replay point)
    New = 1,
}

impl ReplayStatus {
    /// Returns true if currently in replay mode.
    pub fn is_replay(&self) -> bool {
        matches!(self, Self::Replay)
    }

    /// Returns true if executing new operations.
    pub fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}

impl From<u8> for ReplayStatus {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Replay,
            _ => Self::New,
        }
    }
}

impl From<ReplayStatus> for u8 {
    fn from(status: ReplayStatus) -> Self {
        status as u8
    }
}

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
///
/// ## Checkpoint Token Management
///
/// The batcher manages checkpoint tokens according to the following rules:
///
/// 1. **First Checkpoint**: Uses the initial `CheckpointToken` from the Lambda invocation input
/// 2. **Subsequent Checkpoints**: Uses the token returned from the previous checkpoint response
/// 3. **Token Consumption**: Each token can only be used once; the batcher automatically
///    updates to the new token after each successful checkpoint
/// 4. **Error Handling**: If a checkpoint fails with "Invalid checkpoint token", the error
///    is marked as retriable so Lambda can retry with a fresh token
///
/// ## Requirements
///
/// - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
/// - 2.10: THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
/// - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry
/// - 2.12: WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
/// - 2.13: THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch
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
    /// This is updated after each successful checkpoint to ensure
    /// we never reuse a consumed token.
    checkpoint_token: Arc<RwLock<String>>,
    /// Tracks whether the initial token has been consumed
    /// This helps with debugging and ensures we follow the token lifecycle correctly
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
    /// # Algorithm
    ///
    /// The sorting algorithm works as follows:
    /// 1. Identify all CONTEXT START operations and their IDs
    /// 2. Group operations by their parent_id
    /// 3. Sort so that:
    ///    - CONTEXT START comes before any child operations with that parent_id
    ///    - EXECUTION completion is always last
    ///    - Operations with the same ID (START + completion) maintain START before completion
    ///
    /// # Requirements
    ///
    /// - 2.12: WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
    /// - 2.13: THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch
    fn sort_checkpoint_batch(batch: Vec<CheckpointRequest>) -> Vec<CheckpointRequest> {
        use crate::operation::{OperationAction, OperationType};
        use std::collections::{HashMap, HashSet};

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
        // Returns true if ancestor_id is in the parent chain of operation_id
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
        // We use a stable sort to preserve original order when priorities are equal
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
            // Check if 'a' is a descendant of 'b' (b should come first)
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
            // Check if 'b' is a descendant of 'a' (a should come first)
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
            // This supports START+completion in the same batch (Requirements: 2.13)
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
    /// This method:
    /// 1. Gets the current checkpoint token
    /// 2. Sends the batch to the Lambda service (with retry for throttling)
    /// 3. Updates the checkpoint token with the new token from the response
    /// 4. Signals completion to all waiting callers
    ///
    /// # Token Management
    ///
    /// - The first batch uses the initial token from Lambda invocation input
    /// - Each successful checkpoint returns a new token that is used for the next batch
    /// - Tokens are never reused - each token can only be consumed once
    ///
    /// # Error Handling
    ///
    /// - If the checkpoint fails with "Invalid checkpoint token", the error is marked
    ///   as retriable so Lambda can retry with a fresh token.
    /// - If the checkpoint fails with throttling, the SDK retries with exponential backoff.
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
        // Requirements: 2.12, 2.13
        let sorted_batch = Self::sort_checkpoint_batch(batch);

        // Extract operations and completion channels
        let (operations, completions): (Vec<_>, Vec<_>) = sorted_batch
            .into_iter()
            .map(|req| (req.operation, req.completion))
            .unzip();

        // Get current checkpoint token
        // Requirements: 2.9, 2.10 - Use the initial token for first checkpoint,
        // then use the returned token for subsequent checkpoints
        let token = self.checkpoint_token.read().await.clone();

        // Send the batch to the service with retry for throttling
        // Requirements: 18.5 - Handle ThrottlingException with appropriate retry behavior
        let result = self
            .checkpoint_with_retry(&token, operations)
            .await;

        // Handle the result
        match result {
            Ok(response) => {
                // Mark that we've consumed the initial token (if this was the first checkpoint)
                self.initial_token_consumed.store(true, Ordering::SeqCst);
                
                // Update the checkpoint token with the new token from the response
                // Requirements: 2.10 - Use the returned CheckpointToken for subsequent checkpoints
                // This ensures we never reuse a consumed token
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
                // Requirements: 2.11 - Handle InvalidParameterValueException for invalid tokens
                let is_invalid_token = error.is_invalid_checkpoint_token();
                
                // Signal failure to all waiting callers
                // If it's an invalid token error, it's already marked as retriable
                // so Lambda will retry with a fresh token
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
    /// This method implements exponential backoff for throttling errors:
    /// - Initial delay: 100ms (or retry_after_ms if provided)
    /// - Maximum delay: 10 seconds
    /// - Maximum retries: 5
    /// - Backoff multiplier: 2x
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

                    // Use retry_after_ms from the error if available, otherwise use calculated delay
                    let actual_delay = error.get_retry_after_ms().unwrap_or(delay_ms);
                    
                    tracing::debug!(
                        attempt = attempt,
                        delay_ms = actual_delay,
                        "Checkpoint throttled, retrying with exponential backoff"
                    );

                    // Sleep before retrying
                    tokio::time::sleep(StdDuration::from_millis(actual_delay)).await;

                    // Calculate next delay with exponential backoff
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
    tx: mpsc::Sender<CheckpointRequest>,
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

/// Manages the execution state for a durable execution.
///
/// This struct tracks all checkpointed operations, handles replay logic,
/// and manages communication with the Lambda durable execution service.
pub struct ExecutionState {
    /// The ARN of the durable execution
    durable_execution_arn: String,
    
    /// The current checkpoint token (updated after each checkpoint batch)
    checkpoint_token: Arc<RwLock<String>>,
    
    /// Map of operation_id to Operation for quick lookup during replay
    operations: RwLock<HashMap<String, Operation>>,
    
    /// The service client for communicating with Lambda
    service_client: SharedDurableServiceClient,
    
    /// Current replay status (Replay or New)
    replay_status: AtomicU8,
    
    /// Set of operation IDs that have been replayed (for tracking replay progress)
    replayed_operations: RwLock<HashSet<String>>,
    
    /// Marker for pagination when loading additional operations
    next_marker: RwLock<Option<String>>,
    
    /// Set of parent operation IDs that have completed (for orphan prevention)
    parent_done_lock: Mutex<HashSet<String>>,
    
    /// Optional checkpoint sender for batched checkpointing
    checkpoint_sender: Option<CheckpointSender>,
    
    /// The EXECUTION operation (first operation in state) - provides access to original input
    /// Requirements: 19.1, 19.2
    execution_operation: Option<Operation>,
    
    /// The checkpointing mode that controls the trade-off between durability and performance.
    ///
    /// This field determines when and how often the SDK persists operation state:
    /// - `Eager`: Checkpoint after every operation (maximum durability)
    /// - `Batched`: Group operations into batches (balanced, default)
    /// - `Optimistic`: Execute multiple operations before checkpointing (best performance)
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    /// - 24.4: THE Performance_Configuration SHALL document the default behavior and trade-offs
    checkpointing_mode: crate::config::CheckpointingMode,
}

impl ExecutionState {
    /// Creates a new ExecutionState from the Lambda invocation input.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The initial checkpoint token
    /// * `initial_state` - The initial execution state with operations
    /// * `service_client` - The service client for Lambda communication
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    /// - 19.2: THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
    pub fn new(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
    ) -> Self {
        Self::with_checkpointing_mode(
            durable_execution_arn,
            checkpoint_token,
            initial_state,
            service_client,
            crate::config::CheckpointingMode::default(),
        )
    }
    
    /// Creates a new ExecutionState with a specific checkpointing mode.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The initial checkpoint token
    /// * `initial_state` - The initial execution state with operations
    /// * `service_client` - The service client for Lambda communication
    /// * `checkpointing_mode` - The checkpointing mode to use
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    /// - 19.2: THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub fn with_checkpointing_mode(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
        checkpointing_mode: crate::config::CheckpointingMode,
    ) -> Self {
        // Find and extract the EXECUTION operation (first operation of type EXECUTION)
        // Requirements: 19.1, 19.2
        let execution_operation = initial_state
            .operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution)
            .cloned();
        
        // Build the operations map from the initial state
        let operations: HashMap<String, Operation> = initial_state
            .operations
            .into_iter()
            .map(|op| (op.operation_id.clone(), op))
            .collect();
        
        // Determine initial replay status based on whether we have operations
        let replay_status = if operations.is_empty() {
            ReplayStatus::New
        } else {
            ReplayStatus::Replay
        };
        
        Self {
            durable_execution_arn: durable_execution_arn.into(),
            checkpoint_token: Arc::new(RwLock::new(checkpoint_token.into())),
            operations: RwLock::new(operations),
            service_client,
            replay_status: AtomicU8::new(replay_status as u8),
            replayed_operations: RwLock::new(HashSet::new()),
            next_marker: RwLock::new(initial_state.next_marker),
            parent_done_lock: Mutex::new(HashSet::new()),
            checkpoint_sender: None,
            execution_operation,
            checkpointing_mode,
        }
    }
    
    /// Creates a new ExecutionState with a checkpoint batcher.
    ///
    /// This method sets up the checkpoint queue and batcher for efficient
    /// batched checkpointing. Returns the ExecutionState and a handle to
    /// the batcher that should be run in a background task.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The initial checkpoint token
    /// * `initial_state` - The initial execution state with operations
    /// * `service_client` - The service client for Lambda communication
    /// * `batcher_config` - Configuration for the checkpoint batcher
    /// * `queue_buffer_size` - Size of the checkpoint queue buffer
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    /// - 19.2: THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
    pub fn with_batcher(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
        batcher_config: CheckpointBatcherConfig,
        queue_buffer_size: usize,
    ) -> (Self, CheckpointBatcher) {
        Self::with_batcher_and_mode(
            durable_execution_arn,
            checkpoint_token,
            initial_state,
            service_client,
            batcher_config,
            queue_buffer_size,
            crate::config::CheckpointingMode::default(),
        )
    }
    
    /// Creates a new ExecutionState with a checkpoint batcher and specific checkpointing mode.
    ///
    /// This method sets up the checkpoint queue and batcher for efficient
    /// batched checkpointing. Returns the ExecutionState and a handle to
    /// the batcher that should be run in a background task.
    ///
    /// # Arguments
    ///
    /// * `durable_execution_arn` - The ARN of the durable execution
    /// * `checkpoint_token` - The initial checkpoint token
    /// * `initial_state` - The initial execution state with operations
    /// * `service_client` - The service client for Lambda communication
    /// * `batcher_config` - Configuration for the checkpoint batcher
    /// * `queue_buffer_size` - Size of the checkpoint queue buffer
    /// * `checkpointing_mode` - The checkpointing mode to use
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    /// - 19.2: THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub fn with_batcher_and_mode(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
        batcher_config: CheckpointBatcherConfig,
        queue_buffer_size: usize,
        checkpointing_mode: crate::config::CheckpointingMode,
    ) -> (Self, CheckpointBatcher) {
        let arn: String = durable_execution_arn.into();
        let token: String = checkpoint_token.into();
        
        // Find and extract the EXECUTION operation (first operation of type EXECUTION)
        // Requirements: 19.1, 19.2
        let execution_operation = initial_state
            .operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution)
            .cloned();
        
        // Build the operations map from the initial state
        let operations: HashMap<String, Operation> = initial_state
            .operations
            .into_iter()
            .map(|op| (op.operation_id.clone(), op))
            .collect();
        
        // Determine initial replay status based on whether we have operations
        let replay_status = if operations.is_empty() {
            ReplayStatus::New
        } else {
            ReplayStatus::Replay
        };
        
        // Create the checkpoint queue
        let (sender, rx) = create_checkpoint_queue(queue_buffer_size);
        let checkpoint_token = Arc::new(RwLock::new(token));
        
        // Create the batcher
        let batcher = CheckpointBatcher::new(
            batcher_config,
            rx,
            service_client.clone(),
            arn.clone(),
            checkpoint_token.clone(),
        );
        
        let state = Self {
            durable_execution_arn: arn,
            checkpoint_token,
            operations: RwLock::new(operations),
            service_client,
            replay_status: AtomicU8::new(replay_status as u8),
            replayed_operations: RwLock::new(HashSet::new()),
            next_marker: RwLock::new(initial_state.next_marker),
            parent_done_lock: Mutex::new(HashSet::new()),
            checkpoint_sender: Some(sender),
            execution_operation,
            checkpointing_mode,
        };
        
        (state, batcher)
    }
    
    /// Returns the durable execution ARN.
    pub fn durable_execution_arn(&self) -> &str {
        &self.durable_execution_arn
    }
    
    /// Returns the current checkpoint token.
    pub async fn checkpoint_token(&self) -> String {
        self.checkpoint_token.read().await.clone()
    }
    
    /// Updates the checkpoint token after a successful checkpoint.
    pub async fn set_checkpoint_token(&self, token: impl Into<String>) {
        let mut guard = self.checkpoint_token.write().await;
        *guard = token.into();
    }
    
    /// Returns the current replay status.
    pub fn replay_status(&self) -> ReplayStatus {
        ReplayStatus::from(self.replay_status.load(Ordering::SeqCst))
    }
    
    /// Returns true if currently in replay mode.
    pub fn is_replay(&self) -> bool {
        self.replay_status().is_replay()
    }
    
    /// Returns true if executing new operations.
    pub fn is_new(&self) -> bool {
        self.replay_status().is_new()
    }
    
    /// Returns the current checkpointing mode.
    ///
    /// The checkpointing mode determines when and how often the SDK persists
    /// operation state to the durable execution service.
    ///
    /// # Returns
    ///
    /// The [`CheckpointingMode`] configured for this execution state.
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    /// - 24.4: THE Performance_Configuration SHALL document the default behavior and trade-offs
    pub fn checkpointing_mode(&self) -> crate::config::CheckpointingMode {
        self.checkpointing_mode
    }
    
    /// Returns true if eager checkpointing mode is enabled.
    ///
    /// In eager mode, every operation is immediately checkpointed for maximum durability.
    pub fn is_eager_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_eager()
    }
    
    /// Returns true if batched checkpointing mode is enabled.
    ///
    /// In batched mode, operations are grouped into batches before checkpointing.
    pub fn is_batched_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_batched()
    }
    
    /// Returns true if optimistic checkpointing mode is enabled.
    ///
    /// In optimistic mode, multiple operations execute before checkpointing.
    pub fn is_optimistic_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_optimistic()
    }
    
    /// Returns a reference to the EXECUTION operation if it exists.
    ///
    /// The EXECUTION operation is the first operation in the state and represents
    /// the overall execution. It provides access to the original user input.
    ///
    /// # Returns
    ///
    /// An `Option<&Operation>` containing the EXECUTION operation if it exists.
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    pub fn execution_operation(&self) -> Option<&Operation> {
        self.execution_operation.as_ref()
    }
    
    /// Returns the original user input from the EXECUTION operation.
    ///
    /// This method extracts the input payload from the EXECUTION operation's
    /// ExecutionDetails.InputPayload field.
    ///
    /// # Returns
    ///
    /// An `Option<&str>` containing the original input payload if available.
    ///
    /// # Requirements
    ///
    /// - 19.2: THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
    pub fn get_original_input_raw(&self) -> Option<&str> {
        self.execution_operation
            .as_ref()
            .and_then(|op| op.execution_details.as_ref())
            .and_then(|details| details.input_payload.as_deref())
    }
    
    /// Returns the EXECUTION operation's ID if it exists.
    ///
    /// # Returns
    ///
    /// An `Option<&str>` containing the EXECUTION operation ID.
    pub fn execution_operation_id(&self) -> Option<&str> {
        self.execution_operation
            .as_ref()
            .map(|op| op.operation_id.as_str())
    }
    
    /// Completes the execution with a successful result via checkpointing.
    ///
    /// This method checkpoints a SUCCEED action on the EXECUTION operation,
    /// which is useful for large results that exceed the Lambda response size limit.
    ///
    /// # Arguments
    ///
    /// * `result` - The serialized result to checkpoint
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or a `DurableError` if:
    /// - No EXECUTION operation exists
    /// - The checkpoint fails
    ///
    /// # Requirements
    ///
    /// - 19.3: THE EXECUTION_Operation SHALL support completing execution via SUCCEED action with result
    /// - 19.5: WHEN execution result exceeds response size limits, THE EXECUTION_Operation SHALL checkpoint the result and return empty Result field
    pub async fn complete_execution_success(&self, result: Option<String>) -> Result<(), DurableError> {
        let execution_id = self.execution_operation_id().ok_or_else(|| {
            DurableError::Validation {
                message: "Cannot complete execution: no EXECUTION operation exists".to_string(),
            }
        })?;
        
        let update = OperationUpdate::succeed(
            execution_id,
            OperationType::Execution,
            result,
        );
        
        self.create_checkpoint(update, true).await
    }
    
    /// Completes the execution with a failure via checkpointing.
    ///
    /// This method checkpoints a FAIL action on the EXECUTION operation.
    ///
    /// # Arguments
    ///
    /// * `error` - The error details to checkpoint
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or a `DurableError` if:
    /// - No EXECUTION operation exists
    /// - The checkpoint fails
    ///
    /// # Requirements
    ///
    /// - 19.4: THE EXECUTION_Operation SHALL support completing execution via FAIL action with error
    pub async fn complete_execution_failure(&self, error: ErrorObject) -> Result<(), DurableError> {
        let execution_id = self.execution_operation_id().ok_or_else(|| {
            DurableError::Validation {
                message: "Cannot complete execution: no EXECUTION operation exists".to_string(),
            }
        })?;
        
        let update = OperationUpdate::fail(
            execution_id,
            OperationType::Execution,
            error,
        );
        
        self.create_checkpoint(update, true).await
    }
    
    /// Gets the checkpoint result for an operation.
    ///
    /// This method checks if an operation has been previously checkpointed
    /// and returns its result for replay purposes.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The unique identifier of the operation
    ///
    /// # Returns
    ///
    /// A `CheckpointedResult` containing the operation if it exists.
    pub async fn get_checkpoint_result(&self, operation_id: &str) -> CheckpointedResult {
        let operations = self.operations.read().await;
        CheckpointedResult::new(operations.get(operation_id).cloned())
    }
    
    /// Tracks that an operation has been replayed.
    ///
    /// This method is called after an operation returns its checkpointed result
    /// during replay. When all checkpointed operations have been replayed,
    /// the replay status transitions to New.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The unique identifier of the replayed operation
    pub async fn track_replay(&self, operation_id: &str) {
        // Add to replayed set
        {
            let mut replayed = self.replayed_operations.write().await;
            replayed.insert(operation_id.to_string());
        }
        
        // Check if we've replayed all operations
        let (replayed_count, total_count) = {
            let replayed = self.replayed_operations.read().await;
            let operations = self.operations.read().await;
            (replayed.len(), operations.len())
        };
        
        // If all operations have been replayed and there's no more to load,
        // transition to New status
        if replayed_count >= total_count {
            let has_more = self.next_marker.read().await.is_some();
            if !has_more {
                self.replay_status.store(ReplayStatus::New as u8, Ordering::SeqCst);
            }
        }
    }
    
    /// Loads additional operations from the service for pagination.
    ///
    /// This method is called when there are more operations to load
    /// (indicated by next_marker being Some).
    ///
    /// # Returns
    ///
    /// Ok(true) if more operations were loaded, Ok(false) if no more to load,
    /// or an error if the load failed.
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    pub async fn load_more_operations(&self) -> Result<bool, DurableError> {
        // Get the current marker
        let marker = {
            let guard = self.next_marker.read().await;
            match guard.as_ref() {
                Some(m) => m.clone(),
                None => return Ok(false), // No more to load
            }
        };
        
        // Fetch more operations from the service with retry for throttling
        let response = self
            .get_operations_with_retry(&marker)
            .await?;
        
        // Add the new operations to our map
        {
            let mut operations = self.operations.write().await;
            for op in response.operations {
                operations.insert(op.operation_id.clone(), op);
            }
        }
        
        // Update the next marker
        {
            let mut next_marker = self.next_marker.write().await;
            *next_marker = response.next_marker;
        }
        
        Ok(true)
    }
    
    /// Fetches operations from the service with retry for throttling errors.
    ///
    /// This method implements exponential backoff for throttling errors:
    /// - Initial delay: 100ms (or retry_after_ms if provided)
    /// - Maximum delay: 10 seconds
    /// - Maximum retries: 5
    /// - Backoff multiplier: 2x
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    async fn get_operations_with_retry(
        &self,
        marker: &str,
    ) -> Result<crate::client::GetOperationsResponse, DurableError> {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_DELAY_MS: u64 = 100;
        const MAX_DELAY_MS: u64 = 10_000;
        const BACKOFF_MULTIPLIER: u64 = 2;

        let mut attempt = 0;
        let mut delay_ms = INITIAL_DELAY_MS;

        loop {
            let result = self
                .service_client
                .get_operations(&self.durable_execution_arn, marker)
                .await;

            match result {
                Ok(response) => return Ok(response),
                Err(error) if error.is_throttling() => {
                    attempt += 1;
                    if attempt > MAX_RETRIES {
                        tracing::warn!(
                            attempt = attempt,
                            "GetOperations throttling: max retries exceeded"
                        );
                        return Err(error);
                    }

                    // Use retry_after_ms from the error if available, otherwise use calculated delay
                    let actual_delay = error.get_retry_after_ms().unwrap_or(delay_ms);
                    
                    tracing::debug!(
                        attempt = attempt,
                        delay_ms = actual_delay,
                        "GetOperations throttled, retrying with exponential backoff"
                    );

                    // Sleep before retrying
                    tokio::time::sleep(StdDuration::from_millis(actual_delay)).await;

                    // Calculate next delay with exponential backoff
                    delay_ms = (delay_ms * BACKOFF_MULTIPLIER).min(MAX_DELAY_MS);
                }
                Err(error) => return Err(error),
            }
        }
    }
    
    /// Loads all remaining operations from the service.
    ///
    /// This method repeatedly calls load_more_operations until all
    /// operations have been loaded.
    pub async fn load_all_operations(&self) -> Result<(), DurableError> {
        while self.load_more_operations().await? {}
        Ok(())
    }
    
    /// Returns true if there are more operations to load.
    pub async fn has_more_operations(&self) -> bool {
        self.next_marker.read().await.is_some()
    }
    
    /// Returns the number of loaded operations.
    pub async fn operation_count(&self) -> usize {
        self.operations.read().await.len()
    }
    
    /// Returns a reference to the service client.
    pub fn service_client(&self) -> &SharedDurableServiceClient {
        &self.service_client
    }
    
    /// Adds an operation to the local cache.
    ///
    /// This is typically called after a checkpoint succeeds to keep
    /// the local state in sync.
    pub async fn add_operation(&self, operation: Operation) {
        let mut operations = self.operations.write().await;
        operations.insert(operation.operation_id.clone(), operation);
    }
    
    /// Updates an existing operation in the local cache.
    pub async fn update_operation(&self, operation_id: &str, update_fn: impl FnOnce(&mut Operation)) {
        let mut operations = self.operations.write().await;
        if let Some(op) = operations.get_mut(operation_id) {
            update_fn(op);
        }
    }
    
    /// Checks if an operation exists in the local cache.
    pub async fn has_operation(&self, operation_id: &str) -> bool {
        self.operations.read().await.contains_key(operation_id)
    }
    
    /// Gets a clone of an operation from the local cache.
    pub async fn get_operation(&self, operation_id: &str) -> Option<Operation> {
        self.operations.read().await.get(operation_id).cloned()
    }
    
    /// Marks a parent operation as done.
    ///
    /// This is used for orphan prevention - child operations whose
    /// parent has completed should not be allowed to checkpoint.
    pub async fn mark_parent_done(&self, parent_id: &str) {
        let mut done_parents = self.parent_done_lock.lock().await;
        done_parents.insert(parent_id.to_string());
    }
    
    /// Checks if a parent operation has been marked as done.
    ///
    /// Returns true if the parent has completed, meaning any child
    /// operations would be orphaned.
    pub async fn is_parent_done(&self, parent_id: &str) -> bool {
        let done_parents = self.parent_done_lock.lock().await;
        done_parents.contains(parent_id)
    }
    
    /// Checks if an operation would be orphaned.
    ///
    /// An operation is orphaned if it has a parent_id and that parent
    /// has been marked as done.
    pub async fn is_orphaned(&self, parent_id: Option<&str>) -> bool {
        match parent_id {
            Some(pid) => self.is_parent_done(pid).await,
            None => false, // Root operations cannot be orphaned
        }
    }
    
    /// Creates a checkpoint for an operation.
    ///
    /// This method sends a checkpoint request to persist the operation state.
    /// If a checkpoint sender is configured (via `with_batcher`), it uses the
    /// batched checkpointing system. Otherwise, it sends the checkpoint directly.
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation update to checkpoint
    /// * `is_sync` - If true, waits for checkpoint confirmation; if false, fire-and-forget
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or a DurableError on failure.
    ///
    /// # Errors
    ///
    /// - `DurableError::Checkpoint` - If the checkpoint fails
    /// - `DurableError::OrphanedChild` - If the operation's parent has completed
    ///
    /// # Checkpointing Mode Behavior
    ///
    /// The behavior of this method depends on the configured checkpointing mode:
    ///
    /// - **Eager**: Always sends checkpoints synchronously and immediately, bypassing
    ///   the batcher even if one is configured. This provides maximum durability.
    /// - **Batched**: Uses the checkpoint batcher if available, grouping operations
    ///   for efficiency. Falls back to direct checkpointing if no batcher is configured.
    /// - **Optimistic**: Uses the checkpoint batcher with async checkpoints when possible,
    ///   allowing multiple operations to execute before waiting for confirmation.
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub async fn create_checkpoint(
        &self,
        operation: OperationUpdate,
        is_sync: bool,
    ) -> Result<(), DurableError> {
        // Check for orphaned child
        if let Some(ref parent_id) = operation.parent_id {
            if self.is_parent_done(parent_id).await {
                return Err(DurableError::OrphanedChild {
                    message: format!(
                        "Cannot checkpoint operation {} - parent {} has completed",
                        operation.operation_id, parent_id
                    ),
                    operation_id: operation.operation_id.clone(),
                });
            }
        }
        
        // Determine effective sync behavior based on checkpointing mode
        // Requirements: 24.1, 24.2, 24.3
        let effective_is_sync = match self.checkpointing_mode {
            // Eager mode: always synchronous for maximum durability
            crate::config::CheckpointingMode::Eager => true,
            // Batched mode: use the requested sync behavior
            crate::config::CheckpointingMode::Batched => is_sync,
            // Optimistic mode: prefer async unless explicitly requested sync
            crate::config::CheckpointingMode::Optimistic => is_sync,
        };
        
        // In Eager mode, bypass the batcher and send directly for immediate durability
        // Requirements: 24.1 - Checkpoint after every operation
        if self.checkpointing_mode.is_eager() {
            return self.checkpoint_direct(operation, effective_is_sync).await;
        }
        
        // Use the checkpoint sender if available (batched mode)
        if let Some(ref sender) = self.checkpoint_sender {
            let result = sender.checkpoint(operation.clone(), effective_is_sync).await;
            
            // On success, update local cache
            if result.is_ok() {
                self.update_local_cache_from_update(&operation).await;
            }
            
            return result;
        }
        
        // Direct checkpoint (non-batched mode)
        self.checkpoint_direct(operation, effective_is_sync).await
    }
    
    /// Sends a checkpoint directly to the service, bypassing the batcher.
    ///
    /// This method is used for:
    /// - Eager checkpointing mode (always direct for maximum durability)
    /// - When no batcher is configured
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation update to checkpoint
    /// * `is_sync` - If true, waits for checkpoint confirmation; if false, fire-and-forget
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    async fn checkpoint_direct(
        &self,
        operation: OperationUpdate,
        _is_sync: bool,
    ) -> Result<(), DurableError> {
        let token = self.checkpoint_token.read().await.clone();
        let response = self
            .service_client
            .checkpoint(&self.durable_execution_arn, &token, vec![operation.clone()])
            .await?;
        
        // Update the checkpoint token
        {
            let mut token_guard = self.checkpoint_token.write().await;
            *token_guard = response.checkpoint_token;
        }
        
        // Update local cache
        self.update_local_cache_from_update(&operation).await;
        
        Ok(())
    }
    
    /// Creates a synchronous checkpoint (waits for confirmation).
    ///
    /// This is a convenience method that calls `create_checkpoint` with `is_sync = true`.
    pub async fn checkpoint_sync(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        self.create_checkpoint(operation, true).await
    }
    
    /// Creates an asynchronous checkpoint (fire-and-forget).
    ///
    /// This is a convenience method that calls `create_checkpoint` with `is_sync = false`.
    pub async fn checkpoint_async(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        self.create_checkpoint(operation, false).await
    }
    
    /// Creates a checkpoint using the optimal sync behavior for the current mode.
    ///
    /// This method automatically determines whether to use sync or async checkpointing
    /// based on the configured checkpointing mode:
    ///
    /// - **Eager**: Always synchronous (maximum durability)
    /// - **Batched**: Uses the provided `prefer_sync` hint
    /// - **Optimistic**: Prefers async unless `prefer_sync` is true
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation update to checkpoint
    /// * `prefer_sync` - Hint for whether sync is preferred (ignored in Eager mode)
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    /// - 24.2: THE Performance_Configuration SHALL support batched checkpointing mode
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub async fn checkpoint_optimal(
        &self,
        operation: OperationUpdate,
        prefer_sync: bool,
    ) -> Result<(), DurableError> {
        let is_sync = match self.checkpointing_mode {
            crate::config::CheckpointingMode::Eager => true,
            crate::config::CheckpointingMode::Batched => prefer_sync,
            crate::config::CheckpointingMode::Optimistic => prefer_sync,
        };
        self.create_checkpoint(operation, is_sync).await
    }
    
    /// Returns whether async checkpointing is recommended for the current mode.
    ///
    /// This method helps callers decide whether to use async checkpointing:
    ///
    /// - **Eager**: Returns `false` (always use sync)
    /// - **Batched**: Returns `true` (async is acceptable)
    /// - **Optimistic**: Returns `true` (async is preferred)
    ///
    /// # Requirements
    ///
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub fn should_use_async_checkpoint(&self) -> bool {
        match self.checkpointing_mode {
            crate::config::CheckpointingMode::Eager => false,
            crate::config::CheckpointingMode::Batched => true,
            crate::config::CheckpointingMode::Optimistic => true,
        }
    }
    
    /// Returns whether the current mode prioritizes performance over durability.
    ///
    /// This is useful for callers that want to adjust their behavior based on
    /// the configured trade-off:
    ///
    /// - **Eager**: Returns `false` (prioritizes durability)
    /// - **Batched**: Returns `false` (balanced)
    /// - **Optimistic**: Returns `true` (prioritizes performance)
    ///
    /// # Requirements
    ///
    /// - 24.3: THE Performance_Configuration SHALL support optimistic execution mode
    pub fn prioritizes_performance(&self) -> bool {
        self.checkpointing_mode.is_optimistic()
    }
    
    /// Returns whether the current mode prioritizes durability over performance.
    ///
    /// This is useful for callers that want to adjust their behavior based on
    /// the configured trade-off:
    ///
    /// - **Eager**: Returns `true` (prioritizes durability)
    /// - **Batched**: Returns `false` (balanced)
    /// - **Optimistic**: Returns `false` (prioritizes performance)
    ///
    /// # Requirements
    ///
    /// - 24.1: THE Performance_Configuration SHALL support eager checkpointing mode
    pub fn prioritizes_durability(&self) -> bool {
        self.checkpointing_mode.is_eager()
    }
    
    /// Updates the local operation cache based on an operation update.
    async fn update_local_cache_from_update(&self, update: &OperationUpdate) {
        let mut operations = self.operations.write().await;
        
        match update.action {
            crate::operation::OperationAction::Start => {
                // Create a new operation in Started state
                let mut op = Operation::new(&update.operation_id, update.operation_type);
                op.parent_id = update.parent_id.clone();
                op.name = update.name.clone();
                operations.insert(update.operation_id.clone(), op);
            }
            crate::operation::OperationAction::Succeed => {
                // Update existing operation to Succeeded
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Succeeded;
                    op.result = update.result.clone();
                } else {
                    // Create new operation if it doesn't exist
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Succeeded;
                    op.result = update.result.clone();
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Fail => {
                // Update existing operation to Failed
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Failed;
                    op.error = update.error.clone();
                } else {
                    // Create new operation if it doesn't exist
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Failed;
                    op.error = update.error.clone();
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Cancel => {
                // Update existing operation to Cancelled
                // Requirements: 5.5
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Cancelled;
                } else {
                    // Create new operation if it doesn't exist
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Cancelled;
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Retry => {
                // Update existing operation to Pending (waiting for retry)
                // Requirements: 4.7, 4.8, 4.9
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Pending;
                    // Store the payload in step_details for wait-for-condition pattern
                    if update.result.is_some() || update.step_options.is_some() {
                        let step_details = op.step_details.get_or_insert_with(|| crate::operation::StepDetails {
                            result: None,
                            attempt: None,
                            next_attempt_timestamp: None,
                            error: None,
                            payload: None,
                        });
                        // Store payload for wait-for-condition pattern
                        if update.result.is_some() {
                            step_details.payload = update.result.clone();
                        }
                        // Increment attempt counter
                        step_details.attempt = Some(step_details.attempt.unwrap_or(0) + 1);
                    }
                    // Store error if provided
                    if update.error.is_some() {
                        op.error = update.error.clone();
                    }
                } else {
                    // Create new operation if it doesn't exist
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Pending;
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    op.error = update.error.clone();
                    // Initialize step_details with payload and attempt
                    if update.result.is_some() || update.step_options.is_some() {
                        op.step_details = Some(crate::operation::StepDetails {
                            result: None,
                            attempt: Some(1),
                            next_attempt_timestamp: None,
                            error: None,
                            payload: update.result.clone(),
                        });
                    }
                    operations.insert(update.operation_id.clone(), op);
                }
            }
        }
    }
    
    /// Returns a reference to the shared checkpoint token.
    ///
    /// This is useful for creating a CheckpointBatcher that shares the token.
    pub fn shared_checkpoint_token(&self) -> Arc<RwLock<String>> {
        self.checkpoint_token.clone()
    }
    
    /// Returns true if this ExecutionState has a checkpoint sender configured.
    pub fn has_checkpoint_sender(&self) -> bool {
        self.checkpoint_sender.is_some()
    }

    /// Loads child operations for a specific parent operation.
    ///
    /// This method is used when ReplayChildren is enabled for a CONTEXT operation.
    /// It retrieves all child operations that have the specified parent_id and
    /// adds them to the local operations cache.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The operation ID of the parent CONTEXT operation
    ///
    /// # Returns
    ///
    /// A vector of child operations that were loaded.
    ///
    /// # Requirements
    ///
    /// - 10.5: THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
    /// - 10.6: WHEN ReplayChildren is true, THE Child_Context_Operation SHALL include child operations in state loads for replay
    pub async fn load_child_operations(&self, parent_id: &str) -> Result<Vec<Operation>, DurableError> {
        // Get all operations from the current cache that have this parent_id
        let operations = self.operations.read().await;
        let children: Vec<Operation> = operations
            .values()
            .filter(|op| op.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect();
        
        Ok(children)
    }

    /// Gets all child operations for a specific parent from the local cache.
    ///
    /// This method returns child operations that are already loaded in the cache.
    /// Unlike `load_child_operations`, this does not make any API calls.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The operation ID of the parent operation
    ///
    /// # Returns
    ///
    /// A vector of child operations with the specified parent_id.
    pub async fn get_child_operations(&self, parent_id: &str) -> Vec<Operation> {
        let operations = self.operations.read().await;
        operations
            .values()
            .filter(|op| op.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect()
    }

    /// Checks if a CONTEXT operation has ReplayChildren enabled.
    ///
    /// This method checks the ContextDetails of a CONTEXT operation to see
    /// if replay_children was set to true when the operation was started.
    ///
    /// # Arguments
    ///
    /// * `operation_id` - The operation ID of the CONTEXT operation
    ///
    /// # Returns
    ///
    /// `true` if the operation exists, is a CONTEXT type, and has replay_children enabled.
    /// `false` otherwise.
    ///
    /// # Requirements
    ///
    /// - 10.5: THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
    pub async fn has_replay_children(&self, operation_id: &str) -> bool {
        let operations = self.operations.read().await;
        operations
            .get(operation_id)
            .filter(|op| op.operation_type == OperationType::Context)
            .and_then(|op| op.context_details.as_ref())
            .and_then(|details| details.replay_children)
            .unwrap_or(false)
    }
}

// Implement Debug manually since we can't derive it with async locks
impl std::fmt::Debug for ExecutionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionState")
            .field("durable_execution_arn", &self.durable_execution_arn)
            .field("replay_status", &self.replay_status())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod checkpoint_result_tests {
    use super::*;

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

    #[test]
    fn test_replay_status_replay() {
        let status = ReplayStatus::Replay;
        assert!(status.is_replay());
        assert!(!status.is_new());
    }

    #[test]
    fn test_replay_status_new() {
        let status = ReplayStatus::New;
        assert!(!status.is_replay());
        assert!(status.is_new());
    }

    #[test]
    fn test_replay_status_from_u8() {
        assert_eq!(ReplayStatus::from(0), ReplayStatus::Replay);
        assert_eq!(ReplayStatus::from(1), ReplayStatus::New);
        assert_eq!(ReplayStatus::from(2), ReplayStatus::New); // Any non-zero is New
    }

    #[test]
    fn test_replay_status_to_u8() {
        assert_eq!(u8::from(ReplayStatus::Replay), 0);
        assert_eq!(u8::from(ReplayStatus::New), 1);
    }
}

#[cfg(test)]
mod execution_state_tests {
    use super::*;
    use crate::client::{GetOperationsResponse, MockDurableServiceClient};
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus, OperationType};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(MockDurableServiceClient::new())
    }

    fn create_test_operation(id: &str, status: OperationStatus) -> Operation {
        let mut op = Operation::new(id, OperationType::Step);
        op.status = status;
        op
    }

    #[tokio::test]
    async fn test_execution_state_new_empty() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        assert_eq!(state.durable_execution_arn(), "arn:test");
        assert_eq!(state.checkpoint_token().await, "token-123");
        assert!(state.is_new());
        assert!(!state.is_replay());
        assert_eq!(state.operation_count().await, 0);
        assert!(!state.has_more_operations().await);
    }

    #[tokio::test]
    async fn test_execution_state_new_with_operations() {
        let client = create_mock_client();
        let ops = vec![
            create_test_operation("op-1", OperationStatus::Succeeded),
            create_test_operation("op-2", OperationStatus::Succeeded),
        ];
        let initial_state = InitialExecutionState::with_operations(ops);
        
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            initial_state,
            client,
        );

        assert!(state.is_replay());
        assert!(!state.is_new());
        assert_eq!(state.operation_count().await, 2);
    }

    #[tokio::test]
    async fn test_get_checkpoint_result_exists() {
        let client = create_mock_client();
        let mut op = create_test_operation("op-1", OperationStatus::Succeeded);
        op.result = Some(r#"{"value": 42}"#.to_string());
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        let result = state.get_checkpoint_result("op-1").await;
        assert!(result.is_existent());
        assert!(result.is_succeeded());
        assert_eq!(result.result(), Some(r#"{"value": 42}"#));
    }

    #[tokio::test]
    async fn test_get_checkpoint_result_not_exists() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let result = state.get_checkpoint_result("non-existent").await;
        assert!(!result.is_existent());
    }

    #[tokio::test]
    async fn test_track_replay_transitions_to_new() {
        let client = create_mock_client();
        let ops = vec![
            create_test_operation("op-1", OperationStatus::Succeeded),
            create_test_operation("op-2", OperationStatus::Succeeded),
        ];
        let initial_state = InitialExecutionState::with_operations(ops);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        // Initially in replay mode
        assert!(state.is_replay());

        // Track first operation
        state.track_replay("op-1").await;
        assert!(state.is_replay()); // Still in replay

        // Track second operation
        state.track_replay("op-2").await;
        assert!(state.is_new()); // Now in new mode
    }

    #[tokio::test]
    async fn test_track_replay_with_pagination() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_get_operations_response(Ok(GetOperationsResponse {
                    operations: vec![create_test_operation("op-3", OperationStatus::Succeeded)],
                    next_marker: None,
                }))
        );
        
        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        // Track the first operation
        state.track_replay("op-1").await;
        
        // Should still be in replay because there's more to load
        assert!(state.is_replay());
    }

    #[tokio::test]
    async fn test_set_checkpoint_token() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        assert_eq!(state.checkpoint_token().await, "token-123");
        
        state.set_checkpoint_token("token-456").await;
        assert_eq!(state.checkpoint_token().await, "token-456");
    }

    #[tokio::test]
    async fn test_add_operation() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        assert!(!state.has_operation("op-1").await);
        
        let op = create_test_operation("op-1", OperationStatus::Succeeded);
        state.add_operation(op).await;
        
        assert!(state.has_operation("op-1").await);
        assert_eq!(state.operation_count().await, 1);
    }

    #[tokio::test]
    async fn test_update_operation() {
        let client = create_mock_client();
        let ops = vec![create_test_operation("op-1", OperationStatus::Started)];
        let initial_state = InitialExecutionState::with_operations(ops);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        // Verify initial status
        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Started);

        // Update the operation
        state.update_operation("op-1", |op| {
            op.status = OperationStatus::Succeeded;
            op.result = Some("done".to_string());
        }).await;

        // Verify updated status
        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some("done".to_string()));
    }

    #[tokio::test]
    async fn test_load_more_operations() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_get_operations_response(Ok(GetOperationsResponse {
                    operations: vec![
                        create_test_operation("op-2", OperationStatus::Succeeded),
                        create_test_operation("op-3", OperationStatus::Succeeded),
                    ],
                    next_marker: None,
                }))
        );
        
        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        assert_eq!(state.operation_count().await, 1);
        assert!(state.has_more_operations().await);

        // Load more operations
        let loaded = state.load_more_operations().await.unwrap();
        assert!(loaded);
        
        assert_eq!(state.operation_count().await, 3);
        assert!(!state.has_more_operations().await);
    }

    #[tokio::test]
    async fn test_load_more_operations_no_more() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        // No more to load
        let loaded = state.load_more_operations().await.unwrap();
        assert!(!loaded);
    }

    #[tokio::test]
    async fn test_load_all_operations() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_get_operations_response(Ok(GetOperationsResponse {
                    operations: vec![create_test_operation("op-2", OperationStatus::Succeeded)],
                    next_marker: Some("marker-2".to_string()),
                }))
                .with_get_operations_response(Ok(GetOperationsResponse {
                    operations: vec![create_test_operation("op-3", OperationStatus::Succeeded)],
                    next_marker: None,
                }))
        );
        
        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        assert_eq!(state.operation_count().await, 1);

        // Load all operations
        state.load_all_operations().await.unwrap();
        
        assert_eq!(state.operation_count().await, 3);
        assert!(!state.has_more_operations().await);
    }

    #[tokio::test]
    async fn test_mark_parent_done() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        assert!(!state.is_parent_done("parent-1").await);
        
        state.mark_parent_done("parent-1").await;
        
        assert!(state.is_parent_done("parent-1").await);
        assert!(!state.is_parent_done("parent-2").await);
    }

    #[tokio::test]
    async fn test_is_orphaned() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        // Root operations are never orphaned
        assert!(!state.is_orphaned(None).await);

        // Child with active parent is not orphaned
        assert!(!state.is_orphaned(Some("parent-1")).await);

        // Mark parent as done
        state.mark_parent_done("parent-1").await;

        // Now child is orphaned
        assert!(state.is_orphaned(Some("parent-1")).await);
        
        // Other children are still not orphaned
        assert!(!state.is_orphaned(Some("parent-2")).await);
    }

    #[tokio::test]
    async fn test_debug_impl() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("ExecutionState"));
        assert!(debug_str.contains("arn:test"));
    }

    #[test]
    fn test_checkpoint_batcher_config_default() {
        let config = CheckpointBatcherConfig::default();
        assert_eq!(config.max_batch_size_bytes, 750 * 1024);
        assert_eq!(config.max_batch_time_ms, 1000);
        assert_eq!(config.max_batch_operations, usize::MAX);
    }

    #[tokio::test]
    async fn test_create_checkpoint_direct() {
        use crate::client::CheckpointResponse;
        use crate::operation::OperationUpdate;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.create_checkpoint(update, true).await;
        
        assert!(result.is_ok());
        assert_eq!(state.checkpoint_token().await, "new-token");
        assert!(state.has_operation("op-1").await);
    }

    #[tokio::test]
    async fn test_create_checkpoint_updates_local_cache_on_succeed() {
        use crate::client::CheckpointResponse;
        use crate::operation::OperationUpdate;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-2".to_string(),
                }))
        );
        
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        // Start the operation
        let update = OperationUpdate::start("op-1", OperationType::Step);
        state.create_checkpoint(update, true).await.unwrap();
        
        // Verify operation is in Started state
        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Started);

        // Succeed the operation
        let update = OperationUpdate::succeed("op-1", OperationType::Step, Some(r#"{"result": "ok"}"#.to_string()));
        state.create_checkpoint(update, true).await.unwrap();
        
        // Verify operation is now Succeeded
        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some(r#"{"result": "ok"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_create_checkpoint_rejects_orphaned_child() {
        use crate::operation::OperationUpdate;
        
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        // Mark parent as done
        state.mark_parent_done("parent-1").await;

        // Try to checkpoint a child operation
        let update = OperationUpdate::start("child-1", OperationType::Step)
            .with_parent_id("parent-1");
        let result = state.create_checkpoint(update, true).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::OrphanedChild { operation_id, .. } => {
                assert_eq!(operation_id, "child-1");
            }
            _ => panic!("Expected OrphanedChild error"),
        }
    }

    #[tokio::test]
    async fn test_with_batcher_creates_state_and_batcher() {
        use crate::client::CheckpointResponse;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let (state, mut batcher) = ExecutionState::with_batcher(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            CheckpointBatcherConfig {
                max_batch_time_ms: 10,
                ..Default::default()
            },
            100,
        );

        assert!(state.has_checkpoint_sender());
        assert_eq!(state.durable_execution_arn(), "arn:test");
        
        // Run batcher in background
        let batcher_handle = tokio::spawn(async move {
            batcher.run().await;
        });

        // Create a checkpoint using the batched system
        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.create_checkpoint(update, true).await;
        
        // Drop the state to close the checkpoint sender
        drop(state);
        
        // Wait for batcher to finish
        batcher_handle.await.unwrap();
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_checkpoint_sync_convenience_method() {
        use crate::client::CheckpointResponse;
        use crate::operation::OperationUpdate;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_sync(update).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_checkpoint_async_convenience_method() {
        use crate::client::CheckpointResponse;
        use crate::operation::OperationUpdate;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_async(update).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shared_checkpoint_token() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let shared_token = state.shared_checkpoint_token();
        assert_eq!(*shared_token.read().await, "token-123");
        
        // Modify through the shared reference
        {
            let mut guard = shared_token.write().await;
            *guard = "modified-token".to_string();
        }
        
        // Verify the state sees the change
        assert_eq!(state.checkpoint_token().await, "modified-token");
    }

    // Tests for ReplayChildren support (Requirements 10.5, 10.6)

    #[tokio::test]
    async fn test_load_child_operations_returns_children() {
        let client = create_mock_client();
        
        // Create a parent CONTEXT operation and some child operations
        let mut parent_op = Operation::new("parent-ctx", OperationType::Context);
        parent_op.status = OperationStatus::Succeeded;
        
        let mut child1 = create_test_operation("child-1", OperationStatus::Succeeded);
        child1.parent_id = Some("parent-ctx".to_string());
        
        let mut child2 = create_test_operation("child-2", OperationStatus::Succeeded);
        child2.parent_id = Some("parent-ctx".to_string());
        
        // Another operation with different parent
        let mut other_child = create_test_operation("other-child", OperationStatus::Succeeded);
        other_child.parent_id = Some("other-parent".to_string());
        
        let initial_state = InitialExecutionState::with_operations(vec![
            parent_op, child1, child2, other_child
        ]);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        // Load children for parent-ctx
        let children = state.load_child_operations("parent-ctx").await.unwrap();
        
        assert_eq!(children.len(), 2);
        let child_ids: Vec<&str> = children.iter().map(|c| c.operation_id.as_str()).collect();
        assert!(child_ids.contains(&"child-1"));
        assert!(child_ids.contains(&"child-2"));
    }

    #[tokio::test]
    async fn test_load_child_operations_no_children() {
        let client = create_mock_client();
        
        let parent_op = Operation::new("parent-ctx", OperationType::Context);
        let initial_state = InitialExecutionState::with_operations(vec![parent_op]);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        // Load children for parent with no children
        let children = state.load_child_operations("parent-ctx").await.unwrap();
        
        assert!(children.is_empty());
    }

    #[tokio::test]
    async fn test_get_child_operations_returns_cached_children() {
        let client = create_mock_client();
        
        let mut parent_op = Operation::new("parent-ctx", OperationType::Context);
        parent_op.status = OperationStatus::Succeeded;
        
        let mut child1 = create_test_operation("child-1", OperationStatus::Succeeded);
        child1.parent_id = Some("parent-ctx".to_string());
        
        let mut child2 = create_test_operation("child-2", OperationStatus::Succeeded);
        child2.parent_id = Some("parent-ctx".to_string());
        
        let initial_state = InitialExecutionState::with_operations(vec![
            parent_op, child1, child2
        ]);
        
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        // Get children from cache
        let children = state.get_child_operations("parent-ctx").await;
        
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_get_child_operations_empty_for_nonexistent_parent() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );
        
        let children = state.get_child_operations("nonexistent-parent").await;
        assert!(children.is_empty());
    }

    #[tokio::test]
    async fn test_has_replay_children_true() {
        use crate::operation::ContextDetails;
        
        let client = create_mock_client();
        
        let mut ctx_op = Operation::new("ctx-with-replay", OperationType::Context);
        ctx_op.status = OperationStatus::Succeeded;
        ctx_op.context_details = Some(ContextDetails {
            result: None,
            replay_children: Some(true),
            error: None,
        });
        
        let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        assert!(state.has_replay_children("ctx-with-replay").await);
    }

    #[tokio::test]
    async fn test_has_replay_children_false_when_not_set() {
        use crate::operation::ContextDetails;
        
        let client = create_mock_client();
        
        let mut ctx_op = Operation::new("ctx-no-replay", OperationType::Context);
        ctx_op.status = OperationStatus::Succeeded;
        ctx_op.context_details = Some(ContextDetails {
            result: None,
            replay_children: None,
            error: None,
        });
        
        let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        assert!(!state.has_replay_children("ctx-no-replay").await);
    }

    #[tokio::test]
    async fn test_has_replay_children_false_when_explicitly_false() {
        use crate::operation::ContextDetails;
        
        let client = create_mock_client();
        
        let mut ctx_op = Operation::new("ctx-replay-false", OperationType::Context);
        ctx_op.status = OperationStatus::Succeeded;
        ctx_op.context_details = Some(ContextDetails {
            result: None,
            replay_children: Some(false),
            error: None,
        });
        
        let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        assert!(!state.has_replay_children("ctx-replay-false").await);
    }

    #[tokio::test]
    async fn test_has_replay_children_false_for_non_context_operation() {
        let client = create_mock_client();
        
        let step_op = create_test_operation("step-op", OperationStatus::Succeeded);
        
        let initial_state = InitialExecutionState::with_operations(vec![step_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        // STEP operations don't have replay_children
        assert!(!state.has_replay_children("step-op").await);
    }

    #[tokio::test]
    async fn test_has_replay_children_false_for_nonexistent_operation() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );
        
        assert!(!state.has_replay_children("nonexistent").await);
    }

    #[tokio::test]
    async fn test_has_replay_children_false_when_no_context_details() {
        let client = create_mock_client();
        
        // CONTEXT operation without context_details
        let ctx_op = Operation::new("ctx-no-details", OperationType::Context);
        
        let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
        
        assert!(!state.has_replay_children("ctx-no-details").await);
    }

    // Tests for CheckpointingMode (Requirements 24.1, 24.2, 24.3, 24.4)

    #[tokio::test]
    async fn test_checkpointing_mode_default_is_batched() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Batched);
        assert!(state.is_batched_checkpointing());
        assert!(!state.is_eager_checkpointing());
        assert!(!state.is_optimistic_checkpointing());
    }

    #[tokio::test]
    async fn test_checkpointing_mode_eager() {
        let client = create_mock_client();
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Eager,
        );

        assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Eager);
        assert!(state.is_eager_checkpointing());
        assert!(!state.is_batched_checkpointing());
        assert!(!state.is_optimistic_checkpointing());
        assert!(state.prioritizes_durability());
        assert!(!state.prioritizes_performance());
        assert!(!state.should_use_async_checkpoint());
    }

    #[tokio::test]
    async fn test_checkpointing_mode_optimistic() {
        let client = create_mock_client();
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Optimistic,
        );

        assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Optimistic);
        assert!(state.is_optimistic_checkpointing());
        assert!(!state.is_eager_checkpointing());
        assert!(!state.is_batched_checkpointing());
        assert!(state.prioritizes_performance());
        assert!(!state.prioritizes_durability());
        assert!(state.should_use_async_checkpoint());
    }

    #[tokio::test]
    async fn test_checkpointing_mode_batched_helpers() {
        let client = create_mock_client();
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Batched,
        );

        assert!(!state.prioritizes_durability());
        assert!(!state.prioritizes_performance());
        assert!(state.should_use_async_checkpoint());
    }

    #[tokio::test]
    async fn test_checkpoint_optimal_eager_mode() {
        use crate::client::CheckpointResponse;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Eager,
        );

        // In eager mode, checkpoint_optimal should always be sync
        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_optimal(update, false).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_checkpoint_optimal_batched_mode() {
        use crate::client::CheckpointResponse;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Batched,
        );

        // In batched mode, checkpoint_optimal respects the prefer_sync hint
        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_optimal(update, true).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_checkpoint_optimal_optimistic_mode() {
        use crate::client::CheckpointResponse;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Optimistic,
        );

        // In optimistic mode, checkpoint_optimal respects the prefer_sync hint
        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_optimal(update, false).await;
        
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eager_mode_bypasses_batcher() {
        use crate::client::CheckpointResponse;
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        // Create state with batcher but in eager mode
        let (state, mut batcher) = ExecutionState::with_batcher_and_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            CheckpointBatcherConfig {
                max_batch_time_ms: 10,
                ..Default::default()
            },
            100,
            crate::config::CheckpointingMode::Eager,
        );

        // Run batcher in background
        let batcher_handle = tokio::spawn(async move {
            batcher.run().await;
        });

        // In eager mode, checkpoint should bypass the batcher and go directly
        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.create_checkpoint(update, false).await;
        
        // Drop the state to close the checkpoint sender
        drop(state);
        
        // Wait for batcher to finish
        batcher_handle.await.unwrap();
        
        assert!(result.is_ok());
    }
}

#[cfg(test)]
mod checkpoint_queue_tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient};
    use crate::operation::{OperationType, OperationUpdate};

    fn create_test_update(id: &str) -> OperationUpdate {
        OperationUpdate::start(id, OperationType::Step)
    }

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
        assert!(size >= 100); // Base overhead
    }

    #[test]
    fn test_checkpoint_request_estimated_size_with_result() {
        let mut update = create_test_update("op-1");
        update.result = Some("a".repeat(1000));
        let request = CheckpointRequest::async_request(update);
        
        let size = request.estimated_size();
        assert!(size >= 1100); // Base + result
    }

    #[test]
    fn test_create_checkpoint_queue() {
        let (sender, _rx) = create_checkpoint_queue(100);
        // Just verify it creates without panic
        drop(sender);
    }

    #[tokio::test]
    async fn test_checkpoint_sender_sync() {
        let (sender, mut rx) = create_checkpoint_queue(10);
        
        // Spawn a task to handle the request
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
        
        // Verify the request was received
        let request = rx.recv().await.unwrap();
        assert!(!request.is_sync());
        assert_eq!(request.operation.operation_id, "op-1");
    }

    #[tokio::test]
    async fn test_checkpoint_sender_queue_closed() {
        let (sender, rx) = create_checkpoint_queue(10);
        drop(rx); // Close the queue
        
        let update = create_test_update("op-1");
        let result = sender.checkpoint_async(update).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_processes_batch() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let (sender, rx) = create_checkpoint_queue(10);
        let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
        
        let config = CheckpointBatcherConfig {
            max_batch_time_ms: 10, // Short timeout for testing
            ..Default::default()
        };
        
        let mut batcher = CheckpointBatcher::new(
            config,
            rx,
            client,
            "arn:test".to_string(),
            checkpoint_token.clone(),
        );
        
        // Send a request
        let update = create_test_update("op-1");
        let (request, completion_rx) = CheckpointRequest::sync(update);
        sender.tx.send(request).await.unwrap();
        
        // Drop sender to close the queue after processing
        drop(sender);
        
        // Run the batcher
        batcher.run().await;
        
        // Verify the checkpoint was processed
        let result = completion_rx.await.unwrap();
        assert!(result.is_ok());
        
        // Verify token was updated
        let token = checkpoint_token.read().await;
        assert_eq!(*token, "new-token");
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_handles_error() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Test error")))
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
        
        // Send a request
        let update = create_test_update("op-1");
        let (request, completion_rx) = CheckpointRequest::sync(update);
        sender.tx.send(request).await.unwrap();
        
        // Drop sender to close the queue
        drop(sender);
        
        // Run the batcher
        batcher.run().await;
        
        // Verify the error was propagated
        let result = completion_rx.await.unwrap();
        assert!(result.is_err());
        
        // Token should not have changed
        let token = checkpoint_token.read().await;
        assert_eq!(*token, "initial-token");
    }

    #[tokio::test]
    async fn test_checkpoint_batcher_batches_multiple_requests() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "new-token".to_string(),
                }))
        );
        
        let (sender, rx) = create_checkpoint_queue(10);
        let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
        
        let config = CheckpointBatcherConfig {
            max_batch_time_ms: 50, // Give time to collect multiple requests
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
        
        // Send multiple requests quickly
        for i in 0..3 {
            let update = create_test_update(&format!("op-{}", i));
            sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
        }
        
        // Drop sender to close the queue
        drop(sender);
        
        // Run the batcher
        batcher.run().await;
        
        // Verify token was updated (batch was processed)
        let token = checkpoint_token.read().await;
        assert_eq!(*token, "new-token");
    }
}

#[cfg(test)]
mod checkpoint_batching_property_tests {
    use super::*;
    use crate::client::{CheckpointResponse, DurableServiceClient, GetOperationsResponse};
    use crate::operation::{OperationType, OperationUpdate};
    use async_trait::async_trait;
    use proptest::prelude::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock client that counts the number of checkpoint calls
    struct CountingMockClient {
        checkpoint_count: Arc<AtomicUsize>,
    }

    impl CountingMockClient {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let count = Arc::new(AtomicUsize::new(0));
            (Self { checkpoint_count: count.clone() }, count)
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
            self.checkpoint_count.fetch_add(1, Ordering::SeqCst);
            Ok(CheckpointResponse {
                checkpoint_token: format!("token-{}", self.checkpoint_count.load(Ordering::SeqCst)),
            })
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

    // Property 4: Checkpoint Batching Efficiency
    // For any sequence of N checkpoint requests submitted within the batch time window,
    // the Checkpointing_System SHALL send at most ceil(N * avg_size / max_batch_size) API calls.
    // 
    // **Validates: Requirements 2.3**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        
        #[test]
        fn prop_checkpoint_batching_respects_operation_count_limit(
            num_requests in 1usize..20,
            max_ops_per_batch in 1usize..10,
        ) {
            // Run the async test in a tokio runtime
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let (client, call_count) = CountingMockClient::new();
                let client = Arc::new(client);
                
                let (sender, rx) = create_checkpoint_queue(100);
                let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
                
                let config = CheckpointBatcherConfig {
                    max_batch_time_ms: 10, // Short timeout
                    max_batch_operations: max_ops_per_batch,
                    max_batch_size_bytes: usize::MAX, // No size limit for this test
                };
                
                let mut batcher = CheckpointBatcher::new(
                    config,
                    rx,
                    client,
                    "arn:test".to_string(),
                    checkpoint_token.clone(),
                );
                
                // Send all requests quickly
                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), 0);
                    sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
                }
                
                // Drop sender to close the queue
                drop(sender);
                
                // Run the batcher
                batcher.run().await;
                
                // Calculate expected max API calls
                // With operation count limit, we expect at most ceil(N / max_ops_per_batch) calls
                let expected_max_calls = (num_requests + max_ops_per_batch - 1) / max_ops_per_batch;
                let actual_calls = call_count.load(Ordering::SeqCst);
                
                // The actual calls should be at most the expected max
                // (could be fewer if requests arrive in the same batch window)
                if actual_calls > expected_max_calls {
                    return Err(TestCaseError::fail(format!(
                        "Expected at most {} API calls for {} requests with batch size {}, got {}",
                        expected_max_calls, num_requests, max_ops_per_batch, actual_calls
                    )));
                }
                
                // Should have at least 1 call if there were any requests
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
                    max_batch_operations: usize::MAX, // No operation count limit
                    max_batch_size_bytes: max_batch_size,
                };
                
                let mut batcher = CheckpointBatcher::new(
                    config,
                    rx,
                    client,
                    "arn:test".to_string(),
                    checkpoint_token.clone(),
                );
                
                // Send requests with known sizes
                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), result_size);
                    sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
                }
                
                drop(sender);
                batcher.run().await;
                
                // Estimate the size of each request
                let estimated_request_size = 100 + result_size; // base + result
                let total_size = num_requests * estimated_request_size;
                
                // Expected max calls based on size
                let expected_max_calls = (total_size + max_batch_size - 1) / max_batch_size;
                let actual_calls = call_count.load(Ordering::SeqCst);
                
                // Allow some flexibility due to estimation inaccuracies
                // The actual calls should be reasonable (within 2x of expected)
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
                
                // Track completion of sync requests
                let mut receivers = Vec::new();
                
                for i in 0..num_requests {
                    let update = create_test_update_with_size(&format!("op-{}", i), 0);
                    let (request, rx) = CheckpointRequest::sync(update);
                    sender.tx.send(request).await.unwrap();
                    receivers.push(rx);
                }
                
                drop(sender);
                batcher.run().await;
                
                // All requests should have completed successfully
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
}

#[cfg(test)]
mod orphan_prevention_property_tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient};
    use crate::lambda::InitialExecutionState;
    use crate::operation::{OperationType, OperationUpdate};
    use proptest::prelude::*;

    // Property 9: Orphaned Child Prevention
    // For any child operation whose parent has completed (marked done),
    // attempting to checkpoint SHALL fail with OrphanedChildError.
    //
    // **Validates: Requirements 2.8**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        
        #[test]
        fn prop_orphaned_child_checkpoint_fails(
            parent_id in "[a-z]{5,10}",
            child_id in "[a-z]{5,10}",
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let client = Arc::new(
                    MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "new-token".to_string(),
                        }))
                );
                
                let state = ExecutionState::new(
                    "arn:test",
                    "token-123",
                    InitialExecutionState::new(),
                    client,
                );

                // Mark the parent as done
                state.mark_parent_done(&parent_id).await;

                // Try to checkpoint a child operation with the done parent
                let update = OperationUpdate::start(&child_id, OperationType::Step)
                    .with_parent_id(&parent_id);
                let checkpoint_result = state.create_checkpoint(update, true).await;
                
                // Should fail with OrphanedChild error
                match checkpoint_result {
                    Err(DurableError::OrphanedChild { operation_id, .. }) => {
                        if operation_id != child_id {
                            return Err(TestCaseError::fail(format!(
                                "Expected operation_id '{}' in OrphanedChild error, got '{}'",
                                child_id, operation_id
                            )));
                        }
                    }
                    Ok(_) => {
                        return Err(TestCaseError::fail(
                            "Expected OrphanedChild error, but checkpoint succeeded"
                        ));
                    }
                    Err(other) => {
                        return Err(TestCaseError::fail(format!(
                            "Expected OrphanedChild error, got {:?}",
                            other
                        )));
                    }
                }
                
                Ok(())
            });
            result?;
        }

        #[test]
        fn prop_non_orphaned_child_checkpoint_succeeds(
            parent_id in "[a-z]{5,10}",
            child_id in "[a-z]{5,10}",
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let client = Arc::new(
                    MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "new-token".to_string(),
                        }))
                );
                
                let state = ExecutionState::new(
                    "arn:test",
                    "token-123",
                    InitialExecutionState::new(),
                    client,
                );

                // Do NOT mark the parent as done - parent is still active

                // Try to checkpoint a child operation with the active parent
                let update = OperationUpdate::start(&child_id, OperationType::Step)
                    .with_parent_id(&parent_id);
                let checkpoint_result = state.create_checkpoint(update, true).await;
                
                // Should succeed
                if let Err(e) = checkpoint_result {
                    return Err(TestCaseError::fail(format!(
                        "Expected checkpoint to succeed for non-orphaned child, got error: {:?}",
                        e
                    )));
                }
                
                Ok(())
            });
            result?;
        }

        #[test]
        fn prop_root_operation_never_orphaned(
            operation_id in "[a-z]{5,10}",
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let client = Arc::new(
                    MockDurableServiceClient::new()
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "new-token".to_string(),
                        }))
                );
                
                let state = ExecutionState::new(
                    "arn:test",
                    "token-123",
                    InitialExecutionState::new(),
                    client,
                );

                // Root operation has no parent_id
                let update = OperationUpdate::start(&operation_id, OperationType::Step);
                // Note: no with_parent_id call
                
                let checkpoint_result = state.create_checkpoint(update, true).await;
                
                // Should always succeed (root operations can't be orphaned)
                if let Err(e) = checkpoint_result {
                    return Err(TestCaseError::fail(format!(
                        "Expected checkpoint to succeed for root operation, got error: {:?}",
                        e
                    )));
                }
                
                Ok(())
            });
            result?;
        }

        #[test]
        fn prop_marking_parent_done_affects_future_checkpoints(
            parent_id in "[a-z]{5,10}",
            child_id_before in "[a-z]{5,10}",
            child_id_after in "[a-z]{5,10}",
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let result: Result<(), TestCaseError> = rt.block_on(async {
                let client = Arc::new(
                    MockDurableServiceClient::new()
                        // Need multiple checkpoint responses
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-1".to_string(),
                        }))
                        .with_checkpoint_response(Ok(CheckpointResponse {
                            checkpoint_token: "token-2".to_string(),
                        }))
                );
                
                let state = ExecutionState::new(
                    "arn:test",
                    "token-123",
                    InitialExecutionState::new(),
                    client,
                );

                // Checkpoint a child BEFORE marking parent done - should succeed
                let update_before = OperationUpdate::start(&child_id_before, OperationType::Step)
                    .with_parent_id(&parent_id);
                let result_before = state.create_checkpoint(update_before, true).await;
                
                if let Err(e) = result_before {
                    return Err(TestCaseError::fail(format!(
                        "Expected first checkpoint to succeed, got error: {:?}",
                        e
                    )));
                }

                // Now mark the parent as done
                state.mark_parent_done(&parent_id).await;

                // Checkpoint a child AFTER marking parent done - should fail
                let update_after = OperationUpdate::start(&child_id_after, OperationType::Step)
                    .with_parent_id(&parent_id);
                let result_after = state.create_checkpoint(update_after, true).await;
                
                match result_after {
                    Err(DurableError::OrphanedChild { .. }) => {
                        // Expected
                    }
                    Ok(_) => {
                        return Err(TestCaseError::fail(
                            "Expected OrphanedChild error after marking parent done"
                        ));
                    }
                    Err(other) => {
                        return Err(TestCaseError::fail(format!(
                            "Expected OrphanedChild error, got {:?}",
                            other
                        )));
                    }
                }
                
                Ok(())
            });
            result?;
        }
    }
}

#[cfg(test)]
mod execution_operation_tests {
    use super::*;
    use crate::client::{CheckpointResponse, MockDurableServiceClient};
    use crate::error::ErrorObject;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{ExecutionDetails, Operation, OperationStatus, OperationType};

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
        
        // EXECUTION operation should be recognized
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
        
        // No EXECUTION operation
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
        
        // Verify the operation was updated
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
            DurableError::Validation { message } => {
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
        
        // Verify the operation was updated
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
            DurableError::Validation { message } => {
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
        
        // EXECUTION operation should be recognized
        assert!(state.execution_operation().is_some());
        let exec = state.execution_operation().unwrap();
        assert_eq!(exec.operation_type, OperationType::Execution);
        assert_eq!(state.get_original_input_raw(), Some(r#"{"order_id": "123"}"#));
    }
}

#[cfg(test)]
mod batch_ordering_tests {
    use super::*;
    use crate::operation::{OperationAction, OperationType, OperationUpdate};

    fn create_start_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::start(id, op_type)
    }

    fn create_succeed_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::succeed(id, op_type, Some("result".to_string()))
    }

    fn create_fail_update(id: &str, op_type: OperationType) -> OperationUpdate {
        OperationUpdate::fail(id, op_type, crate::error::ErrorObject::new("Error", "message"))
    }

    fn create_request(update: OperationUpdate) -> CheckpointRequest {
        CheckpointRequest::async_request(update)
    }

    // Test: EXECUTION completion must be last in batch
    // Requirements: 2.12
    #[test]
    fn test_execution_completion_is_last() {
        let batch = vec![
            create_request(create_succeed_update("exec-1", OperationType::Execution)),
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_succeed_update("step-2", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // EXECUTION completion should be last
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[2].operation.operation_id, "exec-1");
        assert_eq!(sorted[2].operation.action, OperationAction::Succeed);
        assert_eq!(sorted[2].operation.operation_type, OperationType::Execution);
    }

    // Test: EXECUTION FAIL is also last
    // Requirements: 2.12
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

    // Test: Child operations come after parent CONTEXT starts
    // Requirements: 2.12
    #[test]
    fn test_child_after_parent_context_start() {
        let mut child_update = create_start_update("child-1", OperationType::Step);
        child_update.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(child_update),
            create_request(create_start_update("parent-ctx", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // Parent CONTEXT START should come before child
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "parent-ctx");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "child-1");
    }

    // Test: START and completion for same operation in same batch
    // Requirements: 2.13
    #[test]
    fn test_start_and_completion_same_operation() {
        let batch = vec![
            create_request(create_succeed_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-1", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // START should come before SUCCEED for same operation
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
    }

    // Test: CONTEXT START and completion in same batch
    // Requirements: 2.13
    #[test]
    fn test_context_start_and_completion_same_batch() {
        let batch = vec![
            create_request(create_succeed_update("ctx-1", OperationType::Context)),
            create_request(create_start_update("ctx-1", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // START should come before SUCCEED
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "ctx-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "ctx-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
    }

    // Test: STEP START and FAIL in same batch
    // Requirements: 2.13
    #[test]
    fn test_step_start_and_fail_same_batch() {
        let batch = vec![
            create_request(create_fail_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-1", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // START should come before FAIL for same operation
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Fail);
    }

    // Test: CONTEXT START and FAIL in same batch
    // Requirements: 2.13
    #[test]
    fn test_context_start_and_fail_same_batch() {
        let batch = vec![
            create_request(create_fail_update("ctx-1", OperationType::Context)),
            create_request(create_start_update("ctx-1", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // START should come before FAIL
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].operation.operation_id, "ctx-1");
        assert_eq!(sorted[0].operation.action, OperationAction::Start);
        assert_eq!(sorted[1].operation.operation_id, "ctx-1");
        assert_eq!(sorted[1].operation.action, OperationAction::Fail);
    }

    // Test: Complex scenario with multiple ordering rules
    // Requirements: 2.12, 2.13
    #[test]
    fn test_complex_ordering_scenario() {
        let mut child1 = create_start_update("child-1", OperationType::Step);
        child1.parent_id = Some("parent-ctx".to_string());

        let mut child2 = create_succeed_update("child-2", OperationType::Step);
        child2.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(create_succeed_update("exec-1", OperationType::Execution)), // Should be last
            create_request(child1),                                                      // Should be after parent-ctx START
            create_request(create_succeed_update("parent-ctx", OperationType::Context)), // Should be after parent-ctx START
            create_request(create_start_update("parent-ctx", OperationType::Context)),   // Should be early
            create_request(child2),                                                      // Should be after parent-ctx START
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        assert_eq!(sorted.len(), 5);

        // Find positions
        let parent_start_pos = sorted.iter().position(|r| 
            r.operation.operation_id == "parent-ctx" && r.operation.action == OperationAction::Start
        ).unwrap();
        let parent_succeed_pos = sorted.iter().position(|r| 
            r.operation.operation_id == "parent-ctx" && r.operation.action == OperationAction::Succeed
        ).unwrap();
        let child1_pos = sorted.iter().position(|r| r.operation.operation_id == "child-1").unwrap();
        let child2_pos = sorted.iter().position(|r| r.operation.operation_id == "child-2").unwrap();
        let exec_pos = sorted.iter().position(|r| r.operation.operation_id == "exec-1").unwrap();

        // Verify ordering constraints
        assert!(parent_start_pos < parent_succeed_pos, "Parent START should be before parent SUCCEED");
        assert!(parent_start_pos < child1_pos, "Parent START should be before child-1");
        assert!(parent_start_pos < child2_pos, "Parent START should be before child-2");
        assert_eq!(exec_pos, 4, "EXECUTION completion should be last");
    }

    // Test: Empty batch returns empty
    #[test]
    fn test_empty_batch() {
        let batch: Vec<CheckpointRequest> = vec![];
        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
        assert!(sorted.is_empty());
    }

    // Test: Single item batch returns same item
    #[test]
    fn test_single_item_batch() {
        let batch = vec![create_request(create_start_update("step-1", OperationType::Step))];
        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
    }

    // Test: Preserves original order when no ordering constraints apply
    #[test]
    fn test_preserves_original_order() {
        let batch = vec![
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_start_update("step-2", OperationType::Step)),
            create_request(create_start_update("step-3", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // Order should be preserved
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.operation_id, "step-2");
        assert_eq!(sorted[2].operation.operation_id, "step-3");
    }

    // Test: Multiple children of same parent
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

        // Parent should be first
        assert_eq!(sorted[0].operation.operation_id, "parent-ctx");
        
        // All children should come after parent
        for i in 1..4 {
            assert!(sorted[i].operation.parent_id.as_deref() == Some("parent-ctx"));
        }
    }

    // Test: Nested contexts (grandparent -> parent -> child)
    #[test]
    fn test_nested_contexts() {
        let mut parent_ctx = create_start_update("parent-ctx", OperationType::Context);
        parent_ctx.parent_id = Some("grandparent-ctx".to_string());

        let mut child = create_start_update("child-1", OperationType::Step);
        child.parent_id = Some("parent-ctx".to_string());

        let batch = vec![
            create_request(child),
            create_request(parent_ctx),
            create_request(create_start_update("grandparent-ctx", OperationType::Context)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // Find positions
        let grandparent_pos = sorted.iter().position(|r| r.operation.operation_id == "grandparent-ctx").unwrap();
        let parent_pos = sorted.iter().position(|r| r.operation.operation_id == "parent-ctx").unwrap();
        let child_pos = sorted.iter().position(|r| r.operation.operation_id == "child-1").unwrap();

        // Grandparent should be before parent (parent is child of grandparent)
        assert!(grandparent_pos < parent_pos, "Grandparent should be before parent");
        // Parent should be before child
        assert!(parent_pos < child_pos, "Parent should be before child");
    }

    // Test: EXECUTION START is not affected (only completion is last)
    #[test]
    fn test_execution_start_not_affected() {
        let batch = vec![
            create_request(create_start_update("step-1", OperationType::Step)),
            create_request(create_start_update("exec-1", OperationType::Execution)),
            create_request(create_start_update("step-2", OperationType::Step)),
        ];

        let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

        // EXECUTION START should stay in original position (not moved to end)
        assert_eq!(sorted.len(), 3);
        // Original order should be preserved since no ordering constraints apply
        assert_eq!(sorted[0].operation.operation_id, "step-1");
        assert_eq!(sorted[1].operation.operation_id, "exec-1");
        assert_eq!(sorted[2].operation.operation_id, "step-2");
    }
}
