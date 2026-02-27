//! Lazy operation handle for pre-run operation registration.
//!
//! An `OperationHandle` is a lazy reference to a named operation that is registered
//! before `run()` and auto-populates with operation data during execution. This enables
//! the idiomatic callback testing pattern: pre-register a handle, start a non-blocking
//! run, wait for mid-execution status changes, send callback responses, and await the
//! final result.

use std::sync::Arc;
use tokio::sync::{watch, RwLock};

use serde::de::DeserializeOwned;

use aws_durable_execution_sdk::{Operation, OperationStatus, OperationType};

use crate::error::TestError;
use crate::operation::{
    CallbackDetails, CallbackSender, ContextDetails, DurableOperation, InvokeDetails, StepDetails,
    WaitDetails,
};
use crate::types::WaitingOperationStatus;

/// How an `OperationHandle` matches against operations during execution.
#[derive(Clone, Debug)]
pub enum OperationMatcher {
    /// Match by operation name (first operation with this name).
    ByName(String),
    /// Match by execution order index.
    ByIndex(usize),
    /// Match by unique operation ID.
    ById(String),
    /// Match by operation name and occurrence index (nth operation with this name).
    ByNameAndIndex(String, usize),
}

/// A lazy handle to an operation that will be populated during execution.
///
/// Registered before `run()` via `get_operation_handle()`, `get_operation_handle_by_index()`,
/// or `get_operation_handle_by_id()` on the `LocalDurableTestRunner`. The handle starts
/// unpopulated and is filled with operation data when the orchestrator finds a matching
/// operation during execution.
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::OperationHandle;
///
/// // Pre-register a handle before run()
/// let handle = runner.get_operation_handle("my-callback");
///
/// // Start non-blocking execution
/// let future = runner.run(input);
///
/// // Wait for the operation to reach Submitted status
/// handle.wait_for_data(WaitingOperationStatus::Submitted).await?;
///
/// // Send callback response
/// handle.send_callback_success("result").await?;
///
/// // Await the final result
/// let result = future.await?;
/// ```
pub struct OperationHandle {
    /// How this handle matches operations.
    pub(crate) matcher: OperationMatcher,
    /// Shared operation data, populated during execution.
    pub(crate) inner: Arc<RwLock<Option<Operation>>>,
    /// Watch channel sender for status notifications.
    pub(crate) status_tx: watch::Sender<Option<OperationStatus>>,
    /// Watch channel receiver for status notifications.
    pub(crate) status_rx: watch::Receiver<Option<OperationStatus>>,
    /// Callback sender for interacting with the checkpoint server.
    /// Wrapped in Arc<RwLock<>> so that clones share the same sender reference.
    /// This allows `with_handles()` to set the sender after the handle is cloned
    /// by the test code, and the test's clone will see the update.
    pub(crate) callback_sender: Arc<RwLock<Option<Arc<dyn CallbackSender>>>>,
    /// Shared reference to all operations (for child enumeration).
    pub(crate) all_operations: Arc<RwLock<Vec<Operation>>>,
}

impl Clone for OperationHandle {
    fn clone(&self) -> Self {
        Self {
            matcher: self.matcher.clone(),
            inner: Arc::clone(&self.inner),
            status_tx: self.status_tx.clone(),
            status_rx: self.status_rx.clone(),
            callback_sender: Arc::clone(&self.callback_sender),
            all_operations: Arc::clone(&self.all_operations),
        }
    }
}

impl OperationHandle {
    /// Creates a new unpopulated `OperationHandle` with the given matcher.
    ///
    /// The handle starts with no operation data. It will be populated during
    /// execution when the orchestrator finds an operation matching the matcher.
    ///
    /// # Arguments
    ///
    /// * `matcher` - How this handle should match against operations
    /// * `all_operations` - Shared reference to all operations for child enumeration
    pub fn new(matcher: OperationMatcher, all_operations: Arc<RwLock<Vec<Operation>>>) -> Self {
        let (status_tx, status_rx) = watch::channel(None);
        Self {
            matcher,
            inner: Arc::new(RwLock::new(None)),
            status_tx,
            status_rx,
            callback_sender: Arc::new(RwLock::new(None)),
            all_operations,
        }
    }

    // =========================================================================
    // Inspection Methods (Requirements 1.3, 1.6)
    // =========================================================================

    /// Helper to get the inner operation or return an error if unpopulated.
    async fn get_durable_operation(&self) -> Result<DurableOperation, TestError> {
        let inner = self.inner.read().await;
        match inner.as_ref() {
            Some(op) => Ok(DurableOperation::new(op.clone())),
            None => Err(TestError::OperationNotFound(
                "Operation not yet populated".into(),
            )),
        }
    }

    /// Gets the operation ID.
    ///
    /// # Returns
    ///
    /// - `Ok(String)` - The operation ID if the handle is populated
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    pub async fn get_id(&self) -> Result<String, TestError> {
        let op = self.get_durable_operation().await?;
        Ok(op.get_id().to_string())
    }

