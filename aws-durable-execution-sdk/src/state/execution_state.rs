//! Main execution state management.
//!
//! This module provides the [`ExecutionState`] struct which manages the execution
//! state for a durable execution, including checkpoint tracking, replay logic,
//! and operation state management.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use tokio::sync::{Mutex, RwLock};

use crate::client::SharedDurableServiceClient;
use crate::error::{DurableError, ErrorObject};
use crate::operation::{Operation, OperationStatus, OperationType, OperationUpdate};

use super::batcher::{
    CheckpointBatcher, CheckpointBatcherConfig, CheckpointSender, create_checkpoint_queue,
};
use super::checkpoint_result::CheckpointedResult;
use super::replay_status::ReplayStatus;

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
        
        // Find and extract the EXECUTION operation
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
    pub fn checkpointing_mode(&self) -> crate::config::CheckpointingMode {
        self.checkpointing_mode
    }
    
    /// Returns true if eager checkpointing mode is enabled.
    pub fn is_eager_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_eager()
    }
    
    /// Returns true if batched checkpointing mode is enabled.
    pub fn is_batched_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_batched()
    }
    
    /// Returns true if optimistic checkpointing mode is enabled.
    pub fn is_optimistic_checkpointing(&self) -> bool {
        self.checkpointing_mode.is_optimistic()
    }
    
    /// Returns a reference to the EXECUTION operation if it exists.
    ///
    /// # Requirements
    ///
    /// - 19.1: THE EXECUTION_Operation SHALL be recognized as the first operation in state
    pub fn execution_operation(&self) -> Option<&Operation> {
        self.execution_operation.as_ref()
    }
    
    /// Returns the original user input from the EXECUTION operation.
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
    pub fn execution_operation_id(&self) -> Option<&str> {
        self.execution_operation
            .as_ref()
            .map(|op| op.operation_id.as_str())
    }
    
    /// Completes the execution with a successful result via checkpointing.
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
    pub async fn get_checkpoint_result(&self, operation_id: &str) -> CheckpointedResult {
        let operations = self.operations.read().await;
        CheckpointedResult::new(operations.get(operation_id).cloned())
    }
    
    /// Tracks that an operation has been replayed.
    pub async fn track_replay(&self, operation_id: &str) {
        {
            let mut replayed = self.replayed_operations.write().await;
            replayed.insert(operation_id.to_string());
        }
        
        let (replayed_count, total_count) = {
            let replayed = self.replayed_operations.read().await;
            let operations = self.operations.read().await;
            (replayed.len(), operations.len())
        };
        
        if replayed_count >= total_count {
            let has_more = self.next_marker.read().await.is_some();
            if !has_more {
                self.replay_status.store(ReplayStatus::New as u8, Ordering::SeqCst);
            }
        }
    }
    
    /// Loads additional operations from the service for pagination.
    ///
    /// # Requirements
    ///
    /// - 18.5: THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
    pub async fn load_more_operations(&self) -> Result<bool, DurableError> {
        let marker = {
            let guard = self.next_marker.read().await;
            match guard.as_ref() {
                Some(m) => m.clone(),
                None => return Ok(false),
            }
        };
        
        let response = self.get_operations_with_retry(&marker).await?;
        
        {
            let mut operations = self.operations.write().await;
            for op in response.operations {
                operations.insert(op.operation_id.clone(), op);
            }
        }
        
        {
            let mut next_marker = self.next_marker.write().await;
            *next_marker = response.next_marker;
        }
        
        Ok(true)
    }
    
    /// Fetches operations from the service with retry for throttling errors.
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
                        tracing::warn!(attempt = attempt, "GetOperations throttling: max retries exceeded");
                        return Err(error);
                    }

                    let actual_delay = error.get_retry_after_ms().unwrap_or(delay_ms);
                    tracing::debug!(attempt = attempt, delay_ms = actual_delay, "GetOperations throttled, retrying");
                    tokio::time::sleep(StdDuration::from_millis(actual_delay)).await;
                    delay_ms = (delay_ms * BACKOFF_MULTIPLIER).min(MAX_DELAY_MS);
                }
                Err(error) => return Err(error),
            }
        }
    }
    
    /// Loads all remaining operations from the service.
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
    pub async fn mark_parent_done(&self, parent_id: &str) {
        let mut done_parents = self.parent_done_lock.lock().await;
        done_parents.insert(parent_id.to_string());
    }
    
    /// Checks if a parent operation has been marked as done.
    pub async fn is_parent_done(&self, parent_id: &str) -> bool {
        let done_parents = self.parent_done_lock.lock().await;
        done_parents.contains(parent_id)
    }
    
    /// Checks if an operation would be orphaned.
    pub async fn is_orphaned(&self, parent_id: Option<&str>) -> bool {
        match parent_id {
            Some(pid) => self.is_parent_done(pid).await,
            None => false,
        }
    }
    
    /// Creates a checkpoint for an operation.
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
        let effective_is_sync = match self.checkpointing_mode {
            crate::config::CheckpointingMode::Eager => true,
            crate::config::CheckpointingMode::Batched => is_sync,
            crate::config::CheckpointingMode::Optimistic => is_sync,
        };
        
        // In Eager mode, bypass the batcher and send directly
        if self.checkpointing_mode.is_eager() {
            return self.checkpoint_direct(operation, effective_is_sync).await;
        }
        
        // Use the checkpoint sender if available (batched mode)
        if let Some(ref sender) = self.checkpoint_sender {
            let result = sender.checkpoint(operation.clone(), effective_is_sync).await;
            if result.is_ok() {
                self.update_local_cache_from_update(&operation).await;
            }
            return result;
        }
        
        // Direct checkpoint (non-batched mode)
        self.checkpoint_direct(operation, effective_is_sync).await
    }
    
    /// Sends a checkpoint directly to the service, bypassing the batcher.
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
        
        {
            let mut token_guard = self.checkpoint_token.write().await;
            *token_guard = response.checkpoint_token;
        }
        
        self.update_local_cache_from_update(&operation).await;
        Ok(())
    }
    
    /// Creates a synchronous checkpoint (waits for confirmation).
    pub async fn checkpoint_sync(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        self.create_checkpoint(operation, true).await
    }
    
    /// Creates an asynchronous checkpoint (fire-and-forget).
    pub async fn checkpoint_async(&self, operation: OperationUpdate) -> Result<(), DurableError> {
        self.create_checkpoint(operation, false).await
    }
    
    /// Creates a checkpoint using the optimal sync behavior for the current mode.
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
    pub fn should_use_async_checkpoint(&self) -> bool {
        match self.checkpointing_mode {
            crate::config::CheckpointingMode::Eager => false,
            crate::config::CheckpointingMode::Batched => true,
            crate::config::CheckpointingMode::Optimistic => true,
        }
    }
    
    /// Returns whether the current mode prioritizes performance over durability.
    pub fn prioritizes_performance(&self) -> bool {
        self.checkpointing_mode.is_optimistic()
    }
    
    /// Returns whether the current mode prioritizes durability over performance.
    pub fn prioritizes_durability(&self) -> bool {
        self.checkpointing_mode.is_eager()
    }
    
    /// Updates the local operation cache based on an operation update.
    async fn update_local_cache_from_update(&self, update: &OperationUpdate) {
        let mut operations = self.operations.write().await;
        
        match update.action {
            crate::operation::OperationAction::Start => {
                let mut op = Operation::new(&update.operation_id, update.operation_type);
                op.parent_id = update.parent_id.clone();
                op.name = update.name.clone();
                operations.insert(update.operation_id.clone(), op);
            }
            crate::operation::OperationAction::Succeed => {
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Succeeded;
                    op.result = update.result.clone();
                } else {
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Succeeded;
                    op.result = update.result.clone();
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Fail => {
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Failed;
                    op.error = update.error.clone();
                } else {
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Failed;
                    op.error = update.error.clone();
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Cancel => {
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Cancelled;
                } else {
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Cancelled;
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    operations.insert(update.operation_id.clone(), op);
                }
            }
            crate::operation::OperationAction::Retry => {
                if let Some(op) = operations.get_mut(&update.operation_id) {
                    op.status = OperationStatus::Pending;
                    if update.result.is_some() || update.step_options.is_some() {
                        let step_details = op.step_details.get_or_insert_with(|| crate::operation::StepDetails {
                            result: None,
                            attempt: None,
                            next_attempt_timestamp: None,
                            error: None,
                            payload: None,
                        });
                        if update.result.is_some() {
                            step_details.payload = update.result.clone();
                        }
                        step_details.attempt = Some(step_details.attempt.unwrap_or(0) + 1);
                    }
                    if update.error.is_some() {
                        op.error = update.error.clone();
                    }
                } else {
                    let mut op = Operation::new(&update.operation_id, update.operation_type);
                    op.status = OperationStatus::Pending;
                    op.parent_id = update.parent_id.clone();
                    op.name = update.name.clone();
                    op.error = update.error.clone();
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
    pub fn shared_checkpoint_token(&self) -> Arc<RwLock<String>> {
        self.checkpoint_token.clone()
    }
    
    /// Returns true if this ExecutionState has a checkpoint sender configured.
    pub fn has_checkpoint_sender(&self) -> bool {
        self.checkpoint_sender.is_some()
    }

    /// Loads child operations for a specific parent operation.
    ///
    /// # Requirements
    ///
    /// - 10.5: THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
    /// - 10.6: WHEN ReplayChildren is true, THE Child_Context_Operation SHALL include child operations in state loads for replay
    pub async fn load_child_operations(&self, parent_id: &str) -> Result<Vec<Operation>, DurableError> {
        let operations = self.operations.read().await;
        let children: Vec<Operation> = operations
            .values()
            .filter(|op| op.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect();
        
        Ok(children)
    }

    /// Gets all child operations for a specific parent from the local cache.
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
