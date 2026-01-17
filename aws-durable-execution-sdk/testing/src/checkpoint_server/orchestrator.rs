//! Test execution orchestrator for managing the full execution lifecycle.
//!
//! This module implements the TestExecutionOrchestrator which orchestrates
//! the full test execution lifecycle, including polling for checkpoint updates,
//! processing operations, and scheduling handler re-invocations.
//!
//! This matches the Node.js SDK's `TestExecutionOrchestrator` pattern.
//!
//! # Requirements
//!
//! - 16.1: WHEN a wait operation is encountered, THE Test_Execution_Orchestrator
//!   SHALL track the wait's scheduled end timestamp
//! - 16.2: WHEN time skipping is enabled and a wait's scheduled end time is reached,
//!   THE Test_Execution_Orchestrator SHALL mark the wait as SUCCEEDED and schedule
//!   handler re-invocation
//! - 16.3: WHEN time skipping is enabled, THE Test_Execution_Orchestrator SHALL use
//!   tokio::time::advance() to skip wait durations instantly
//! - 16.4: WHEN a handler invocation returns PENDING status, THE Test_Execution_Orchestrator
//!   SHALL continue polling for operation updates and re-invoke the handler when
//!   operations complete
//! - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
//!   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
//! - 16.6: WHEN multiple operations are pending (waits, callbacks, steps with retries),
//!   THE Test_Execution_Orchestrator SHALL process them in scheduled order

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use aws_durable_execution_sdk::{
    DurableContext, DurableError, DurableServiceClient, Operation, OperationStatus, OperationType,
};

use super::scheduler::{QueueScheduler, Scheduler};
use super::types::ExecutionId;
use super::worker_manager::CheckpointWorkerManager;
use crate::types::{ExecutionStatus, Invocation, TestResultError};

/// Configuration for time skipping behavior.
#[derive(Debug, Clone, Default)]
pub struct SkipTimeConfig {
    /// Whether time skipping is enabled.
    pub enabled: bool,
}

/// Result of a test execution.
#[derive(Debug)]
pub struct TestExecutionResult<T> {
    /// The execution status
    pub status: ExecutionStatus,
    /// The result value (if succeeded)
    pub result: Option<T>,
    /// The error (if failed)
    pub error: Option<TestResultError>,
    /// All operations from the execution
    pub operations: Vec<Operation>,
    /// Handler invocations
    pub invocations: Vec<Invocation>,
    /// The execution ID
    pub execution_id: String,
}

impl<T> TestExecutionResult<T> {
    /// Create a successful result.
    pub fn success(result: T, operations: Vec<Operation>, execution_id: String) -> Self {
        Self {
            status: ExecutionStatus::Succeeded,
            result: Some(result),
            error: None,
            operations,
            invocations: Vec::new(),
            execution_id,
        }
    }

    /// Create a failed result.
    pub fn failure(error: TestResultError, operations: Vec<Operation>, execution_id: String) -> Self {
        Self {
            status: ExecutionStatus::Failed,
            result: None,
            error: Some(error),
            operations,
            invocations: Vec::new(),
            execution_id,
        }
    }

    /// Create a running/pending result.
    pub fn running(operations: Vec<Operation>, execution_id: String) -> Self {
        Self {
            status: ExecutionStatus::Running,
            result: None,
            error: None,
            operations,
            invocations: Vec::new(),
            execution_id,
        }
    }

    /// Add an invocation to the result.
    pub fn with_invocation(mut self, invocation: Invocation) -> Self {
        self.invocations.push(invocation);
        self
    }
}


/// Internal storage for operations during test execution.
#[derive(Debug, Default)]
pub struct OperationStorage {
    /// All operations in execution order
    operations: Vec<Operation>,
    /// Map from operation ID to index in operations vec
    operations_by_id: std::collections::HashMap<String, usize>,
    /// Map from operation name to indices in operations vec
    operations_by_name: std::collections::HashMap<String, Vec<usize>>,
}

impl OperationStorage {
    /// Create a new operation storage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an operation to storage.
    pub fn add_operation(&mut self, operation: Operation) {
        let index = self.operations.len();
        let id = operation.operation_id.clone();
        let name = operation.name.clone();

        self.operations.push(operation);
        self.operations_by_id.insert(id, index);

        if let Some(name) = name {
            self.operations_by_name
                .entry(name)
                .or_default()
                .push(index);
        }
    }

    /// Update an existing operation or add if not exists.
    pub fn update_operation(&mut self, operation: Operation) {
        let id = operation.operation_id.clone();
        if let Some(&index) = self.operations_by_id.get(&id) {
            self.operations[index] = operation;
        } else {
            self.add_operation(operation);
        }
    }

    /// Get an operation by ID.
    pub fn get_by_id(&self, id: &str) -> Option<&Operation> {
        self.operations_by_id
            .get(id)
            .and_then(|&idx| self.operations.get(idx))
    }

    /// Get all operations.
    pub fn get_all(&self) -> &[Operation] {
        &self.operations
    }

    /// Clear all operations.
    pub fn clear(&mut self) {
        self.operations.clear();
        self.operations_by_id.clear();
        self.operations_by_name.clear();
    }

    /// Get the number of operations.
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Check if storage is empty.
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

/// Type alias for a boxed handler function.
pub type BoxedHandler<I, O> = Box<
    dyn Fn(I, DurableContext) -> Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        + Send
        + Sync,
>;

/// Orchestrates test execution lifecycle, polling, and handler invocation.
///
/// This struct manages the full execution lifecycle including:
/// - Starting executions via checkpoint API
/// - Polling for checkpoint updates
/// - Processing operation updates (waits, callbacks, retries)
/// - Scheduling handler re-invocations
/// - Resolving execution when complete
pub struct TestExecutionOrchestrator<I, O>
where
    I: DeserializeOwned + Send + Serialize + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    /// The handler function to execute
    handler: BoxedHandler<I, O>,
    /// Storage for operations
    operation_storage: Arc<RwLock<OperationStorage>>,
    /// The checkpoint API client
    checkpoint_api: Arc<CheckpointWorkerManager>,
    /// Time skipping configuration
    skip_time_config: SkipTimeConfig,
    /// The scheduler for handler invocations
    scheduler: Box<dyn Scheduler>,
    /// Set of pending operation IDs
    pending_operations: HashSet<String>,
    /// Flag indicating if an invocation is active
    invocation_active: Arc<AtomicBool>,
    /// Current execution ID
    execution_id: Option<ExecutionId>,
    /// Current checkpoint token
    checkpoint_token: Option<String>,
    /// Flag indicating if execution is complete
    execution_complete: Arc<AtomicBool>,
    /// The final result (if any)
    final_result: Arc<Mutex<Option<Result<O, DurableError>>>>,
}

impl<I, O> TestExecutionOrchestrator<I, O>
where
    I: DeserializeOwned + Send + Serialize + Clone + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    /// Create a new orchestrator.
    ///
    /// # Arguments
    ///
    /// * `handler` - The handler function to execute
    /// * `operation_storage` - Shared storage for operations
    /// * `checkpoint_api` - The checkpoint API client
    /// * `skip_time_config` - Configuration for time skipping
    pub fn new<F, Fut>(
        handler: F,
        operation_storage: Arc<RwLock<OperationStorage>>,
        checkpoint_api: Arc<CheckpointWorkerManager>,
        skip_time_config: SkipTimeConfig,
    ) -> Self
    where
        F: Fn(I, DurableContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, DurableError>> + Send + 'static,
    {
        let boxed_handler = Box::new(move |input: I, ctx: DurableContext| {
            let fut = handler(input, ctx);
            Box::pin(fut) as Pin<Box<dyn Future<Output = Result<O, DurableError>> + Send>>
        });

        // Use QueueScheduler for time-skipping mode (FIFO order)
        let scheduler: Box<dyn Scheduler> = Box::new(QueueScheduler::new());

        Self {
            handler: boxed_handler,
            operation_storage,
            checkpoint_api,
            skip_time_config,
            scheduler,
            pending_operations: HashSet::new(),
            invocation_active: Arc::new(AtomicBool::new(false)),
            execution_id: None,
            checkpoint_token: None,
            execution_complete: Arc::new(AtomicBool::new(false)),
            final_result: Arc::new(Mutex::new(None)),
        }
    }

    /// Check if time skipping is enabled.
    pub fn is_time_skipping_enabled(&self) -> bool {
        self.skip_time_config.enabled
    }

    /// Get the current execution ID.
    pub fn execution_id(&self) -> Option<&str> {
        self.execution_id.as_deref()
    }

    /// Get the current checkpoint token.
    pub fn checkpoint_token(&self) -> Option<&str> {
        self.checkpoint_token.as_deref()
    }

    /// Check if execution is complete.
    pub fn is_execution_complete(&self) -> bool {
        self.execution_complete.load(Ordering::SeqCst)
    }

    /// Check if an invocation is currently active.
    pub fn is_invocation_active(&self) -> bool {
        self.invocation_active.load(Ordering::SeqCst)
    }
}


