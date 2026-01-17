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
use crate::types::ExecutionArn;

use super::batcher::{
    create_checkpoint_queue, CheckpointBatcher, CheckpointBatcherConfig, CheckpointSender,
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

    /// Returns the durable execution ARN as an `ExecutionArn` newtype.
    #[inline]
    pub fn durable_execution_arn_typed(&self) -> ExecutionArn {
        ExecutionArn::from(self.durable_execution_arn.clone())
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
    ///
    /// # Memory Ordering
    ///
    /// Uses `Ordering::Acquire` to ensure that when we read the replay status,
    /// we also see all the operations that were replayed before the status was
    /// set to `New`. This creates a happens-before relationship with the
    /// `Release` store in `track_replay`.
    ///
    /// Requirements: 4.2, 4.3, 4.6
    pub fn replay_status(&self) -> ReplayStatus {
        // Acquire ordering ensures we see all writes that happened before
        // the corresponding Release store that set this value.
        ReplayStatus::from(self.replay_status.load(Ordering::Acquire))
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
    pub async fn complete_execution_success(
        &self,
        result: Option<String>,
    ) -> Result<(), DurableError> {
        let execution_id =
            self.execution_operation_id()
                .ok_or_else(|| DurableError::Validation {
                    message: "Cannot complete execution: no EXECUTION operation exists".to_string(),
                })?;

        let update = OperationUpdate::succeed(execution_id, OperationType::Execution, result);

        self.create_checkpoint(update, true).await
    }

    /// Completes the execution with a failure via checkpointing.
    ///
    /// # Requirements
    ///
    /// - 19.4: THE EXECUTION_Operation SHALL support completing execution via FAIL action with error
    pub async fn complete_execution_failure(&self, error: ErrorObject) -> Result<(), DurableError> {
        let execution_id =
            self.execution_operation_id()
                .ok_or_else(|| DurableError::Validation {
                    message: "Cannot complete execution: no EXECUTION operation exists".to_string(),
                })?;

        let update = OperationUpdate::fail(execution_id, OperationType::Execution, error);

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
                // Release ordering ensures that all the replay tracking writes
                // (replayed_operations updates) are visible to any thread that
                // subsequently reads the replay_status with Acquire ordering.
                // This establishes a happens-before relationship.
                //
                // Requirements: 4.2, 4.3, 4.6
                self.replay_status
                    .store(ReplayStatus::New as u8, Ordering::Release);
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
                        tracing::warn!(
                            attempt = attempt,
                            "GetOperations throttling: max retries exceeded"
                        );
                        return Err(error);
                    }

                    let actual_delay = error.get_retry_after_ms().unwrap_or(delay_ms);
                    tracing::debug!(
                        attempt = attempt,
                        delay_ms = actual_delay,
                        "GetOperations throttled, retrying"
                    );
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
    pub async fn update_operation(
        &self,
        operation_id: &str,
        update_fn: impl FnOnce(&mut Operation),
    ) {
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
            let result = sender
                .checkpoint(operation.clone(), effective_is_sync)
                .await;
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

        // Update local cache from the response's NewExecutionState if available
        if let Some(ref new_state) = response.new_execution_state {
            self.update_local_cache_from_response(new_state).await;
        } else {
            self.update_local_cache_from_update(&operation).await;
        }
        Ok(())
    }

    /// Creates a checkpoint and returns the full response including NewExecutionState.
    /// This is useful for operations like CALLBACK that need service-generated values.
    pub async fn create_checkpoint_with_response(
        &self,
        operation: OperationUpdate,
    ) -> Result<crate::client::CheckpointResponse, DurableError> {
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

        let token = self.checkpoint_token.read().await.clone();
        let response = self
            .service_client
            .checkpoint(&self.durable_execution_arn, &token, vec![operation.clone()])
            .await?;

        tracing::debug!(
            has_new_state = response.new_execution_state.is_some(),
            num_operations = response
                .new_execution_state
                .as_ref()
                .map(|s| s.operations.len())
                .unwrap_or(0),
            "Checkpoint response received"
        );

        {
            let mut token_guard = self.checkpoint_token.write().await;
            *token_guard = response.checkpoint_token.clone();
        }

        // Update local cache from the response's NewExecutionState if available
        if let Some(ref new_state) = response.new_execution_state {
            self.update_local_cache_from_response(new_state).await;
        } else {
            self.update_local_cache_from_update(&operation).await;
        }

        Ok(response)
    }

    /// Updates the local operation cache from a checkpoint response's NewExecutionState.
    async fn update_local_cache_from_response(&self, new_state: &crate::client::NewExecutionState) {
        let mut operations = self.operations.write().await;
        for op in &new_state.operations {
            operations.insert(op.operation_id.clone(), op.clone());
        }
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
                        let step_details =
                            op.step_details
                                .get_or_insert(crate::operation::StepDetails {
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
    pub async fn load_child_operations(
        &self,
        parent_id: &str,
    ) -> Result<Vec<Operation>, DurableError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::client::{
        CheckpointResponse, GetOperationsResponse, MockDurableServiceClient,
        SharedDurableServiceClient,
    };
    use crate::error::ErrorObject;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{
        ContextDetails, ExecutionDetails, Operation, OperationStatus, OperationType,
        OperationUpdate,
    };

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(MockDurableServiceClient::new())
    }

    fn create_test_operation(id: &str, status: OperationStatus) -> Operation {
        let mut op = Operation::new(id, OperationType::Step);
        op.status = status;
        op
    }

    fn create_execution_operation(input_payload: Option<&str>) -> Operation {
        let mut op = Operation::new("exec-123", OperationType::Execution);
        op.status = OperationStatus::Started;
        op.execution_details = Some(ExecutionDetails {
            input_payload: input_payload.map(|s| s.to_string()),
        });
        op
    }

    // Basic ExecutionState tests

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

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

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

        assert!(state.is_replay());
        state.track_replay("op-1").await;
        assert!(state.is_replay());
        state.track_replay("op-2").await;
        assert!(state.is_new());
    }

    #[tokio::test]
    async fn test_track_replay_with_pagination() {
        let client = Arc::new(
            MockDurableServiceClient::new().with_get_operations_response(Ok(
                GetOperationsResponse {
                    operations: vec![create_test_operation("op-3", OperationStatus::Succeeded)],
                    next_marker: None,
                },
            )),
        );

        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        state.track_replay("op-1").await;
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

        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Started);

        state
            .update_operation("op-1", |op| {
                op.status = OperationStatus::Succeeded;
                op.result = Some("done".to_string());
            })
            .await;

        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some("done".to_string()));
    }

    #[tokio::test]
    async fn test_load_more_operations() {
        let client = Arc::new(
            MockDurableServiceClient::new().with_get_operations_response(Ok(
                GetOperationsResponse {
                    operations: vec![
                        create_test_operation("op-2", OperationStatus::Succeeded),
                        create_test_operation("op-3", OperationStatus::Succeeded),
                    ],
                    next_marker: None,
                },
            )),
        );

        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        assert_eq!(state.operation_count().await, 1);
        assert!(state.has_more_operations().await);

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
                })),
        );

        let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
        let mut initial_state = InitialExecutionState::with_operations(ops);
        initial_state.next_marker = Some("marker-1".to_string());

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        assert_eq!(state.operation_count().await, 1);
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

        assert!(!state.is_orphaned(None).await);
        assert!(!state.is_orphaned(Some("parent-1")).await);

        state.mark_parent_done("parent-1").await;

        assert!(state.is_orphaned(Some("parent-1")).await);
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

    #[tokio::test]
    async fn test_create_checkpoint_direct() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
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
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-2"))),
        );

        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        let update = OperationUpdate::start("op-1", OperationType::Step);
        state.create_checkpoint(update, true).await.unwrap();

        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Started);

        let update = OperationUpdate::succeed(
            "op-1",
            OperationType::Step,
            Some(r#"{"result": "ok"}"#.to_string()),
        );
        state.create_checkpoint(update, true).await.unwrap();

        let op = state.get_operation("op-1").await.unwrap();
        assert_eq!(op.status, OperationStatus::Succeeded);
        assert_eq!(op.result, Some(r#"{"result": "ok"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_create_checkpoint_rejects_orphaned_child() {
        let client = create_mock_client();
        let state = ExecutionState::new(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
        );

        state.mark_parent_done("parent-1").await;

        let update =
            OperationUpdate::start("child-1", OperationType::Step).with_parent_id("parent-1");
        let result = state.create_checkpoint(update, true).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::DurableError::OrphanedChild { operation_id, .. } => {
                assert_eq!(operation_id, "child-1");
            }
            _ => panic!("Expected OrphanedChild error"),
        }
    }

    #[tokio::test]
    async fn test_with_batcher_creates_state_and_batcher() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
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

        let batcher_handle = tokio::spawn(async move {
            batcher.run().await;
        });

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.create_checkpoint(update, true).await;

        drop(state);
        batcher_handle.await.unwrap();

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_checkpoint_sync_convenience_method() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
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
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
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

        {
            let mut guard = shared_token.write().await;
            *guard = "modified-token".to_string();
        }

        assert_eq!(state.checkpoint_token().await, "modified-token");
    }

    // Tests for ReplayChildren support (Requirements 10.5, 10.6)

    #[tokio::test]
    async fn test_load_child_operations_returns_children() {
        let client = create_mock_client();

        let mut parent_op = Operation::new("parent-ctx", OperationType::Context);
        parent_op.status = OperationStatus::Succeeded;

        let mut child1 = create_test_operation("child-1", OperationStatus::Succeeded);
        child1.parent_id = Some("parent-ctx".to_string());

        let mut child2 = create_test_operation("child-2", OperationStatus::Succeeded);
        child2.parent_id = Some("parent-ctx".to_string());

        let mut other_child = create_test_operation("other-child", OperationStatus::Succeeded);
        other_child.parent_id = Some("other-parent".to_string());

        let initial_state =
            InitialExecutionState::with_operations(vec![parent_op, child1, child2, other_child]);

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

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

        let initial_state = InitialExecutionState::with_operations(vec![parent_op, child1, child2]);

        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

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

        assert_eq!(
            state.checkpointing_mode(),
            crate::config::CheckpointingMode::Batched
        );
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

        assert_eq!(
            state.checkpointing_mode(),
            crate::config::CheckpointingMode::Eager
        );
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

        assert_eq!(
            state.checkpointing_mode(),
            crate::config::CheckpointingMode::Optimistic
        );
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
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

        let state = ExecutionState::with_checkpointing_mode(
            "arn:test",
            "token-123",
            InitialExecutionState::new(),
            client,
            crate::config::CheckpointingMode::Eager,
        );

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.checkpoint_optimal(update, false).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_eager_mode_bypasses_batcher() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

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

        let batcher_handle = tokio::spawn(async move {
            batcher.run().await;
        });

        let update = OperationUpdate::start("op-1", OperationType::Step);
        let result = state.create_checkpoint(update, false).await;

        drop(state);
        batcher_handle.await.unwrap();

        assert!(result.is_ok());
    }

    // Tests for EXECUTION operation handling (Requirements 19.1-19.5)

    #[tokio::test]
    async fn test_execution_operation_recognized() {
        let client = create_mock_client();
        let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
        let step_op = Operation::new("step-1", OperationType::Step);

        let initial_state = InitialExecutionState::with_operations(vec![exec_op, step_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

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
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

        let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
        let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        let result = state
            .complete_execution_success(Some(r#"{"status": "completed"}"#.to_string()))
            .await;
        assert!(result.is_ok());

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

        let result = state
            .complete_execution_success(Some("result".to_string()))
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::DurableError::Validation { message } => {
                assert!(message.contains("no EXECUTION operation"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[tokio::test]
    async fn test_complete_execution_failure() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("new-token"))),
        );

        let exec_op = create_execution_operation(Some(r#"{"order_id": "123"}"#));
        let initial_state = InitialExecutionState::with_operations(vec![exec_op]);
        let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

        let error = ErrorObject::new("ProcessingError", "Order processing failed");
        let result = state.complete_execution_failure(error).await;
        assert!(result.is_ok());

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
            crate::error::DurableError::Validation { message } => {
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

        assert!(state.execution_operation().is_some());
        let exec = state.execution_operation().unwrap();
        assert_eq!(exec.operation_type, OperationType::Execution);
        assert_eq!(
            state.get_original_input_raw(),
            Some(r#"{"order_id": "123"}"#)
        );
    }

    // Property-based tests for orphan prevention

    mod property_tests {
        use super::*;
        use proptest::prelude::*;

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
                            .with_checkpoint_response(Ok(CheckpointResponse::new("new-token")))
                    );

                    let state = ExecutionState::new(
                        "arn:test",
                        "token-123",
                        InitialExecutionState::new(),
                        client,
                    );

                    state.mark_parent_done(&parent_id).await;

                    let update = OperationUpdate::start(&child_id, OperationType::Step)
                        .with_parent_id(&parent_id);
                    let checkpoint_result = state.create_checkpoint(update, true).await;

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
                            .with_checkpoint_response(Ok(CheckpointResponse::new("new-token")))
                    );

                    let state = ExecutionState::new(
                        "arn:test",
                        "token-123",
                        InitialExecutionState::new(),
                        client,
                    );

                    let update = OperationUpdate::start(&child_id, OperationType::Step)
                        .with_parent_id(&parent_id);
                    let checkpoint_result = state.create_checkpoint(update, true).await;

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
                            .with_checkpoint_response(Ok(CheckpointResponse::new("new-token")))
                    );

                    let state = ExecutionState::new(
                        "arn:test",
                        "token-123",
                        InitialExecutionState::new(),
                        client,
                    );

                    let update = OperationUpdate::start(&operation_id, OperationType::Step);
                    let checkpoint_result = state.create_checkpoint(update, true).await;

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
                            .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                            .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
                    );

                    let state = ExecutionState::new(
                        "arn:test",
                        "token-123",
                        InitialExecutionState::new(),
                        client,
                    );

                    let update_before = OperationUpdate::start(&child_id_before, OperationType::Step)
                        .with_parent_id(&parent_id);
                    let result_before = state.create_checkpoint(update_before, true).await;

                    if let Err(e) = result_before {
                        return Err(TestCaseError::fail(format!(
                            "Expected first checkpoint to succeed, got error: {:?}",
                            e
                        )));
                    }

                    state.mark_parent_done(&parent_id).await;

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
}