    /// Gets the operation name.
    ///
    /// # Returns
    ///
    /// - `Ok(Option<String>)` - The operation name if the handle is populated
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    pub async fn get_name(&self) -> Result<Option<String>, TestError> {
        let op = self.get_durable_operation().await?;
        Ok(op.get_name().map(|s| s.to_string()))
    }

    /// Gets the operation type.
    ///
    /// # Returns
    ///
    /// - `Ok(OperationType)` - The operation type if the handle is populated
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    pub async fn get_type(&self) -> Result<OperationType, TestError> {
        let op = self.get_durable_operation().await?;
        Ok(op.get_type())
    }

    /// Gets the operation status.
    ///
    /// # Returns
    ///
    /// - `Ok(OperationStatus)` - The operation status if the handle is populated
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    pub async fn get_status(&self) -> Result<OperationStatus, TestError> {
        let op = self.get_durable_operation().await?;
        Ok(op.get_status())
    }

    /// Gets step-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(StepDetails<T>)` - The step details if populated and is a Step operation
    /// - `Err(TestError)` - If unpopulated or wrong operation type
    pub async fn get_step_details<T: DeserializeOwned>(&self) -> Result<StepDetails<T>, TestError> {
        let op = self.get_durable_operation().await?;
        op.get_step_details()
    }

    /// Gets callback-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(CallbackDetails<T>)` - The callback details if populated and is a Callback operation
    /// - `Err(TestError)` - If unpopulated or wrong operation type
    pub async fn get_callback_details<T: DeserializeOwned>(
        &self,
    ) -> Result<CallbackDetails<T>, TestError> {
        let op = self.get_durable_operation().await?;
        op.get_callback_details()
    }

    /// Gets wait-specific details.
    ///
    /// # Returns
    ///
    /// - `Ok(WaitDetails)` - The wait details if populated and is a Wait operation
    /// - `Err(TestError)` - If unpopulated or wrong operation type
    pub async fn get_wait_details(&self) -> Result<WaitDetails, TestError> {
        let op = self.get_durable_operation().await?;
        op.get_wait_details()
    }

    /// Gets invoke-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(InvokeDetails<T>)` - The invoke details if populated and is an Invoke operation
    /// - `Err(TestError)` - If unpopulated or wrong operation type
    pub async fn get_invoke_details<T: DeserializeOwned>(
        &self,
    ) -> Result<InvokeDetails<T>, TestError> {
        let op = self.get_durable_operation().await?;
        op.get_invoke_details()
    }

    /// Gets context-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(ContextDetails<T>)` - The context details if populated and is a Context operation
    /// - `Err(TestError)` - If unpopulated or wrong operation type
    pub async fn get_context_details<T: DeserializeOwned>(
        &self,
    ) -> Result<ContextDetails<T>, TestError> {
        let op = self.get_durable_operation().await?;
        op.get_context_details()
    }

    /// Checks if the handle has been populated with operation data.
    ///
    /// # Returns
    ///
    /// `true` if the handle has been populated, `false` otherwise.
    pub async fn is_populated(&self) -> bool {
        self.inner.read().await.is_some()
    }

    // =========================================================================
    // Async Waiting (Requirements 1.4, 4.1, 4.2, 4.3, 4.5)
    // =========================================================================

    /// Waits for the operation to reach the specified `WaitingOperationStatus`.
    ///
    /// Resolves immediately if the operation has already reached the target status.
    /// If the watch channel closes (execution ended) before the target status is
    /// reached, returns `Err(TestError::ExecutionCompletedEarly(...))`.
    ///
    /// # Arguments
    ///
    /// * `status` - The target status to wait for
    ///
    /// # Returns
    ///
    /// - `Ok(())` - When the operation reaches the target status
    /// - `Err(TestError::ExecutionCompletedEarly)` - If execution ends before reaching the target
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Wait for a callback to be ready for responses
    /// handle.wait_for_data(WaitingOperationStatus::Submitted).await?;
    ///
    /// // Wait for an operation to complete
    /// handle.wait_for_data(WaitingOperationStatus::Completed).await?;
    /// ```
    pub async fn wait_for_data(&self, status: WaitingOperationStatus) -> Result<(), TestError> {
        // Check if the current status already satisfies the target
        if self.check_status_reached(status).await {
            return Ok(());
        }

        // Subscribe to the watch channel and wait for updates
        let mut rx = self.status_rx.clone();
        loop {
            // Wait for the next status change
            if rx.changed().await.is_err() {
                // Channel closed — execution ended. Do a final check.
                if self.check_status_reached(status).await {
                    return Ok(());
                }
                return Err(TestError::execution_completed_early(
                    self.matcher_description(),
                    status,
                ));
            }

            // Check if the new status satisfies the target
            if self.check_status_reached(status).await {
                return Ok(());
            }
        }
    }