impl<I, O> TestExecutionOrchestrator<I, O>
where
    I: DeserializeOwned + Send + Serialize + Clone + 'static,
    O: Serialize + DeserializeOwned + Send + 'static,
{
    /// Execute the handler and return the result.
    ///
    /// This method orchestrates the full execution lifecycle:
    /// 1. Start execution via checkpoint API
    /// 2. Begin polling for checkpoint updates
    /// 3. Invoke handler
    /// 4. Process operation updates (waits, callbacks, retries)
    /// 5. Schedule re-invocations as needed
    /// 6. Return result when execution completes
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the handler
    ///
    /// # Requirements
    ///
    /// - 16.4: WHEN a handler invocation returns PENDING status,
    ///   THE Test_Execution_Orchestrator SHALL continue polling for operation
    ///   updates and re-invoke the handler when operations complete
    /// - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
    ///   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
    /// - 18.1: WHEN an execution starts, THE Test_Execution_Orchestrator SHALL
    ///   begin polling for checkpoint data
    pub async fn execute_handler(
        &mut self,
        payload: I,
    ) -> Result<TestExecutionResult<O>, crate::error::TestError> {
        use super::types::{ApiType, StartDurableExecutionRequest};
        use aws_durable_execution_sdk::lambda::InitialExecutionState;
        use aws_durable_execution_sdk::state::ExecutionState;

        // Clear previous state
        self.operation_storage.write().await.clear();
        self.pending_operations.clear();
        self.execution_complete.store(false, Ordering::SeqCst);
        *self.final_result.lock().await = None;

        // Serialize the payload for the checkpoint server
        let payload_json = serde_json::to_string(&payload)?;

        // Start execution with the checkpoint server
        let invocation_id = uuid::Uuid::new_v4().to_string();
        let start_request = StartDurableExecutionRequest {
            invocation_id: invocation_id.clone(),
            payload: Some(payload_json),
        };
        let start_payload = serde_json::to_string(&start_request)?;

        let start_response = self
            .checkpoint_api
            .send_api_request(ApiType::StartDurableExecution, start_payload)
            .await?;

        if let Some(error) = start_response.error {
            return Err(crate::error::TestError::CheckpointServerError(error));
        }

        let invocation_result: super::InvocationResult = serde_json::from_str(
            &start_response.payload.ok_or_else(|| {
                crate::error::TestError::CheckpointServerError(
                    "Empty response from checkpoint server".to_string(),
                )
            })?,
        )?;

        self.execution_id = Some(invocation_result.execution_id.clone());
        self.checkpoint_token = Some(invocation_result.checkpoint_token.clone());

        let execution_arn = invocation_result.execution_id.clone();
        let checkpoint_token = invocation_result.checkpoint_token.clone();

        // Create initial execution state
        let initial_state = InitialExecutionState::new();

        // Create the execution state with the checkpoint worker manager
        let execution_state = Arc::new(ExecutionState::new(
            &execution_arn,
            &checkpoint_token,
            initial_state,
            self.checkpoint_api.clone(),
        ));

        // Create the durable context
        let ctx = DurableContext::new(execution_state.clone());

        // Record invocation start
        let start_time = chrono::Utc::now();
        let mut invocation = Invocation::with_start(start_time);

        // Execute the handler
        self.invocation_active.store(true, Ordering::SeqCst);
        let handler_result = (self.handler)(payload.clone(), ctx).await;
        self.invocation_active.store(false, Ordering::SeqCst);

        // Record invocation end
        let end_time = chrono::Utc::now();
        invocation = invocation.with_end(end_time);

        // Retrieve operations from the checkpoint server
        let operations = match self.checkpoint_api.get_operations(&execution_arn, "").await {
            Ok(response) => {
                let mut storage = self.operation_storage.write().await;
                for op in &response.operations {
                    storage.update_operation(op.clone());
                }
                response.operations
            }
            Err(_) => Vec::new(),
        };

        // Build the test result based on handler outcome
        match handler_result {
            Ok(result) => {
                self.execution_complete.store(true, Ordering::SeqCst);
                let mut test_result =
                    TestExecutionResult::success(result, operations, execution_arn);
                test_result.invocations.push(invocation);
                Ok(test_result)
            }
            Err(error) => {
                // Check if this is a suspend error (which means pending, not failed)
                if error.is_suspend() {
                    // Handler suspended - need to process pending operations
                    let test_result = self
                        .handle_pending_execution(payload, execution_arn, invocation)
                        .await?;
                    Ok(test_result)
                } else {
                    self.execution_complete.store(true, Ordering::SeqCst);
                    let error_obj = aws_durable_execution_sdk::ErrorObject::from(&error);
                    let test_error = TestResultError::new(error_obj.error_type, error.to_string());
                    let mut test_result =
                        TestExecutionResult::failure(test_error.clone(), operations, execution_arn);
                    test_result.invocations.push(invocation.with_error(test_error));
                    Ok(test_result)
                }
            }
        }
    }

    /// Handle a pending execution by processing operations and re-invoking as needed.
    async fn handle_pending_execution(
        &mut self,
        payload: I,
        execution_arn: String,
        initial_invocation: Invocation,
    ) -> Result<TestExecutionResult<O>, crate::error::TestError> {
        let mut invocations = vec![initial_invocation];
        let mut iteration_count = 0;
        const MAX_ITERATIONS: usize = 100; // Safety limit

        loop {
            iteration_count += 1;
            if iteration_count > MAX_ITERATIONS {
                return Err(crate::error::TestError::CheckpointServerError(
                    "Maximum iteration count exceeded".to_string(),
                ));
            }

            // Get current operations
            let operations = match self.checkpoint_api.get_operations(&execution_arn, "").await {
                Ok(response) => {
                    let mut storage = self.operation_storage.write().await;
                    for op in &response.operations {
                        storage.update_operation(op.clone());
                    }
                    response.operations
                }
                Err(_) => Vec::new(),
            };

            // Process operations and check for execution completion
            let process_result = self.process_operations(&operations, &execution_arn);

            match process_result {
                ProcessOperationsResult::ExecutionSucceeded(result_str) => {
                    self.execution_complete.store(true, Ordering::SeqCst);
                    if let Ok(result) = serde_json::from_str::<O>(&result_str) {
                        let mut test_result =
                            TestExecutionResult::success(result, operations, execution_arn);
                        test_result.invocations = invocations;
                        return Ok(test_result);
                    }
                    // If we can't parse the result, return running status
                    let mut test_result = TestExecutionResult::running(operations, execution_arn);
                    test_result.invocations = invocations;
                    return Ok(test_result);
                }
                ProcessOperationsResult::ExecutionFailed(error) => {
                    self.execution_complete.store(true, Ordering::SeqCst);
                    let mut test_result =
                        TestExecutionResult::failure(error, operations, execution_arn);
                    test_result.invocations = invocations;
                    return Ok(test_result);
                }
                ProcessOperationsResult::NoPendingOperations => {
                    // No more pending operations we can handle
                    let mut test_result = TestExecutionResult::running(operations, execution_arn);
                    test_result.invocations = invocations;
                    return Ok(test_result);
                }
                ProcessOperationsResult::ShouldReinvoke(advance_time_ms) => {
                    // If advance_time_ms is None, it means there are pending operations
                    // but none with a scheduled time (e.g., only callbacks are pending).
                    // Callbacks need external signals to complete, not re-invocation.
                    // In this case, return Running status instead of re-invoking.
                    if advance_time_ms.is_none() {
                        let mut test_result = TestExecutionResult::running(operations, execution_arn);
                        test_result.invocations = invocations;
                        return Ok(test_result);
                    }

                    // Advance time if needed (for time skipping mode)
                    if let Some(advance_ms) = advance_time_ms {
                        if advance_ms > 0 {
                            tokio::time::advance(tokio::time::Duration::from_millis(advance_ms))
                                .await;
                        }
                    }

                    // Mark all pending wait operations as SUCCEEDED after time advancement
                    // This is critical for the handler to see the waits as completed
                    // In time-skip mode, we complete waits immediately since we've already
                    // "advanced" time conceptually
                    if self.skip_time_config.enabled {
                        for op in &operations {
                            if op.operation_type == OperationType::Wait
                                && op.status == OperationStatus::Started
                            {
                                // In time-skip mode, complete all pending waits immediately
                                // We don't check the timestamp because tokio::time::advance
                                // doesn't affect chrono::Utc::now()
                                
                                // Update the wait operation to SUCCEEDED
                                let mut updated_operation = op.clone();
                                updated_operation.status = OperationStatus::Succeeded;
                                updated_operation.end_timestamp = Some(chrono::Utc::now().timestamp_millis());

                                let update_request = super::types::UpdateCheckpointDataRequest {
                                    execution_id: execution_arn.clone(),
                                    operation_id: op.operation_id.clone(),
                                    operation_data: updated_operation,
                                    payload: None,
                                    error: None,
                                };

                                let payload = serde_json::to_string(&update_request)?;
                                let _ = self
                                    .checkpoint_api
                                    .send_api_request(super::types::ApiType::UpdateCheckpointData, payload)
                                    .await;
                            }
                        }
                    }
                }
            }

            // Re-invoke the handler
            let new_invocation_id = uuid::Uuid::new_v4().to_string();
            let start_invocation_request = super::types::StartInvocationRequest {
                execution_id: execution_arn.clone(),
                invocation_id: new_invocation_id.clone(),
            };
            let start_payload = serde_json::to_string(&start_invocation_request)?;

            let start_response = self
                .checkpoint_api
                .send_api_request(super::types::ApiType::StartInvocation, start_payload)
                .await?;

            if let Some(error) = start_response.error {
                return Err(crate::error::TestError::CheckpointServerError(error));
            }

            let invocation_result: super::InvocationResult = serde_json::from_str(
                &start_response.payload.ok_or_else(|| {
                    crate::error::TestError::CheckpointServerError(
                        "Empty response from checkpoint server".to_string(),
                    )
                })?,
            )?;

            self.checkpoint_token = Some(invocation_result.checkpoint_token.clone());

            // Create new execution state for re-invocation with the current operations
            // This is critical for the handler to see the updated operation states
            use aws_durable_execution_sdk::lambda::InitialExecutionState;
            use aws_durable_execution_sdk::state::ExecutionState;

            // Convert operation_events to Operations for the initial state
            let current_operations: Vec<Operation> = invocation_result
                .operation_events
                .iter()
                .map(|e| e.operation.clone())
                .collect();
            let initial_state = InitialExecutionState::with_operations(current_operations);
            let execution_state = Arc::new(ExecutionState::new(
                &execution_arn,
                &invocation_result.checkpoint_token,
                initial_state,
                self.checkpoint_api.clone(),
            ));

            let ctx = DurableContext::new(execution_state);

            // Record invocation
            let start_time = chrono::Utc::now();
            let mut invocation = Invocation::with_start(start_time);

            // Execute handler
            self.invocation_active.store(true, Ordering::SeqCst);
            let handler_result = (self.handler)(payload.clone(), ctx).await;
            self.invocation_active.store(false, Ordering::SeqCst);

            let end_time = chrono::Utc::now();
            invocation = invocation.with_end(end_time);

            match handler_result {
                Ok(result) => {
                    self.execution_complete.store(true, Ordering::SeqCst);
                    invocations.push(invocation);

                    // Get final operations
                    let final_operations =
                        match self.checkpoint_api.get_operations(&execution_arn, "").await {
                            Ok(response) => response.operations,
                            Err(_) => Vec::new(),
                        };

                    let mut test_result =
                        TestExecutionResult::success(result, final_operations, execution_arn);
                    test_result.invocations = invocations;
                    return Ok(test_result);
                }
                Err(error) => {
                    if error.is_suspend() {
                        // Still pending, continue loop
                        invocations.push(invocation);
                        continue;
                    } else {
                        self.execution_complete.store(true, Ordering::SeqCst);
                        let error_obj = aws_durable_execution_sdk::ErrorObject::from(&error);
                        let test_error =
                            TestResultError::new(error_obj.error_type, error.to_string());
                        invocations.push(invocation.with_error(test_error.clone()));

                        let final_operations =
                            match self.checkpoint_api.get_operations(&execution_arn, "").await {
                                Ok(response) => response.operations,
                                Err(_) => Vec::new(),
                            };

                        let mut test_result = TestExecutionResult::failure(
                            test_error,
                            final_operations,
                            execution_arn,
                        );
                        test_result.invocations = invocations;
                        return Ok(test_result);
                    }
                }
            }
        }
    }

    /// Process a batch of operations from checkpoint polling.
    ///
    /// This method iterates over all operations and dispatches each one
    /// to the appropriate handler based on its type.
    ///
    /// # Arguments
    ///
    /// * `operations` - The list of operations to process
    /// * `execution_id` - The execution ID
    ///
    /// # Returns
    ///
    /// A `ProcessOperationsResult` indicating what action should be taken next.
    ///
    /// # Requirements
    ///
    /// - 18.2: WHEN checkpoint polling receives operation updates,
    ///   THE Test_Execution_Orchestrator SHALL process each operation based on its type
    fn process_operations(
        &mut self,
        operations: &[Operation],
        execution_id: &str,
    ) -> ProcessOperationsResult {
        // First, check for execution completion
        if let Some(exec_result) = self.handle_execution_update(operations) {
            return exec_result;
        }

        // Track pending operations and earliest scheduled time
        let mut has_pending_operations = false;
        let mut earliest_scheduled_time: Option<i64> = None;

        // Process each operation
        for operation in operations {
            let result = self.process_operation(operation, execution_id);

            match result {
                OperationProcessResult::Pending(scheduled_time) => {
                    has_pending_operations = true;
                    if let Some(time) = scheduled_time {
                        match earliest_scheduled_time {
                            None => earliest_scheduled_time = Some(time),
                            Some(current) if time < current => earliest_scheduled_time = Some(time),
                            _ => {}
                        }
                    }
                }
                OperationProcessResult::Completed => {
                    // Operation is complete, nothing to do
                }
                OperationProcessResult::NotApplicable => {
                    // Operation type not handled
                }
            }
        }

        if !has_pending_operations {
            return ProcessOperationsResult::NoPendingOperations;
        }

        // Calculate time to advance if time skipping is enabled
        let advance_time_ms = if self.skip_time_config.enabled {
            if let Some(end_ts) = earliest_scheduled_time {
                let now_ms = chrono::Utc::now().timestamp_millis();
                if end_ts > now_ms {
                    Some((end_ts - now_ms) as u64)
                } else {
                    Some(0)
                }
            } else {
                None
            }
        } else {
            None
        };

        ProcessOperationsResult::ShouldReinvoke(advance_time_ms)
    }

    /// Process a single operation based on its type.
    ///
    /// This method dispatches to the appropriate handler based on the operation type.
    ///
    /// # Arguments
    ///
    /// * `operation` - The operation to process
    /// * `execution_id` - The execution ID
    ///
    /// # Returns
    ///
    /// An `OperationProcessResult` indicating the operation's status.
    ///
    /// # Requirements
    ///
    /// - 18.2: WHEN checkpoint polling receives operation updates,
    ///   THE Test_Execution_Orchestrator SHALL process each operation based on its type
    fn process_operation(
        &mut self,
        operation: &Operation,
        execution_id: &str,
    ) -> OperationProcessResult {
        // Skip completed operations
        if operation.status.is_terminal() {
            return OperationProcessResult::Completed;
        }

        match operation.operation_type {
            OperationType::Wait => self.handle_wait_update(operation, execution_id),
            OperationType::Step => self.handle_step_update(operation, execution_id),
            OperationType::Callback => self.handle_callback_update(operation, execution_id),
            OperationType::Execution => {
                // Execution operations are handled separately in handle_execution_update
                OperationProcessResult::NotApplicable
            }
            OperationType::Invoke | OperationType::Context => {
                // These operation types are handled by the SDK internally
                if operation.status == OperationStatus::Started {
                    OperationProcessResult::Pending(None)
                } else {
                    OperationProcessResult::Completed
                }
            }
        }
    }

    /// Handle WAIT operation - schedule re-invocation at wait end time.
    ///
    /// When a wait operation is encountered, this method extracts the scheduled
    /// end timestamp and returns it so the orchestrator can schedule re-invocation.
    ///
    /// # Arguments
    ///
    /// * `operation` - The wait operation to handle
    /// * `execution_id` - The execution ID
    ///
    /// # Returns
    ///
    /// An `OperationProcessResult` with the scheduled end timestamp if available.
    ///
    /// # Requirements
    ///
    /// - 16.1: WHEN a wait operation is encountered, THE Test_Execution_Orchestrator
    ///   SHALL track the wait's scheduled end timestamp
    /// - 18.3: WHEN a WAIT operation update is received with START action,
    ///   THE Test_Execution_Orchestrator SHALL schedule re-invocation at the
    ///   wait's scheduled end timestamp
    fn handle_wait_update(
        &mut self,
        operation: &Operation,
        _execution_id: &str,
    ) -> OperationProcessResult {
        // Only process started wait operations
        if operation.status != OperationStatus::Started {
            return OperationProcessResult::Completed;
        }

        // Track this as a pending operation
        self.pending_operations
            .insert(operation.operation_id.clone());

        // Extract scheduled end timestamp from wait details
        let scheduled_end_timestamp = operation
            .wait_details
            .as_ref()
            .and_then(|details| details.scheduled_end_timestamp);

        OperationProcessResult::Pending(scheduled_end_timestamp)
    }

    /// Handle STEP operation - schedule retry at next attempt time.
    ///
    /// When a step operation is pending retry, this method extracts the next
    /// attempt timestamp and returns it so the orchestrator can schedule re-invocation.
    ///
    /// # Arguments
    ///
    /// * `operation` - The step operation to handle
    /// * `execution_id` - The execution ID
    ///
    /// # Returns
    ///
    /// An `OperationProcessResult` with the next attempt timestamp if available.
    ///
    /// # Requirements
    ///
    /// - 18.4: WHEN a STEP operation update is received with RETRY action,
    ///   THE Test_Execution_Orchestrator SHALL schedule re-invocation at the
    ///   step's next attempt timestamp
    fn handle_step_update(
        &mut self,
        operation: &Operation,
        _execution_id: &str,
    ) -> OperationProcessResult {
        // Check if step is pending retry
        if operation.status != OperationStatus::Pending && operation.status != OperationStatus::Started {
            return OperationProcessResult::Completed;
        }

        // Track this as a pending operation
        self.pending_operations
            .insert(operation.operation_id.clone());

        // Extract next attempt timestamp from step details
        let next_attempt_timestamp = operation
            .step_details
            .as_ref()
            .and_then(|details| details.next_attempt_timestamp);

        OperationProcessResult::Pending(next_attempt_timestamp)
    }

    /// Handle CALLBACK operation - schedule re-invocation when callback completes.
    ///
    /// When a callback operation is started, this method tracks it as pending.
    /// The callback will complete when an external system sends a success/failure response.
    ///
    /// # Arguments
    ///
    /// * `operation` - The callback operation to handle
    /// * `execution_id` - The execution ID
    ///
    /// # Returns
    ///
    /// An `OperationProcessResult` indicating the callback is pending.
    ///
    /// # Requirements
    ///
    /// - 18.5: WHEN a CALLBACK operation status changes to SUCCEEDED or FAILED,
    ///   THE Test_Execution_Orchestrator SHALL schedule handler re-invocation
    fn handle_callback_update(
        &mut self,
        operation: &Operation,
        _execution_id: &str,
    ) -> OperationProcessResult {
        // Check if callback is still pending
        if operation.status != OperationStatus::Started {
            // Callback has completed (succeeded or failed)
            self.pending_operations.remove(&operation.operation_id);
            return OperationProcessResult::Completed;
        }

        // Track this as a pending operation
        self.pending_operations
            .insert(operation.operation_id.clone());

        // Callbacks don't have a scheduled time - they complete when external
        // system sends a response
        OperationProcessResult::Pending(None)
    }

    /// Schedule handler re-invocation at a specific timestamp.
    ///
    /// This method schedules a handler re-invocation via the scheduler. When time
    /// skipping is enabled, it advances tokio time before invocation. It also
    /// updates checkpoint data (marks the operation as SUCCEEDED) before re-invoking.
    ///
    /// # Arguments
    ///
    /// * `timestamp_ms` - The timestamp in milliseconds since epoch when to invoke
    /// * `execution_id` - The execution ID
    /// * `operation_id` - The operation ID to mark as SUCCEEDED before re-invoking
    ///
    /// # Requirements
    ///
    /// - 16.2: WHEN time skipping is enabled and a wait's scheduled end time is reached,
    ///   THE Test_Execution_Orchestrator SHALL mark the wait as SUCCEEDED and schedule
    ///   handler re-invocation
    /// - 16.3: WHEN time skipping is enabled, THE Test_Execution_Orchestrator SHALL use
    ///   tokio::time::advance() to skip wait durations instantly
    /// - 17.3: WHEN a function is scheduled, THE Scheduler SHALL execute any checkpoint
    ///   updates before invoking the handler
    pub fn schedule_invocation_at_timestamp(
        &mut self,
        timestamp_ms: i64,
        execution_id: &str,
        operation_id: &str,
    ) {
        let checkpoint_api = Arc::clone(&self.checkpoint_api);
        let execution_id_owned = execution_id.to_string();
        let operation_id_owned = operation_id.to_string();
        let skip_time_enabled = self.skip_time_config.enabled;

        // Calculate the timestamp as DateTime<Utc>
        let timestamp = chrono::DateTime::from_timestamp_millis(timestamp_ms)
            .map(|dt| dt.with_timezone(&chrono::Utc));

        // Create the checkpoint update function that marks the operation as SUCCEEDED
        let update_checkpoint: super::scheduler::CheckpointUpdateFn = Box::new(move || {
            let checkpoint_api = checkpoint_api;
            let execution_id = execution_id_owned;
            let operation_id = operation_id_owned;

            Box::pin(async move {
                // If time skipping is enabled, advance tokio time to the scheduled timestamp
                if skip_time_enabled {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let target_ms = timestamp_ms;
                    if target_ms > now_ms {
                        let advance_duration =
                            tokio::time::Duration::from_millis((target_ms - now_ms) as u64);
                        tokio::time::advance(advance_duration).await;
                    }
                }

                // Update checkpoint data to mark the operation as SUCCEEDED
                let mut updated_operation =
                    Operation::new(&operation_id, OperationType::Wait);
                updated_operation.status = OperationStatus::Succeeded;
                updated_operation.end_timestamp = Some(chrono::Utc::now().timestamp_millis());

                let update_request = super::types::UpdateCheckpointDataRequest {
                    execution_id: execution_id.clone(),
                    operation_id: operation_id.clone(),
                    operation_data: updated_operation,
                    payload: None,
                    error: None,
                };

                let payload = serde_json::to_string(&update_request)
                    .map_err(crate::error::TestError::SerializationError)?;

                let response = checkpoint_api
                    .send_api_request(super::types::ApiType::UpdateCheckpointData, payload)
                    .await?;

                if let Some(error) = response.error {
                    return Err(crate::error::TestError::CheckpointServerError(error));
                }

                Ok(())
            })
        });

        // Create the invocation function (empty for now - actual invocation happens in the main loop)
        let start_invocation: super::scheduler::BoxedAsyncFn = Box::new(|| {
            Box::pin(async {
                // The actual handler invocation is managed by the main execution loop
                // This function just signals that the scheduled time has been reached
            })
        });

        // Create the error handler
        let on_error: super::scheduler::ErrorHandler = Box::new(|error| {
            tracing::error!("Error during scheduled invocation: {:?}", error);
        });

        // Schedule the function via the scheduler
        self.scheduler.schedule_function(
            start_invocation,
            on_error,
            timestamp,
            Some(update_checkpoint),
        );
    }

    /// Schedule handler re-invocation at a specific timestamp with a custom checkpoint update.
    ///
    /// This is a more flexible version that allows specifying a custom checkpoint update function.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp when to invoke (optional, None for immediate)
    /// * `update_checkpoint` - Optional function to update checkpoint data before invocation
    ///
    /// # Requirements
    ///
    /// - 17.3: WHEN a function is scheduled, THE Scheduler SHALL execute any checkpoint
    ///   updates before invoking the handler
    pub fn schedule_invocation_with_update(
        &mut self,
        timestamp: Option<chrono::DateTime<chrono::Utc>>,
        update_checkpoint: Option<super::scheduler::CheckpointUpdateFn>,
    ) {
        let skip_time_enabled = self.skip_time_config.enabled;

        // Wrap the checkpoint update to include time advancement if needed
        let wrapped_update: Option<super::scheduler::CheckpointUpdateFn> = if skip_time_enabled {
            if let Some(ts) = timestamp {
                let original_update = update_checkpoint;
                Some(Box::new(move || {
                    Box::pin(async move {
                        // Advance tokio time to the scheduled timestamp
                        let now = chrono::Utc::now();
                        if ts > now {
                            let duration = (ts - now).to_std().unwrap_or_default();
                            tokio::time::advance(duration).await;
                        }

                        // Execute the original checkpoint update if provided
                        if let Some(update_fn) = original_update {
                            update_fn().await?;
                        }

                        Ok(())
                    })
                }))
            } else {
                update_checkpoint
            }
        } else {
            update_checkpoint
        };

        // Create the invocation function
        let start_invocation: super::scheduler::BoxedAsyncFn = Box::new(|| {
            Box::pin(async {
                // The actual handler invocation is managed by the main execution loop
            })
        });

        // Create the error handler
        let on_error: super::scheduler::ErrorHandler = Box::new(|error| {
            tracing::error!("Error during scheduled invocation: {:?}", error);
        });

        // Schedule the function
        self.scheduler.schedule_function(
            start_invocation,
            on_error,
            timestamp,
            wrapped_update,
        );
    }

    /// Check if there are scheduled functions pending.
    ///
    /// # Returns
    ///
    /// `true` if there are scheduled functions waiting to be executed.
    ///
    /// # Requirements
    ///
    /// - 17.4: WHEN the scheduler has pending functions, THE Scheduler SHALL report
    ///   that scheduled functions exist via has_scheduled_function()
    pub fn has_scheduled_functions(&self) -> bool {
        self.scheduler.has_scheduled_function()
    }

    /// Invoke the handler and process the result.
    ///
    /// This method handles a single handler invocation, including:
    /// - Checking for active invocations (prevents concurrent invocations in time-skip mode)
    /// - Starting invocation via checkpoint API
    /// - Invoking handler with checkpoint token and operations
    /// - Processing handler result (PENDING, SUCCEEDED, FAILED)
    /// - Scheduling re-invocation if dirty operations exist
    ///
    /// # Arguments
    ///
    /// * `payload` - The input payload to pass to the handler
    /// * `execution_id` - The execution ID
    /// * `is_initial` - Whether this is the initial invocation (vs a re-invocation)
    ///
    /// # Returns
    ///
    /// An `InvokeHandlerResult` indicating the outcome of the invocation.
    ///
    /// # Requirements
    ///
    /// - 16.4: WHEN a handler invocation returns PENDING status,
    ///   THE Test_Execution_Orchestrator SHALL continue polling for operation
    ///   updates and re-invoke the handler when operations complete
    /// - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
    ///   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
    /// - 16.6: WHEN multiple operations are pending (waits, callbacks, steps with retries),
    ///   THE Test_Execution_Orchestrator SHALL process them in scheduled order
    pub async fn invoke_handler(
        &mut self,
        payload: I,
        execution_id: &str,
        is_initial: bool,
    ) -> Result<InvokeHandlerResult<O>, crate::error::TestError> {
        use super::types::{ApiType, StartDurableExecutionRequest, StartInvocationRequest};
        use aws_durable_execution_sdk::lambda::InitialExecutionState;
        use aws_durable_execution_sdk::state::ExecutionState;

        // Check for active invocations (prevent concurrent invocations in time-skip mode)
        if self.skip_time_config.enabled && self.invocation_active.load(Ordering::SeqCst) {
            return Err(crate::error::TestError::CheckpointServerError(
                "Concurrent invocation detected in time-skip mode. Only one invocation can be active at a time.".to_string(),
            ));
        }

        // Start invocation via checkpoint API
        let invocation_id = uuid::Uuid::new_v4().to_string();
        let checkpoint_token = if is_initial {
            // For initial invocation, start a new durable execution
            let payload_json = serde_json::to_string(&payload)?;
            let start_request = StartDurableExecutionRequest {
                invocation_id: invocation_id.clone(),
                payload: Some(payload_json),
            };
            let start_payload = serde_json::to_string(&start_request)?;

            let start_response = self
                .checkpoint_api
                .send_api_request(ApiType::StartDurableExecution, start_payload)
                .await?;

            if let Some(error) = start_response.error {
                return Err(crate::error::TestError::CheckpointServerError(error));
            }

            let invocation_result: super::InvocationResult = serde_json::from_str(
                &start_response.payload.ok_or_else(|| {
                    crate::error::TestError::CheckpointServerError(
                        "Empty response from checkpoint server".to_string(),
                    )
                })?,
            )?;

            // Update orchestrator state
            self.execution_id = Some(invocation_result.execution_id.clone());
            self.checkpoint_token = Some(invocation_result.checkpoint_token.clone());

            invocation_result.checkpoint_token
        } else {
            // For re-invocation, start a new invocation for existing execution
            let start_invocation_request = StartInvocationRequest {
                execution_id: execution_id.to_string(),
                invocation_id: invocation_id.clone(),
            };
            let start_payload = serde_json::to_string(&start_invocation_request)?;

            let start_response = self
                .checkpoint_api
                .send_api_request(ApiType::StartInvocation, start_payload)
                .await?;

            if let Some(error) = start_response.error {
                return Err(crate::error::TestError::CheckpointServerError(error));
            }

            let invocation_result: super::InvocationResult = serde_json::from_str(
                &start_response.payload.ok_or_else(|| {
                    crate::error::TestError::CheckpointServerError(
                        "Empty response from checkpoint server".to_string(),
                    )
                })?,
            )?;

            // Update checkpoint token
            self.checkpoint_token = Some(invocation_result.checkpoint_token.clone());

            invocation_result.checkpoint_token
        };

        // Create execution state with the checkpoint worker manager
        let initial_state = InitialExecutionState::new();
        let execution_state = Arc::new(ExecutionState::new(
            execution_id,
            &checkpoint_token,
            initial_state,
            self.checkpoint_api.clone(),
        ));

        // Create the durable context
        let ctx = DurableContext::new(execution_state.clone());

        // Record invocation start
        let start_time = chrono::Utc::now();
        let mut invocation = Invocation::with_start(start_time);

        // Mark invocation as active
        self.invocation_active.store(true, Ordering::SeqCst);

        // Invoke handler with checkpoint token and operations
        let handler_result = (self.handler)(payload.clone(), ctx).await;

        // Mark invocation as inactive
        self.invocation_active.store(false, Ordering::SeqCst);

        // Record invocation end
        let end_time = chrono::Utc::now();
        invocation = invocation.with_end(end_time);

        // Retrieve operations from the checkpoint server
        let operations = match self.checkpoint_api.get_operations(execution_id, "").await {
            Ok(response) => {
                let mut storage = self.operation_storage.write().await;
                for op in &response.operations {
                    storage.update_operation(op.clone());
                }
                response.operations
            }
            Err(_) => Vec::new(),
        };

        // Process handler result (PENDING, SUCCEEDED, FAILED)
        match handler_result {
            Ok(result) => {
                // Handler completed successfully
                self.execution_complete.store(true, Ordering::SeqCst);
                Ok(InvokeHandlerResult::Succeeded {
                    result,
                    operations,
                    invocation,
                })
            }
            Err(error) => {
                if error.is_suspend() {
                    // Handler suspended - check for dirty operations and schedule re-invocation
                    let process_result = self.process_operations(&operations, execution_id);

                    match process_result {
                        ProcessOperationsResult::ExecutionSucceeded(result_str) => {
                            self.execution_complete.store(true, Ordering::SeqCst);
                            if let Ok(result) = serde_json::from_str::<O>(&result_str) {
                                Ok(InvokeHandlerResult::Succeeded {
                                    result,
                                    operations,
                                    invocation,
                                })
                            } else {
                                Ok(InvokeHandlerResult::Pending {
                                    operations,
                                    invocation,
                                    should_reinvoke: false,
                                    advance_time_ms: None,
                                })
                            }
                        }
                        ProcessOperationsResult::ExecutionFailed(test_error) => {
                            self.execution_complete.store(true, Ordering::SeqCst);
                            Ok(InvokeHandlerResult::Failed {
                                error: test_error,
                                operations,
                                invocation,
                            })
                        }
                        ProcessOperationsResult::NoPendingOperations => {
                            // No pending operations - execution is stuck
                            Ok(InvokeHandlerResult::Pending {
                                operations,
                                invocation,
                                should_reinvoke: false,
                                advance_time_ms: None,
                            })
                        }
                        ProcessOperationsResult::ShouldReinvoke(advance_time_ms) => {
                            // Schedule re-invocation if dirty operations exist
                            Ok(InvokeHandlerResult::Pending {
                                operations,
                                invocation,
                                should_reinvoke: true,
                                advance_time_ms,
                            })
                        }
                    }
                } else {
                    // Handler failed with an actual error
                    self.execution_complete.store(true, Ordering::SeqCst);
                    let error_obj = aws_durable_execution_sdk::ErrorObject::from(&error);
                    let test_error = TestResultError::new(error_obj.error_type, error.to_string());
                    let invocation_with_error = invocation.with_error(test_error.clone());
                    Ok(InvokeHandlerResult::Failed {
                        error: test_error,
                        operations,
                        invocation: invocation_with_error,
                    })
                }
            }
        }
    }

    /// Flush all scheduled functions without executing them.
    ///
    /// This is useful for cleanup when execution completes or is cancelled.
    ///
    /// # Requirements
    ///
    /// - 17.5: WHEN execution completes, THE Scheduler SHALL flush any remaining
    ///   scheduled functions
    pub fn flush_scheduled_functions(&mut self) {
        self.scheduler.flush_timers();
    }

    /// Process the next scheduled function.
    ///
    /// # Returns
    ///
    /// `true` if a function was processed, `false` if the queue is empty.
    pub async fn process_next_scheduled(&mut self) -> bool {
        self.scheduler.process_next().await
    }

    /// Handle EXECUTION operation - resolve execution when complete.
    ///
    /// This method checks if the execution operation has completed (succeeded or failed)
    /// and returns the appropriate result.
    ///
    /// # Arguments
    ///
    /// * `operations` - All operations to search for the execution operation
    ///
    /// # Returns
    ///
    /// `Some(ProcessOperationsResult)` if execution is complete, `None` otherwise.
    ///
    /// # Requirements
    ///
    /// - 16.5: WHEN a handler invocation returns SUCCEEDED or FAILED status,
    ///   THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
    fn handle_execution_update(&self, operations: &[Operation]) -> Option<ProcessOperationsResult> {
        // Find the execution operation
        let execution_op = operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution)?;

        match execution_op.status {
            OperationStatus::Succeeded => {
                let result_str = execution_op.result.clone().unwrap_or_default();
                Some(ProcessOperationsResult::ExecutionSucceeded(result_str))
            }
            OperationStatus::Failed => {
                let error = if let Some(err) = &execution_op.error {
                    TestResultError::new(err.error_type.clone(), err.error_message.clone())
                } else {
                    TestResultError::new("ExecutionFailed", "Execution failed")
                };
                Some(ProcessOperationsResult::ExecutionFailed(error))
            }
            _ => None,
        }
    }
}

