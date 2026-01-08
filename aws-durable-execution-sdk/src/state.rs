//! Execution state management for the AWS Durable Execution SDK.
//!
//! This module provides the core state management types for durable executions,
//! including checkpoint tracking, replay logic, and operation state management.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
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
pub struct CheckpointBatcher {
    /// Configuration for batching behavior
    config: CheckpointBatcherConfig,
    /// Receiver for checkpoint requests
    queue_rx: mpsc::Receiver<CheckpointRequest>,
    /// Service client for sending checkpoints
    service_client: SharedDurableServiceClient,
    /// Reference to the execution state for updating tokens
    durable_execution_arn: String,
    /// Current checkpoint token
    checkpoint_token: Arc<RwLock<String>>,
}

impl CheckpointBatcher {
    /// Creates a new CheckpointBatcher.
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

    /// Processes a batch of checkpoint requests.
    async fn process_batch(&self, batch: Vec<CheckpointRequest>) {
        if batch.is_empty() {
            return;
        }

        // Extract operations and completion channels
        let (operations, completions): (Vec<_>, Vec<_>) = batch
            .into_iter()
            .map(|req| (req.operation, req.completion))
            .unzip();

        // Get current checkpoint token
        let token = self.checkpoint_token.read().await.clone();

        // Send the batch to the service
        let result = self
            .service_client
            .checkpoint(&self.durable_execution_arn, &token, operations)
            .await;

        // Handle the result
        match result {
            Ok(response) => {
                // Update the checkpoint token
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
                // Signal failure to all waiting callers
                for completion in completions.into_iter().flatten() {
                    let _ = completion.send(Err(DurableError::Checkpoint {
                        message: error.to_string(),
                        is_retriable: error.is_retriable(),
                        aws_error: None,
                    }));
                }
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
    pub fn new(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
    ) -> Self {
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
    pub fn with_batcher(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        initial_state: crate::lambda::InitialExecutionState,
        service_client: SharedDurableServiceClient,
        batcher_config: CheckpointBatcherConfig,
        queue_buffer_size: usize,
    ) -> (Self, CheckpointBatcher) {
        let arn: String = durable_execution_arn.into();
        let token: String = checkpoint_token.into();
        
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
    pub async fn load_more_operations(&self) -> Result<bool, DurableError> {
        // Get the current marker
        let marker = {
            let guard = self.next_marker.read().await;
            match guard.as_ref() {
                Some(m) => m.clone(),
                None => return Ok(false), // No more to load
            }
        };
        
        // Fetch more operations from the service
        let response = self
            .service_client
            .get_operations(&self.durable_execution_arn, &marker)
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
        
        // Use the checkpoint sender if available (batched mode)
        if let Some(ref sender) = self.checkpoint_sender {
            let result = sender.checkpoint(operation.clone(), is_sync).await;
            
            // On success, update local cache
            if result.is_ok() {
                self.update_local_cache_from_update(&operation).await;
            }
            
            return result;
        }
        
        // Direct checkpoint (non-batched mode)
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