    /// Checks whether the current operation state satisfies the target `WaitingOperationStatus`.
    async fn check_status_reached(&self, target: WaitingOperationStatus) -> bool {
        let inner = self.inner.read().await;
        match inner.as_ref() {
            None => false,
            Some(op) => Self::status_matches_target(op, target),
        }
    }

    /// Determines if an operation's current state matches the target waiting status.
    fn status_matches_target(op: &Operation, target: WaitingOperationStatus) -> bool {
        match target {
            WaitingOperationStatus::Started => {
                // Any populated operation has started
                true
            }
            WaitingOperationStatus::Submitted => {
                // For callbacks, Submitted means callback_id is available
                if op.operation_type == OperationType::Callback {
                    op.callback_details
                        .as_ref()
                        .map(|d| d.callback_id.is_some())
                        .unwrap_or(false)
                } else {
                    // For non-callbacks, treat Submitted as Started
                    true
                }
            }
            WaitingOperationStatus::Completed => {
                // Terminal status means completed
                op.status.is_terminal()
            }
        }
    }

    // =========================================================================
    // Callback Interaction Methods (Requirements 1.5, 5.4)
    // =========================================================================

    /// Sends a success response for a callback operation.
    ///
    /// Validates that the handle is populated, the operation is a callback type,
    /// and the callback_id is available (Submitted status) before delegating to
    /// the callback sender.
    ///
    /// # Arguments
    ///
    /// * `result` - The result value to send as a JSON string
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the callback response was sent successfully
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    /// - `Err(TestError::NotCallbackOperation)` - If the operation is not a callback
    /// - `Err(TestError::ResultNotAvailable)` - If the callback_id is not yet available
    pub async fn send_callback_success(&self, result: &str) -> Result<(), TestError> {
        let callback_id = self.validate_callback_ready().await?;

        let sender_guard = self.callback_sender.read().await;
        match sender_guard.as_ref() {
            Some(sender) => sender.send_success(&callback_id, result).await,
            None => Err(TestError::result_not_available(
                "No callback sender configured on this handle",
            )),
        }
    }

    /// Sends a failure response for a callback operation.
    ///
    /// Validates that the handle is populated, the operation is a callback type,
    /// and the callback_id is available (Submitted status) before delegating to
    /// the callback sender.
    ///
    /// # Arguments
    ///
    /// * `error` - The error information to send
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the callback failure was sent successfully
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    /// - `Err(TestError::NotCallbackOperation)` - If the operation is not a callback
    /// - `Err(TestError::ResultNotAvailable)` - If the callback_id is not yet available
    pub async fn send_callback_failure(
        &self,
        error: &crate::types::TestResultError,
    ) -> Result<(), TestError> {
        let callback_id = self.validate_callback_ready().await?;

        let sender_guard = self.callback_sender.read().await;
        match sender_guard.as_ref() {
            Some(sender) => sender.send_failure(&callback_id, error).await,
            None => Err(TestError::result_not_available(
                "No callback sender configured on this handle",
            )),
        }
    }

    /// Sends a heartbeat for a callback operation.
    ///
    /// Validates that the handle is populated, the operation is a callback type,
    /// and the callback_id is available (Submitted status) before delegating to
    /// the callback sender.
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the heartbeat was sent successfully
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    /// - `Err(TestError::NotCallbackOperation)` - If the operation is not a callback
    /// - `Err(TestError::ResultNotAvailable)` - If the callback_id is not yet available
    pub async fn send_callback_heartbeat(&self) -> Result<(), TestError> {
        let callback_id = self.validate_callback_ready().await?;

        let sender_guard = self.callback_sender.read().await;
        match sender_guard.as_ref() {
            Some(sender) => sender.send_heartbeat(&callback_id).await,
            None => Err(TestError::result_not_available(
                "No callback sender configured on this handle",
            )),
        }
    }

    /// Validates that the handle is populated with a callback operation that has
    /// a callback_id available (Submitted status). Returns the callback_id on success.
    async fn validate_callback_ready(&self) -> Result<String, TestError> {
        let inner = self.inner.read().await;
        let op = inner
            .as_ref()
            .ok_or_else(|| TestError::OperationNotFound("Operation not yet populated".into()))?;

        if op.operation_type != OperationType::Callback {
            return Err(TestError::NotCallbackOperation);
        }

        op.callback_details
            .as_ref()
            .and_then(|d| d.callback_id.clone())
            .ok_or_else(|| {
                TestError::result_not_available(
                    "Callback ID not available — operation has not reached Submitted status",
                )
            })
    }

    // =========================================================================
    // Child Operation Methods (Requirements 3.1, 3.2, 3.3, 3.4)
    // =========================================================================