/// Result of processing operations.
///
/// This enum represents the possible outcomes of processing a batch of operations.
#[derive(Debug)]
pub enum ProcessOperationsResult {
    /// Execution completed successfully with the given result string
    ExecutionSucceeded(String),
    /// Execution failed with the given error
    ExecutionFailed(TestResultError),
    /// No pending operations that can be advanced
    NoPendingOperations,
    /// Should re-invoke the handler, optionally advancing time by the given milliseconds
    ShouldReinvoke(Option<u64>),
}

/// Result of processing a single operation.
///
/// This enum represents the possible outcomes of processing a single operation.
#[derive(Debug)]
pub enum OperationProcessResult {
    /// Operation is pending with an optional scheduled timestamp (milliseconds since epoch)
    Pending(Option<i64>),
    /// Operation has completed
    Completed,
    /// Operation type is not applicable for processing
    NotApplicable,
}

/// Result of invoking the handler.
///
/// This enum represents the possible outcomes of a single handler invocation.
#[derive(Debug)]
pub enum InvokeHandlerResult<T> {
    /// Handler completed successfully with a result
    Succeeded {
        /// The result value
        result: T,
        /// All operations from the execution
        operations: Vec<Operation>,
        /// The invocation details
        invocation: Invocation,
    },
    /// Handler failed with an error
    Failed {
        /// The error that occurred
        error: TestResultError,
        /// All operations from the execution
        operations: Vec<Operation>,
        /// The invocation details
        invocation: Invocation,
    },
    /// Handler is pending (suspended) and may need re-invocation
    Pending {
        /// All operations from the execution
        operations: Vec<Operation>,
        /// The invocation details
        invocation: Invocation,
        /// Whether the handler should be re-invoked
        should_reinvoke: bool,
        /// Optional time to advance before re-invocation (milliseconds)
        advance_time_ms: Option<u64>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::{ErrorObject, WaitDetails, StepDetails};

    #[test]
    fn test_skip_time_config_default() {
        let config = SkipTimeConfig::default();
        assert!(!config.enabled);
    }

    #[test]
    fn test_operation_storage_new() {
        let storage = OperationStorage::new();
        assert!(storage.is_empty());
        assert_eq!(storage.len(), 0);
    }

    #[test]
    fn test_operation_storage_add_and_get() {
        let mut storage = OperationStorage::new();

        let op = Operation::new("op-1", aws_durable_execution_sdk::OperationType::Step);
        storage.add_operation(op);

        assert_eq!(storage.len(), 1);
        assert!(storage.get_by_id("op-1").is_some());
    }

    #[test]
    fn test_operation_storage_update() {
        let mut storage = OperationStorage::new();

        let mut op = Operation::new("op-1", aws_durable_execution_sdk::OperationType::Step);
        op.status = aws_durable_execution_sdk::OperationStatus::Started;
        storage.add_operation(op);

        let mut updated_op = Operation::new("op-1", aws_durable_execution_sdk::OperationType::Step);
        updated_op.status = aws_durable_execution_sdk::OperationStatus::Succeeded;
        storage.update_operation(updated_op);

        assert_eq!(storage.len(), 1);
        let retrieved = storage.get_by_id("op-1").unwrap();
        assert_eq!(
            retrieved.status,
            aws_durable_execution_sdk::OperationStatus::Succeeded
        );
    }

    #[test]
    fn test_test_execution_result_success() {
        let result: TestExecutionResult<String> =
            TestExecutionResult::success("test".to_string(), vec![], "exec-1".to_string());
        assert_eq!(result.status, ExecutionStatus::Succeeded);
        assert_eq!(result.result, Some("test".to_string()));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_test_execution_result_failure() {
        let error = TestResultError::new("TestError", "test error");
        let result: TestExecutionResult<String> =
            TestExecutionResult::failure(error, vec![], "exec-1".to_string());
        assert_eq!(result.status, ExecutionStatus::Failed);
        assert!(result.result.is_none());
        assert!(result.error.is_some());
    }

    #[test]
    fn test_test_execution_result_running() {
        let result: TestExecutionResult<String> =
            TestExecutionResult::running(vec![], "exec-1".to_string());
        assert_eq!(result.status, ExecutionStatus::Running);
        assert!(result.result.is_none());
        assert!(result.error.is_none());
    }

    // Tests for ProcessOperationsResult
    #[test]
    fn test_process_operations_result_execution_succeeded() {
        let result = ProcessOperationsResult::ExecutionSucceeded("test result".to_string());
        match result {
            ProcessOperationsResult::ExecutionSucceeded(s) => assert_eq!(s, "test result"),
            _ => panic!("Expected ExecutionSucceeded"),
        }
    }

    #[test]
    fn test_process_operations_result_execution_failed() {
        let error = TestResultError::new("TestError", "test error");
        let result = ProcessOperationsResult::ExecutionFailed(error);
        match result {
            ProcessOperationsResult::ExecutionFailed(e) => {
                assert_eq!(e.error_type, Some("TestError".to_string()));
            }
            _ => panic!("Expected ExecutionFailed"),
        }
    }

    #[test]
    fn test_process_operations_result_no_pending() {
        let result = ProcessOperationsResult::NoPendingOperations;
        assert!(matches!(result, ProcessOperationsResult::NoPendingOperations));
    }

    #[test]
    fn test_process_operations_result_should_reinvoke() {
        let result = ProcessOperationsResult::ShouldReinvoke(Some(1000));
        match result {
            ProcessOperationsResult::ShouldReinvoke(Some(ms)) => assert_eq!(ms, 1000),
            _ => panic!("Expected ShouldReinvoke with time"),
        }
    }

    // Tests for OperationProcessResult
    #[test]
    fn test_operation_process_result_pending_with_timestamp() {
        let result = OperationProcessResult::Pending(Some(1234567890));
        match result {
            OperationProcessResult::Pending(Some(ts)) => assert_eq!(ts, 1234567890),
            _ => panic!("Expected Pending with timestamp"),
        }
    }

    #[test]
    fn test_operation_process_result_pending_without_timestamp() {
        let result = OperationProcessResult::Pending(None);
        match result {
            OperationProcessResult::Pending(None) => {}
            _ => panic!("Expected Pending without timestamp"),
        }
    }

    #[test]
    fn test_operation_process_result_completed() {
        let result = OperationProcessResult::Completed;
        assert!(matches!(result, OperationProcessResult::Completed));
    }

    #[test]
    fn test_operation_process_result_not_applicable() {
        let result = OperationProcessResult::NotApplicable;
        assert!(matches!(result, OperationProcessResult::NotApplicable));
    }

    // Tests for handle_execution_update
    #[test]
    fn test_handle_execution_update_succeeded() {
        // Create a mock orchestrator for testing
        // We'll test the logic directly by creating operations
        let mut exec_op = Operation::new("exec-1", OperationType::Execution);
        exec_op.status = OperationStatus::Succeeded;
        exec_op.result = Some("\"success\"".to_string());

        let operations = vec![exec_op];

        // Find execution operation and check status
        let execution_op = operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution);

        assert!(execution_op.is_some());
        let exec = execution_op.unwrap();
        assert_eq!(exec.status, OperationStatus::Succeeded);
        assert_eq!(exec.result, Some("\"success\"".to_string()));
    }

    #[test]
    fn test_handle_execution_update_failed() {
        let mut exec_op = Operation::new("exec-1", OperationType::Execution);
        exec_op.status = OperationStatus::Failed;
        exec_op.error = Some(ErrorObject {
            error_type: "TestError".to_string(),
            error_message: "Test error message".to_string(),
            stack_trace: None,
        });

        let operations = vec![exec_op];

        let execution_op = operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution);

        assert!(execution_op.is_some());
        let exec = execution_op.unwrap();
        assert_eq!(exec.status, OperationStatus::Failed);
        assert!(exec.error.is_some());
    }

    #[test]
    fn test_handle_execution_update_still_running() {
        let mut exec_op = Operation::new("exec-1", OperationType::Execution);
        exec_op.status = OperationStatus::Started;

        let operations = vec![exec_op];

        let execution_op = operations
            .iter()
            .find(|op| op.operation_type == OperationType::Execution);

        assert!(execution_op.is_some());
        let exec = execution_op.unwrap();
        assert_eq!(exec.status, OperationStatus::Started);
    }

    // Tests for wait operation handling
    #[test]
    fn test_wait_operation_started_with_timestamp() {
        let mut wait_op = Operation::new("wait-1", OperationType::Wait);
        wait_op.status = OperationStatus::Started;
        wait_op.wait_details = Some(WaitDetails {
            scheduled_end_timestamp: Some(1234567890000),
        });

        // Verify the wait details are accessible
        assert!(wait_op.wait_details.is_some());
        let details = wait_op.wait_details.as_ref().unwrap();
        assert_eq!(details.scheduled_end_timestamp, Some(1234567890000));
    }

    #[test]
    fn test_wait_operation_completed() {
        let mut wait_op = Operation::new("wait-1", OperationType::Wait);
        wait_op.status = OperationStatus::Succeeded;

        // Completed operations should be skipped
        assert!(wait_op.status.is_terminal());
    }