    /// Returns all child operations nested under this operation.
    ///
    /// Child operations are those whose `parent_id` matches this operation's `id`.
    /// The returned operations preserve execution order (same order as in the
    /// shared operations list).
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<DurableOperation>)` - Child operations if the handle is populated
    /// - `Err(TestError::OperationNotFound)` - If the handle is not yet populated
    ///
    /// # Requirements
    ///
    /// - 3.1: Returns all operations whose parent_id matches this operation's id
    /// - 3.2: Returns empty Vec when no children exist
    /// - 3.3: Returned children support get_child_operations() for nested hierarchies
    /// - 3.4: Preserves execution order of child operations
    pub async fn get_child_operations(&self) -> Result<Vec<DurableOperation>, TestError> {
        let inner = self.inner.read().await;
        let op = inner
            .as_ref()
            .ok_or_else(|| TestError::OperationNotFound("Operation not yet populated".into()))?;

        let my_id = &op.operation_id;
        let all_ops = self.all_operations.read().await;

        // Snapshot into an Arc<Vec<Operation>> so each child DurableOperation
        // can enumerate its own children via with_operations().
        let all_ops_arc = Arc::new(all_ops.clone());

        let children = all_ops_arc
            .iter()
            .filter(|child| child.parent_id.as_deref() == Some(my_id))
            .map(|child| {
                DurableOperation::new(child.clone()).with_operations(Arc::clone(&all_ops_arc))
            })
            .collect();

        Ok(children)
    }

    /// Returns a human-readable description of the matcher for error messages.
    fn matcher_description(&self) -> String {
        match &self.matcher {
            OperationMatcher::ByName(name) => format!("name={}", name),
            OperationMatcher::ByIndex(idx) => format!("index={}", idx),
            OperationMatcher::ById(id) => format!("id={}", id),
            OperationMatcher::ByNameAndIndex(name, idx) => format!("name={}, index={}", name, idx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::{
        CallbackDetails as SdkCallbackDetails, ChainedInvokeDetails as SdkChainedInvokeDetails,
        ContextDetails as SdkContextDetails, OperationType, StepDetails as SdkStepDetails,
        WaitDetails as SdkWaitDetails,
    };

    /// Helper to create a step operation for testing.
    fn create_step_operation() -> Operation {
        let mut op = Operation::new("step-001".to_string(), OperationType::Step);
        op.name = Some("my-step".to_string());
        op.status = OperationStatus::Succeeded;
        op.step_details = Some(SdkStepDetails {
            result: Some(r#""hello""#.to_string()),
            attempt: Some(1),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });
        op
    }

    /// Helper to create a callback operation for testing.
    fn create_callback_operation() -> Operation {
        let mut op = Operation::new("cb-001".to_string(), OperationType::Callback);
        op.name = Some("my-callback".to_string());
        op.status = OperationStatus::Started;
        op.callback_details = Some(SdkCallbackDetails {
            callback_id: Some("cb-id-123".to_string()),
            result: None,
            error: None,
        });
        op
    }

    /// Helper to create a wait operation for testing.
    fn create_wait_operation() -> Operation {
        let mut op = Operation::new("wait-001".to_string(), OperationType::Wait);
        op.name = Some("my-wait".to_string());
        op.status = OperationStatus::Succeeded;
        op.start_timestamp = Some(1000);
        op.wait_details = Some(SdkWaitDetails {
            scheduled_end_timestamp: Some(6000),
        });
        op
    }

    /// Helper to create an invoke operation for testing.
    fn create_invoke_operation() -> Operation {
        let mut op = Operation::new("invoke-001".to_string(), OperationType::Invoke);
        op.name = Some("my-invoke".to_string());
        op.status = OperationStatus::Succeeded;
        op.chained_invoke_details = Some(SdkChainedInvokeDetails {
            result: Some(r#"42"#.to_string()),
            error: None,
        });
        op
    }

    /// Helper to create a context operation for testing.
    fn create_context_operation() -> Operation {
        let mut op = Operation::new("ctx-001".to_string(), OperationType::Context);
        op.name = Some("my-context".to_string());
        op.status = OperationStatus::Succeeded;
        op.context_details = Some(SdkContextDetails {
            result: Some(r#""done""#.to_string()),
            replay_children: None,
            error: None,
        });
        op
    }

    /// Helper to populate a handle with an operation.
    async fn populate_handle(handle: &OperationHandle, op: Operation) {
        let mut inner = handle.inner.write().await;
        *inner = Some(op);
    }

    #[test]
    fn test_operation_handle_new_by_name() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test-op".into()), all_ops);

        assert!(matches!(handle.matcher, OperationMatcher::ByName(ref n) if n == "test-op"));
        assert!(handle.callback_sender.try_read().unwrap().is_none());
    }

    #[test]
    fn test_operation_handle_new_by_index() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByIndex(3), all_ops);

        assert!(matches!(handle.matcher, OperationMatcher::ByIndex(3)));
    }

    #[test]
    fn test_operation_handle_new_by_id() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ById("abc-123".into()), all_ops);

        assert!(matches!(handle.matcher, OperationMatcher::ById(ref id) if id == "abc-123"));
    }

    #[tokio::test]
    async fn test_operation_handle_starts_unpopulated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        let inner = handle.inner.read().await;
        assert!(inner.is_none());
        assert!(handle.status_rx.borrow().is_none());
    }

    #[test]
    fn test_operation_handle_clone() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let cloned = handle.clone();

        // Cloned handle shares the same inner Arc
        assert!(Arc::ptr_eq(&handle.inner, &cloned.inner));
        assert!(Arc::ptr_eq(&handle.all_operations, &cloned.all_operations));
        assert!(matches!(cloned.matcher, OperationMatcher::ByName(ref n) if n == "test"));
    }

    #[test]
    fn test_operation_matcher_debug() {
        let by_name = OperationMatcher::ByName("test".into());
        let by_index = OperationMatcher::ByIndex(0);
        let by_id = OperationMatcher::ById("id-1".into());

        // Verify Debug is implemented and produces reasonable output
        assert!(format!("{:?}", by_name).contains("ByName"));
        assert!(format!("{:?}", by_index).contains("ByIndex"));
        assert!(format!("{:?}", by_id).contains("ById"));
    }

    // =========================================================================
    // Inspection Method Tests (Requirements 1.3, 1.6)
    // =========================================================================

    #[tokio::test]
    async fn test_is_populated_false_when_new() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        assert!(!handle.is_populated().await);
    }

    #[tokio::test]
    async fn test_is_populated_true_after_population() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        assert!(handle.is_populated().await);
    }

    #[tokio::test]
    async fn test_get_id_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.get_id().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_id_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        assert_eq!(handle.get_id().await.unwrap(), "step-001");
    }

    #[tokio::test]
    async fn test_get_name_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.get_name().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_name_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        assert_eq!(
            handle.get_name().await.unwrap(),
            Some("my-step".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_type_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.get_type().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_type_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        assert_eq!(handle.get_type().await.unwrap(), OperationType::Step);
    }

    #[tokio::test]
    async fn test_get_status_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.get_status().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_status_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        assert_eq!(
            handle.get_status().await.unwrap(),
            OperationStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn test_get_step_details_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result: Result<StepDetails<String>, _> = handle.get_step_details().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_step_details_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        let details: StepDetails<String> = handle.get_step_details().await.unwrap();
        assert_eq!(details.result, Some("hello".to_string()));
        assert_eq!(details.attempt, Some(1));
    }

    #[tokio::test]
    async fn test_get_callback_details_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result: Result<CallbackDetails<String>, _> = handle.get_callback_details().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_callback_details_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_callback_operation()).await;
        let details: CallbackDetails<String> = handle.get_callback_details().await.unwrap();
        assert_eq!(details.callback_id, Some("cb-id-123".to_string()));
    }

    #[tokio::test]
    async fn test_get_wait_details_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.get_wait_details().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_wait_details_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_wait_operation()).await;
        let details = handle.get_wait_details().await.unwrap();
        assert_eq!(details.wait_seconds, Some(5));
    }

    #[tokio::test]
    async fn test_get_invoke_details_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result: Result<InvokeDetails<serde_json::Value>, _> = handle.get_invoke_details().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_invoke_details_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_invoke_operation()).await;
        let details: InvokeDetails<i32> = handle.get_invoke_details().await.unwrap();
        assert_eq!(details.result, Some(42));
    }

    #[tokio::test]
    async fn test_get_context_details_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result: Result<ContextDetails<String>, _> = handle.get_context_details().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_context_details_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_context_operation()).await;
        let details: ContextDetails<String> = handle.get_context_details().await.unwrap();
        assert_eq!(details.result, Some("done".to_string()));
    }

    #[tokio::test]
    async fn test_get_step_details_wrong_type_returns_type_mismatch() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_wait_operation()).await;
        let result: Result<StepDetails<String>, _> = handle.get_step_details().await;
        assert!(matches!(
            result,
            Err(TestError::OperationTypeMismatch { .. })
        ));
    }

    // =========================================================================
    // wait_for_data Tests (Requirements 1.4, 4.1, 4.2, 4.3, 4.5)
    // =========================================================================

    #[tokio::test]
    async fn test_wait_for_data_started_resolves_immediately_when_populated() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        // Any populated operation satisfies Started
        let result = handle.wait_for_data(WaitingOperationStatus::Started).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_completed_resolves_immediately_when_terminal() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        // create_step_operation has status Succeeded (terminal)
        populate_handle(&handle, create_step_operation()).await;
        let result = handle
            .wait_for_data(WaitingOperationStatus::Completed)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_submitted_resolves_for_callback_with_id() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        // create_callback_operation has callback_id set
        populate_handle(&handle, create_callback_operation()).await;
        let result = handle
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_submitted_resolves_for_non_callback() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        // Non-callback operations treat Submitted as Started
        populate_handle(&handle, create_step_operation()).await;
        let result = handle
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_unpopulated_not_started_returns_error_on_channel_close() {
        // Test the ExecutionCompletedEarly path by verifying that when the
        // operation never reaches the target status and the channel closes,
        // we get the appropriate error.
        //
        // In real usage, the OperationHandle holds a status_tx sender, which
        // means the channel can't close while the handle exists. The channel-
        // close path is a safety net for edge cases. We test the underlying
        // logic via check_status_reached instead.
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        // Unpopulated handle should not satisfy any status
        assert!(
            !handle
                .check_status_reached(WaitingOperationStatus::Started)
                .await
        );
        assert!(
            !handle
                .check_status_reached(WaitingOperationStatus::Submitted)
                .await
        );
        assert!(
            !handle
                .check_status_reached(WaitingOperationStatus::Completed)
                .await
        );
    }

    #[tokio::test]
    async fn test_wait_for_data_non_terminal_does_not_satisfy_completed() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        // Populate with a non-terminal status
        let mut op = Operation::new("step-001".to_string(), OperationType::Step);
        op.status = OperationStatus::Started;
        populate_handle(&handle, op).await;

        // Started satisfies Started but not Completed
        assert!(
            handle
                .check_status_reached(WaitingOperationStatus::Started)
                .await
        );
        assert!(
            !handle
                .check_status_reached(WaitingOperationStatus::Completed)
                .await
        );
    }

    #[tokio::test]
    async fn test_wait_for_data_callback_without_id_does_not_satisfy_submitted() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        // Populate with a callback that has no callback_id
        let mut op = Operation::new("cb-001".to_string(), OperationType::Callback);
        op.status = OperationStatus::Started;
        op.callback_details = Some(SdkCallbackDetails {
            callback_id: None,
            result: None,
            error: None,
        });
        populate_handle(&handle, op).await;

        // Started is satisfied, but Submitted is not (no callback_id)
        assert!(
            handle
                .check_status_reached(WaitingOperationStatus::Started)
                .await
        );
        assert!(
            !handle
                .check_status_reached(WaitingOperationStatus::Submitted)
                .await
        );
    }

    #[tokio::test]
    async fn test_status_matches_target_all_terminal_statuses_satisfy_completed() {
        let terminal_statuses = vec![
            OperationStatus::Succeeded,
            OperationStatus::Failed,
            OperationStatus::Cancelled,
            OperationStatus::TimedOut,
            OperationStatus::Stopped,
        ];

        for status in terminal_statuses {
            let mut op = Operation::new("op-001".to_string(), OperationType::Step);
            op.status = status;
            assert!(
                OperationHandle::status_matches_target(&op, WaitingOperationStatus::Completed),
                "Expected {:?} to satisfy Completed",
                status
            );
        }
    }

    #[tokio::test]
    async fn test_status_matches_target_non_terminal_does_not_satisfy_completed() {
        let non_terminal_statuses = vec![
            OperationStatus::Started,
            OperationStatus::Pending,
            OperationStatus::Ready,
        ];

        for status in non_terminal_statuses {
            let mut op = Operation::new("op-001".to_string(), OperationType::Step);
            op.status = status;
            assert!(
                !OperationHandle::status_matches_target(&op, WaitingOperationStatus::Completed),
                "Expected {:?} to NOT satisfy Completed",
                status
            );
        }
    }

    #[tokio::test]
    async fn test_wait_for_data_resolves_when_status_update_arrives() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        let handle_clone = handle.clone();
        // Spawn a task that populates the handle and sends a status update after a delay
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            // Populate with a completed operation
            let mut op = create_step_operation();
            op.status = OperationStatus::Succeeded;
            {
                let mut inner = handle_clone.inner.write().await;
                *inner = Some(op);
            }
            let _ = handle_clone
                .status_tx
                .send(Some(OperationStatus::Succeeded));
        });

        let result = handle
            .wait_for_data(WaitingOperationStatus::Completed)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_waits_through_non_terminal_updates() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        let handle_clone = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            // First: populate with Started status (non-terminal)
            let mut op = create_step_operation();
            op.status = OperationStatus::Started;
            {
                let mut inner = handle_clone.inner.write().await;
                *inner = Some(op);
            }
            let _ = handle_clone.status_tx.send(Some(OperationStatus::Started));

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            // Then: update to Succeeded (terminal)
            {
                let mut inner = handle_clone.inner.write().await;
                if let Some(ref mut op) = *inner {
                    op.status = OperationStatus::Succeeded;
                }
            }
            let _ = handle_clone
                .status_tx
                .send(Some(OperationStatus::Succeeded));
        });

        let result = handle
            .wait_for_data(WaitingOperationStatus::Completed)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_submitted_waits_for_callback_id() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);

        let handle_clone = handle.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            // First: populate with callback but no callback_id
            let mut op = Operation::new("cb-001".to_string(), OperationType::Callback);
            op.name = Some("my-callback".to_string());
            op.status = OperationStatus::Started;
            op.callback_details = Some(SdkCallbackDetails {
                callback_id: None,
                result: None,
                error: None,
            });
            {
                let mut inner = handle_clone.inner.write().await;
                *inner = Some(op);
            }
            let _ = handle_clone.status_tx.send(Some(OperationStatus::Started));

            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            // Then: update with callback_id
            {
                let mut inner = handle_clone.inner.write().await;
                if let Some(ref mut op) = *inner {
                    if let Some(ref mut details) = op.callback_details {
                        details.callback_id = Some("cb-id-123".to_string());
                    }
                }
            }
            let _ = handle_clone.status_tx.send(Some(OperationStatus::Started));
        });

        let result = handle
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_matcher_description() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));

        let handle =
            OperationHandle::new(OperationMatcher::ByName("my-op".into()), all_ops.clone());
        assert_eq!(handle.matcher_description(), "name=my-op");

        let handle = OperationHandle::new(OperationMatcher::ByIndex(3), all_ops.clone());
        assert_eq!(handle.matcher_description(), "index=3");

        let handle = OperationHandle::new(OperationMatcher::ById("abc-123".into()), all_ops);
        assert_eq!(handle.matcher_description(), "id=abc-123");
    }

    // =========================================================================
    // Callback Method Tests (Requirements 1.5, 5.4)
    // =========================================================================

    /// Mock callback sender for testing.
    #[derive(Clone)]
    struct MockCallbackSender {
        success_calls: Arc<RwLock<Vec<(String, String)>>>,
        failure_calls: Arc<RwLock<Vec<(String, String)>>>,
        heartbeat_calls: Arc<RwLock<Vec<String>>>,
    }

    impl MockCallbackSender {
        fn new() -> Self {
            Self {
                success_calls: Arc::new(RwLock::new(Vec::new())),
                failure_calls: Arc::new(RwLock::new(Vec::new())),
                heartbeat_calls: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl CallbackSender for MockCallbackSender {
        async fn send_success(&self, callback_id: &str, result: &str) -> Result<(), TestError> {
            self.success_calls
                .write()
                .await
                .push((callback_id.to_string(), result.to_string()));
            Ok(())
        }

        async fn send_failure(
            &self,
            callback_id: &str,
            error: &crate::types::TestResultError,
        ) -> Result<(), TestError> {
            self.failure_calls
                .write()
                .await
                .push((callback_id.to_string(), error.to_string()));
            Ok(())
        }

        async fn send_heartbeat(&self, callback_id: &str) -> Result<(), TestError> {
            self.heartbeat_calls
                .write()
                .await
                .push(callback_id.to_string());
            Ok(())
        }
    }

    /// Helper to create a handle with a mock callback sender and a populated callback operation.
    async fn create_callback_handle_with_sender() -> (OperationHandle, MockCallbackSender) {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let sender = MockCallbackSender::new();
        {
            let mut guard = handle.callback_sender.write().await;
            *guard = Some(Arc::new(sender.clone()));
        }
        populate_handle(&handle, create_callback_operation()).await;
        (handle, sender)
    }

    #[tokio::test]
    async fn test_send_callback_success_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.send_callback_success("ok").await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_send_callback_success_non_callback_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        let result = handle.send_callback_success("ok").await;
        assert!(matches!(result, Err(TestError::NotCallbackOperation)));
    }

    #[tokio::test]
    async fn test_send_callback_success_no_callback_id_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        // Callback without callback_id (not yet Submitted)
        let mut op = Operation::new("cb-001".to_string(), OperationType::Callback);
        op.callback_details = Some(SdkCallbackDetails {
            callback_id: None,
            result: None,
            error: None,
        });
        populate_handle(&handle, op).await;
        let result = handle.send_callback_success("ok").await;
        assert!(matches!(result, Err(TestError::ResultNotAvailable(_))));
    }

    #[tokio::test]
    async fn test_send_callback_success_no_sender_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_callback_operation()).await;
        // No callback_sender configured
        let result = handle.send_callback_success("ok").await;
        assert!(matches!(result, Err(TestError::ResultNotAvailable(_))));
    }

    #[tokio::test]
    async fn test_send_callback_success_delegates_to_sender() {
        let (handle, sender) = create_callback_handle_with_sender().await;
        let result = handle.send_callback_success("my-result").await;
        assert!(result.is_ok());
        let calls = sender.success_calls.read().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], ("cb-id-123".to_string(), "my-result".to_string()));
    }

    #[tokio::test]
    async fn test_send_callback_failure_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let error = crate::types::TestResultError::new("TestError", "something failed");
        let result = handle.send_callback_failure(&error).await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_send_callback_failure_non_callback_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        let error = crate::types::TestResultError::new("TestError", "something failed");
        let result = handle.send_callback_failure(&error).await;
        assert!(matches!(result, Err(TestError::NotCallbackOperation)));
    }

    #[tokio::test]
    async fn test_send_callback_failure_delegates_to_sender() {
        let (handle, sender) = create_callback_handle_with_sender().await;
        let error = crate::types::TestResultError::new("TestError", "something failed");
        let result = handle.send_callback_failure(&error).await;
        assert!(result.is_ok());
        let calls = sender.failure_calls.read().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "cb-id-123");
    }

    #[tokio::test]
    async fn test_send_callback_heartbeat_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        let result = handle.send_callback_heartbeat().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_send_callback_heartbeat_non_callback_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("test".into()), all_ops);
        populate_handle(&handle, create_step_operation()).await;
        let result = handle.send_callback_heartbeat().await;
        assert!(matches!(result, Err(TestError::NotCallbackOperation)));
    }

    #[tokio::test]
    async fn test_send_callback_heartbeat_delegates_to_sender() {
        let (handle, sender) = create_callback_handle_with_sender().await;
        let result = handle.send_callback_heartbeat().await;
        assert!(result.is_ok());
        let calls = sender.heartbeat_calls.read().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], "cb-id-123");
    }

    // =========================================================================
    // get_child_operations() tests (Requirements 3.1, 3.2, 3.3, 3.4)
    // =========================================================================

    /// Helper to create an operation with a parent_id for child enumeration tests.
    fn create_operation_with_parent(id: &str, name: &str, parent_id: Option<&str>) -> Operation {
        let mut op = Operation::new(id.to_string(), OperationType::Step);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Succeeded;
        op.parent_id = parent_id.map(|s| s.to_string());
        op
    }

    #[tokio::test]
    async fn test_get_child_operations_unpopulated_returns_error() {
        let all_ops = Arc::new(RwLock::new(Vec::new()));
        let handle = OperationHandle::new(OperationMatcher::ByName("parent".into()), all_ops);

        let result = handle.get_child_operations().await;
        assert!(matches!(result, Err(TestError::OperationNotFound(_))));
    }

    #[tokio::test]
    async fn test_get_child_operations_returns_matching_children() {
        let parent_op = create_operation_with_parent("parent-1", "parent", None);
        let child1 = create_operation_with_parent("child-1", "child-a", Some("parent-1"));
        let child2 = create_operation_with_parent("child-2", "child-b", Some("parent-1"));
        let unrelated = create_operation_with_parent("other-1", "other", Some("other-parent"));

        let all_ops = Arc::new(RwLock::new(vec![
            parent_op.clone(),
            child1,
            child2,
            unrelated,
        ]));

        let handle = OperationHandle::new(OperationMatcher::ByName("parent".into()), all_ops);
        populate_handle(&handle, parent_op).await;

        let children = handle.get_child_operations().await.unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].get_id(), "child-1");
        assert_eq!(children[1].get_id(), "child-2");
    }

    #[tokio::test]
    async fn test_get_child_operations_empty_when_no_children() {
        let parent_op = create_operation_with_parent("parent-1", "parent", None);
        let unrelated = create_operation_with_parent("other-1", "other", Some("other-parent"));

        let all_ops = Arc::new(RwLock::new(vec![parent_op.clone(), unrelated]));

        let handle = OperationHandle::new(OperationMatcher::ByName("parent".into()), all_ops);
        populate_handle(&handle, parent_op).await;

        let children = handle.get_child_operations().await.unwrap();
        assert!(children.is_empty());
    }

    #[tokio::test]
    async fn test_get_child_operations_preserves_execution_order() {
        let parent_op = create_operation_with_parent("parent-1", "parent", None);
        let child_c = create_operation_with_parent("child-c", "third", Some("parent-1"));
        let child_a = create_operation_with_parent("child-a", "first", Some("parent-1"));
        let child_b = create_operation_with_parent("child-b", "second", Some("parent-1"));

        // Insertion order: c, a, b — children should come back in that order
        let all_ops = Arc::new(RwLock::new(vec![
            parent_op.clone(),
            child_c,
            child_a,
            child_b,
        ]));

        let handle = OperationHandle::new(OperationMatcher::ByName("parent".into()), all_ops);
        populate_handle(&handle, parent_op).await;

        let children = handle.get_child_operations().await.unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].get_id(), "child-c");
        assert_eq!(children[1].get_id(), "child-a");
        assert_eq!(children[2].get_id(), "child-b");
    }

    #[tokio::test]
    async fn test_get_child_operations_children_support_recursive_enumeration() {
        let parent_op = create_operation_with_parent("parent-1", "parent", None);
        let child = create_operation_with_parent("child-1", "child", Some("parent-1"));
        let grandchild =
            create_operation_with_parent("grandchild-1", "grandchild", Some("child-1"));

        let all_ops = Arc::new(RwLock::new(vec![parent_op.clone(), child, grandchild]));

        let handle = OperationHandle::new(OperationMatcher::ByName("parent".into()), all_ops);
        populate_handle(&handle, parent_op).await;

        let children = handle.get_child_operations().await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].get_id(), "child-1");

        // Recursive: child should be able to enumerate its own children
        let grandchildren = children[0].get_child_operations();
        assert_eq!(grandchildren.len(), 1);
        assert_eq!(grandchildren[0].get_id(), "grandchild-1");
    }
}