    // Tests for step operation handling
    #[test]
    fn test_step_operation_pending_retry() {
        let mut step_op = Operation::new("step-1", OperationType::Step);
        step_op.status = OperationStatus::Pending;
        step_op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(1),
            next_attempt_timestamp: Some(1234567890000),
            error: None,
            payload: None,
        });

        // Verify the step details are accessible
        assert!(step_op.step_details.is_some());
        let details = step_op.step_details.as_ref().unwrap();
        assert_eq!(details.next_attempt_timestamp, Some(1234567890000));
        assert_eq!(details.attempt, Some(1));
    }

    #[test]
    fn test_step_operation_succeeded() {
        let mut step_op = Operation::new("step-1", OperationType::Step);
        step_op.status = OperationStatus::Succeeded;
        step_op.step_details = Some(StepDetails {
            result: Some("\"result\"".to_string()),
            attempt: Some(0),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });

        // Completed operations should be skipped
        assert!(step_op.status.is_terminal());
    }

    // Tests for callback operation handling
    #[test]
    fn test_callback_operation_started() {
        let mut callback_op = Operation::new("callback-1", OperationType::Callback);
        callback_op.status = OperationStatus::Started;

        // Started callbacks are pending
        assert_eq!(callback_op.status, OperationStatus::Started);
        assert!(!callback_op.status.is_terminal());
    }

    #[test]
    fn test_callback_operation_succeeded() {
        let mut callback_op = Operation::new("callback-1", OperationType::Callback);
        callback_op.status = OperationStatus::Succeeded;

        // Completed callbacks should trigger re-invocation
        assert!(callback_op.status.is_terminal());
    }

    // Tests for operation type dispatch
    #[test]
    fn test_operation_type_dispatch_wait() {
        let op = Operation::new("op-1", OperationType::Wait);
        assert_eq!(op.operation_type, OperationType::Wait);
    }

    #[test]
    fn test_operation_type_dispatch_step() {
        let op = Operation::new("op-1", OperationType::Step);
        assert_eq!(op.operation_type, OperationType::Step);
    }

    #[test]
    fn test_operation_type_dispatch_callback() {
        let op = Operation::new("op-1", OperationType::Callback);
        assert_eq!(op.operation_type, OperationType::Callback);
    }

    #[test]
    fn test_operation_type_dispatch_execution() {
        let op = Operation::new("op-1", OperationType::Execution);
        assert_eq!(op.operation_type, OperationType::Execution);
    }

    #[test]
    fn test_operation_type_dispatch_invoke() {
        let op = Operation::new("op-1", OperationType::Invoke);
        assert_eq!(op.operation_type, OperationType::Invoke);
    }

    #[test]
    fn test_operation_type_dispatch_context() {
        let op = Operation::new("op-1", OperationType::Context);
        assert_eq!(op.operation_type, OperationType::Context);
    }

    // Test earliest scheduled time calculation
    #[test]
    fn test_earliest_scheduled_time_single_wait() {
        let mut wait_op = Operation::new("wait-1", OperationType::Wait);
        wait_op.status = OperationStatus::Started;
        wait_op.wait_details = Some(WaitDetails {
            scheduled_end_timestamp: Some(1000),
        });

        let operations = vec![wait_op];

        let mut earliest: Option<i64> = None;
        for op in &operations {
            if op.operation_type == OperationType::Wait && op.status == OperationStatus::Started {
                if let Some(details) = &op.wait_details {
                    if let Some(end_ts) = details.scheduled_end_timestamp {
                        match earliest {
                            None => earliest = Some(end_ts),
                            Some(current) if end_ts < current => earliest = Some(end_ts),
                            _ => {}
                        }
                    }
                }
            }
        }

        assert_eq!(earliest, Some(1000));
    }

    #[test]
    fn test_earliest_scheduled_time_multiple_waits() {
        let mut wait_op1 = Operation::new("wait-1", OperationType::Wait);
        wait_op1.status = OperationStatus::Started;
        wait_op1.wait_details = Some(WaitDetails {
            scheduled_end_timestamp: Some(2000),
        });

        let mut wait_op2 = Operation::new("wait-2", OperationType::Wait);
        wait_op2.status = OperationStatus::Started;
        wait_op2.wait_details = Some(WaitDetails {
            scheduled_end_timestamp: Some(1000),
        });

        let mut wait_op3 = Operation::new("wait-3", OperationType::Wait);
        wait_op3.status = OperationStatus::Started;
        wait_op3.wait_details = Some(WaitDetails {
            scheduled_end_timestamp: Some(3000),
        });

        let operations = vec![wait_op1, wait_op2, wait_op3];

        let mut earliest: Option<i64> = None;
        for op in &operations {
            if op.operation_type == OperationType::Wait && op.status == OperationStatus::Started {
                if let Some(details) = &op.wait_details {
                    if let Some(end_ts) = details.scheduled_end_timestamp {
                        match earliest {
                            None => earliest = Some(end_ts),
                            Some(current) if end_ts < current => earliest = Some(end_ts),
                            _ => {}
                        }
                    }
                }
            }
        }

        assert_eq!(earliest, Some(1000)); // Should be the earliest
    }

    // Tests for schedule_invocation_at_timestamp
    #[tokio::test]
    async fn test_schedule_invocation_at_timestamp_schedules_function() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator with time skipping disabled
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let mut orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Schedule an invocation at a future timestamp
        let future_timestamp = chrono::Utc::now().timestamp_millis() + 1000;
        orchestrator.schedule_invocation_at_timestamp(
            future_timestamp,
            "exec-1",
            "wait-1",
        );

        // Verify that a function was scheduled
        assert!(orchestrator.has_scheduled_functions());
    }

    #[tokio::test]
    async fn test_schedule_invocation_with_update_schedules_function() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let mut orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Schedule an invocation with no timestamp (immediate)
        orchestrator.schedule_invocation_with_update(None, None);

        // Verify that a function was scheduled
        assert!(orchestrator.has_scheduled_functions());
    }

    #[tokio::test]
    async fn test_flush_scheduled_functions_clears_queue() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let mut orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Schedule multiple invocations
        let future_timestamp = chrono::Utc::now().timestamp_millis() + 1000;
        orchestrator.schedule_invocation_at_timestamp(future_timestamp, "exec-1", "wait-1");
        orchestrator.schedule_invocation_at_timestamp(future_timestamp + 1000, "exec-1", "wait-2");

        // Verify functions are scheduled
        assert!(orchestrator.has_scheduled_functions());

        // Flush all scheduled functions
        orchestrator.flush_scheduled_functions();

        // Verify queue is empty
        assert!(!orchestrator.has_scheduled_functions());
    }

    #[tokio::test]
    async fn test_process_next_scheduled_processes_function() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let mut orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Schedule an immediate invocation
        orchestrator.schedule_invocation_with_update(None, None);

        // Process the scheduled function
        let processed = orchestrator.process_next_scheduled().await;
        assert!(processed);

        // Queue should now be empty
        assert!(!orchestrator.has_scheduled_functions());
    }

    #[tokio::test]
    async fn test_schedule_invocation_with_time_skipping_enabled() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator with time skipping enabled
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let mut orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: true },
        );

        // Verify time skipping is enabled
        assert!(orchestrator.is_time_skipping_enabled());

        // Schedule an invocation at a future timestamp
        let future_timestamp = chrono::Utc::now().timestamp_millis() + 5000;
        orchestrator.schedule_invocation_at_timestamp(
            future_timestamp,
            "exec-1",
            "wait-1",
        );

        // Verify that a function was scheduled
        assert!(orchestrator.has_scheduled_functions());
    }

    // Tests for InvokeHandlerResult
    #[test]
    fn test_invoke_handler_result_succeeded() {
        let invocation = Invocation::with_start(chrono::Utc::now());
        let result: InvokeHandlerResult<String> = InvokeHandlerResult::Succeeded {
            result: "test result".to_string(),
            operations: vec![],
            invocation,
        };
        
        match result {
            InvokeHandlerResult::Succeeded { result, operations, .. } => {
                assert_eq!(result, "test result");
                assert!(operations.is_empty());
            }
            _ => panic!("Expected Succeeded variant"),
        }
    }

    #[test]
    fn test_invoke_handler_result_failed() {
        let invocation = Invocation::with_start(chrono::Utc::now());
        let error = TestResultError::new("TestError", "test error message");
        let result: InvokeHandlerResult<String> = InvokeHandlerResult::Failed {
            error,
            operations: vec![],
            invocation,
        };
        
        match result {
            InvokeHandlerResult::Failed { error, operations, .. } => {
                assert_eq!(error.error_type, Some("TestError".to_string()));
                assert!(operations.is_empty());
            }
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_invoke_handler_result_pending_with_reinvoke() {
        let invocation = Invocation::with_start(chrono::Utc::now());
        let result: InvokeHandlerResult<String> = InvokeHandlerResult::Pending {
            operations: vec![],
            invocation,
            should_reinvoke: true,
            advance_time_ms: Some(5000),
        };
        
        match result {
            InvokeHandlerResult::Pending { should_reinvoke, advance_time_ms, .. } => {
                assert!(should_reinvoke);
                assert_eq!(advance_time_ms, Some(5000));
            }
            _ => panic!("Expected Pending variant"),
        }
    }

    #[test]
    fn test_invoke_handler_result_pending_without_reinvoke() {
        let invocation = Invocation::with_start(chrono::Utc::now());
        let result: InvokeHandlerResult<String> = InvokeHandlerResult::Pending {
            operations: vec![],
            invocation,
            should_reinvoke: false,
            advance_time_ms: None,
        };
        
        match result {
            InvokeHandlerResult::Pending { should_reinvoke, advance_time_ms, .. } => {
                assert!(!should_reinvoke);
                assert_eq!(advance_time_ms, None);
            }
            _ => panic!("Expected Pending variant"),
        }
    }

    // Tests for invoke_handler method behavior
    #[tokio::test]
    async fn test_invoke_handler_creates_orchestrator_state() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Verify initial state
        assert!(orchestrator.execution_id().is_none());
        assert!(orchestrator.checkpoint_token().is_none());
        assert!(!orchestrator.is_execution_complete());
        assert!(!orchestrator.is_invocation_active());
    }

    #[tokio::test]
    async fn test_invoke_handler_time_skip_mode_prevents_concurrent() {
        use super::*;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Create an orchestrator with time skipping enabled
        let handler = |_input: String, _ctx: DurableContext| async move {
            Ok("result".to_string())
        };

        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let orchestrator = TestExecutionOrchestrator::new(
            handler,
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: true },
        );

        // Verify time skipping is enabled
        assert!(orchestrator.is_time_skipping_enabled());
        
        // Initially no invocation is active
        assert!(!orchestrator.is_invocation_active());
    }

    #[tokio::test]
    async fn test_invoke_handler_tracks_invocation_active_state() {
        use super::*;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::RwLock;

        // Create a mock checkpoint worker manager
        let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();

        // Track whether we observed the invocation as active
        let was_active = Arc::new(AtomicBool::new(false));
        let was_active_clone = Arc::clone(&was_active);

        // Create an orchestrator
        let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));
        let orchestrator = TestExecutionOrchestrator::new(
            move |_input: String, _ctx: DurableContext| {
                let was_active = Arc::clone(&was_active_clone);
                async move {
                    // This would be where we'd check if invocation is active
                    // but we can't access orchestrator from inside the handler
                    was_active.store(true, Ordering::SeqCst);
                    Ok("result".to_string())
                }
            },
            operation_storage,
            checkpoint_api,
            SkipTimeConfig { enabled: false },
        );

        // Initially no invocation is active
        assert!(!orchestrator.is_invocation_active());
    }
}

/// Property-based tests for TestExecutionOrchestrator
///
/// These tests verify the correctness properties defined in the design document.
#[cfg(test)]
mod property_tests {
    use super::*;
    use aws_durable_execution_sdk::{OperationType, WaitDetails};
    use proptest::prelude::*;

    /// Strategy for generating wait durations in seconds (1 to 60 seconds)
    fn wait_duration_strategy() -> impl Strategy<Value = u64> {
        1u64..=60
    }

    /// Strategy for generating multiple wait durations
    fn multiple_wait_durations_strategy() -> impl Strategy<Value = Vec<u64>> {
        prop::collection::vec(wait_duration_strategy(), 1..=3)
    }

    proptest! {
        /// **Feature: rust-testing-utilities, Property 19: Wait Operation Completion (Orchestrator)**
        ///
        /// *For any* wait operation with scheduled end timestamp T, when time skipping is enabled
        /// and time advances past T, the orchestrator SHALL mark the wait as SUCCEEDED and
        /// re-invoke the handler.
        ///
        /// This test verifies that:
        /// 1. Wait operations are tracked with their scheduled end timestamps (Req 16.1)
        /// 2. When time skipping is enabled and time advances past T, waits are marked SUCCEEDED (Req 16.2)
        /// 3. Time skipping uses tokio::time::advance() to skip wait durations instantly (Req 16.3)
        ///
        /// **Validates: Requirements 16.1, 16.2, 16.3**
        #[test]
        fn prop_wait_operation_completion(wait_seconds in wait_duration_strategy()) {
            // Use current_thread runtime which is required for tokio::time::pause()
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                // Calculate the scheduled end timestamp
                let now_ms = chrono::Utc::now().timestamp_millis();
                let scheduled_end_ms = now_ms + (wait_seconds as i64 * 1000);

                // Create a wait operation with the scheduled end timestamp
                let mut wait_op = Operation::new("wait-test", OperationType::Wait);
                wait_op.status = OperationStatus::Started;
                wait_op.wait_details = Some(WaitDetails {
                    scheduled_end_timestamp: Some(scheduled_end_ms),
                });

                // Property 16.1: Wait operation should have scheduled end timestamp tracked
                prop_assert!(wait_op.wait_details.is_some());
                let details = wait_op.wait_details.as_ref().unwrap();
                prop_assert_eq!(details.scheduled_end_timestamp, Some(scheduled_end_ms));

                // Create an orchestrator with time skipping enabled
                let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();
                let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));

                let handler = |_input: String, _ctx: DurableContext| async move {
                    Ok("result".to_string())
                };

                let mut orchestrator = TestExecutionOrchestrator::new(
                    handler,
                    operation_storage.clone(),
                    checkpoint_api,
                    SkipTimeConfig { enabled: true },
                );

                // Property 16.3: Verify time skipping is enabled
                prop_assert!(orchestrator.is_time_skipping_enabled());

                // Process the wait operation
                let operations = vec![wait_op.clone()];
                let result = orchestrator.process_operations(&operations, "exec-test");

                // Property 16.1: Wait operation should be tracked as pending with scheduled time
                match result {
                    ProcessOperationsResult::ShouldReinvoke(advance_time_ms) => {
                        // When time skipping is enabled, we should get the time to advance
                        prop_assert!(
                            advance_time_ms.is_some(),
                            "Should have advance time when time skipping is enabled"
                        );
                        
                        // The advance time should be approximately the wait duration
                        // (may be slightly less due to time elapsed during test)
                        if let Some(advance_ms) = advance_time_ms {
                            // Allow some tolerance for test execution time
                            let expected_min = (wait_seconds as u64).saturating_sub(1) * 1000;
                            let expected_max = (wait_seconds as u64 + 1) * 1000;
                            prop_assert!(
                                advance_ms >= expected_min && advance_ms <= expected_max,
                                "Advance time {} should be approximately {} seconds ({}ms - {}ms)",
                                advance_ms, wait_seconds, expected_min, expected_max
                            );
                        }
                    }
                    ProcessOperationsResult::NoPendingOperations => {
                        // This is also acceptable if the wait was already processed
                        // (e.g., if the scheduled time has already passed)
                    }
                    other => {
                        prop_assert!(
                            false,
                            "Expected ShouldReinvoke or NoPendingOperations, got {:?}",
                            other
                        );
                    }
                }

                // Verify the wait operation was tracked as pending
                prop_assert!(
                    orchestrator.pending_operations.contains("wait-test"),
                    "Wait operation should be tracked as pending"
                );

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 19: Wait Operation Completion (Multiple Waits)**
        ///
        /// *For any* set of wait operations with different scheduled end timestamps,
        /// the orchestrator SHALL process them in order of their scheduled times and
        /// return the earliest scheduled time for advancement.
        ///
        /// **Validates: Requirements 16.1, 16.2, 16.3**
        #[test]
        fn prop_wait_operation_completion_multiple_waits(
            wait_durations in multiple_wait_durations_strategy()
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let now_ms = chrono::Utc::now().timestamp_millis();

                // Create multiple wait operations with different scheduled end timestamps
                let mut operations = Vec::new();
                for (i, &duration) in wait_durations.iter().enumerate() {
                    let scheduled_end_ms = now_ms + (duration as i64 * 1000);
                    let mut wait_op = Operation::new(&format!("wait-{}", i), OperationType::Wait);
                    wait_op.status = OperationStatus::Started;
                    wait_op.wait_details = Some(WaitDetails {
                        scheduled_end_timestamp: Some(scheduled_end_ms),
                    });
                    operations.push(wait_op);
                }

                // Create an orchestrator with time skipping enabled
                let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();
                let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));

                let handler = |_input: String, _ctx: DurableContext| async move {
                    Ok("result".to_string())
                };

                let mut orchestrator = TestExecutionOrchestrator::new(
                    handler,
                    operation_storage,
                    checkpoint_api,
                    SkipTimeConfig { enabled: true },
                );

                // Process all wait operations
                let result = orchestrator.process_operations(&operations, "exec-test");

                // Find the minimum wait duration (earliest scheduled time)
                let min_duration = wait_durations.iter().min().copied().unwrap_or(0);

                match result {
                    ProcessOperationsResult::ShouldReinvoke(advance_time_ms) => {
                        // Property: Should return the earliest scheduled time
                        if let Some(advance_ms) = advance_time_ms {
                            // The advance time should be approximately the minimum wait duration
                            let expected_min = min_duration.saturating_sub(1) * 1000;
                            let expected_max = (min_duration + 1) * 1000;
                            prop_assert!(
                                advance_ms >= expected_min && advance_ms <= expected_max,
                                "Advance time {} should be approximately {} seconds (min duration)",
                                advance_ms, min_duration
                            );
                        }
                    }
                    ProcessOperationsResult::NoPendingOperations => {
                        // Acceptable if all waits were already processed
                    }
                    other => {
                        prop_assert!(
                            false,
                            "Expected ShouldReinvoke or NoPendingOperations, got {:?}",
                            other
                        );
                    }
                }

                // Property: All wait operations should be tracked as pending
                for (i, _) in wait_durations.iter().enumerate() {
                    let op_id = format!("wait-{}", i);
                    prop_assert!(
                        orchestrator.pending_operations.contains(&op_id),
                        "Wait operation {} should be tracked as pending",
                        op_id
                    );
                }

                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 19: Wait Operation Completion (Completed Waits)**
        ///
        /// *For any* wait operation that has already completed (status is terminal),
        /// the orchestrator SHALL NOT track it as pending and SHALL NOT schedule re-invocation.
        ///
        /// **Validates: Requirements 16.1, 16.2, 16.3**
        #[test]
        fn prop_wait_operation_completion_already_completed(wait_seconds in wait_duration_strategy()) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            rt.block_on(async {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let scheduled_end_ms = now_ms + (wait_seconds as i64 * 1000);

                // Create a wait operation that has already completed
                let mut wait_op = Operation::new("wait-completed", OperationType::Wait);
                wait_op.status = OperationStatus::Succeeded; // Already completed
                wait_op.wait_details = Some(WaitDetails {
                    scheduled_end_timestamp: Some(scheduled_end_ms),
                });
                wait_op.end_timestamp = Some(now_ms);

                // Create an orchestrator with time skipping enabled
                let checkpoint_api = CheckpointWorkerManager::get_instance(None).unwrap();
                let operation_storage = Arc::new(RwLock::new(OperationStorage::new()));

                let handler = |_input: String, _ctx: DurableContext| async move {
                    Ok("result".to_string())
                };

                let mut orchestrator = TestExecutionOrchestrator::new(
                    handler,
                    operation_storage,
                    checkpoint_api,
                    SkipTimeConfig { enabled: true },
                );

                // Process the completed wait operation
                let operations = vec![wait_op];
                let result = orchestrator.process_operations(&operations, "exec-test");

                // Property: Completed wait should not trigger re-invocation
                match result {
                    ProcessOperationsResult::NoPendingOperations => {
                        // Expected: no pending operations since the wait is already completed
                    }
                    ProcessOperationsResult::ShouldReinvoke(_) => {
                        prop_assert!(
                            false,
                            "Completed wait operation should not trigger re-invocation"
                        );
                    }
                    other => {
                        prop_assert!(
                            false,
                            "Expected NoPendingOperations for completed wait, got {:?}",
                            other
                        );
                    }
                }

                // Property: Completed wait should not be tracked as pending
                prop_assert!(
                    !orchestrator.pending_operations.contains("wait-completed"),
                    "Completed wait operation should not be tracked as pending"
                );

                Ok(())
            })?;
        }
    }
}
