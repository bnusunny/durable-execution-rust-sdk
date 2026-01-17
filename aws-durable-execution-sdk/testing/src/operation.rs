//! Durable operation types for testing.
//!
//! This module provides the `DurableOperation` struct which wraps SDK operations
//! and provides type-specific inspection methods and callback interaction capabilities.

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tokio::sync::watch;

use crate::error::TestError;
use crate::types::{TestResultError, WaitingOperationStatus};
use aws_durable_execution_sdk::{Operation, OperationStatus, OperationType};

/// Details for a step operation.
///
/// Contains information about a step operation including retry attempts,
/// result, and error information.
///
/// # Type Parameters
///
/// * `T` - The type of the result value (defaults to `serde_json::Value`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDetails<T = serde_json::Value> {
    /// The current retry attempt (0-indexed)
    pub attempt: Option<u32>,
    /// Timestamp for the next retry attempt
    pub next_attempt_timestamp: Option<DateTime<Utc>>,
    /// The result value if the step succeeded
    pub result: Option<T>,
    /// Error information if the step failed
    pub error: Option<TestResultError>,
}

impl<T> StepDetails<T> {
    /// Creates new StepDetails with default values.
    pub fn new() -> Self {
        Self {
            attempt: None,
            next_attempt_timestamp: None,
            result: None,
            error: None,
        }
    }
}

impl<T> Default for StepDetails<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Details for a wait operation.
///
/// Contains information about a wait/sleep operation including
/// the wait duration and scheduled end time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitDetails {
    /// Number of seconds to wait
    pub wait_seconds: Option<u64>,
    /// Timestamp when the wait is scheduled to end
    pub scheduled_end_timestamp: Option<DateTime<Utc>>,
}

impl WaitDetails {
    /// Creates new WaitDetails with default values.
    pub fn new() -> Self {
        Self {
            wait_seconds: None,
            scheduled_end_timestamp: None,
        }
    }
}

impl Default for WaitDetails {
    fn default() -> Self {
        Self::new()
    }
}

/// Details for a callback operation.
///
/// Contains information about a callback operation including
/// the callback ID, result, and error information.
///
/// # Type Parameters
///
/// * `T` - The type of the result value (defaults to `serde_json::Value`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackDetails<T = serde_json::Value> {
    /// The callback ID for external systems to use
    pub callback_id: Option<String>,
    /// The result value if the callback succeeded
    pub result: Option<T>,
    /// Error information if the callback failed
    pub error: Option<TestResultError>,
}

impl<T> CallbackDetails<T> {
    /// Creates new CallbackDetails with default values.
    pub fn new() -> Self {
        Self {
            callback_id: None,
            result: None,
            error: None,
        }
    }
}

impl<T> Default for CallbackDetails<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Details for an invoke operation.
///
/// Contains information about a chained invoke operation including
/// the result and error information.
///
/// # Type Parameters
///
/// * `T` - The type of the result value (defaults to `serde_json::Value`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeDetails<T = serde_json::Value> {
    /// The result value if the invocation succeeded
    pub result: Option<T>,
    /// Error information if the invocation failed
    pub error: Option<TestResultError>,
}

impl<T> InvokeDetails<T> {
    /// Creates new InvokeDetails with default values.
    pub fn new() -> Self {
        Self {
            result: None,
            error: None,
        }
    }
}

impl<T> Default for InvokeDetails<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Details for a context operation.
///
/// Contains information about a context operation including
/// the result and error information.
///
/// # Type Parameters
///
/// * `T` - The type of the result value (defaults to `serde_json::Value`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextDetails<T = serde_json::Value> {
    /// The result value if the context succeeded
    pub result: Option<T>,
    /// Error information if the context failed
    pub error: Option<TestResultError>,
}

impl<T> ContextDetails<T> {
    /// Creates new ContextDetails with default values.
    pub fn new() -> Self {
        Self {
            result: None,
            error: None,
        }
    }
}

impl<T> Default for ContextDetails<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for sending callback responses.
///
/// This trait is implemented by test runners to handle callback interactions.
#[async_trait::async_trait]
pub trait CallbackSender: Send + Sync {
    /// Sends a success response for a callback.
    async fn send_success(&self, callback_id: &str, result: &str) -> Result<(), TestError>;

    /// Sends a failure response for a callback.
    async fn send_failure(
        &self,
        callback_id: &str,
        error: &TestResultError,
    ) -> Result<(), TestError>;

    /// Sends a heartbeat for a callback.
    async fn send_heartbeat(&self, callback_id: &str) -> Result<(), TestError>;
}

/// A durable operation with inspection and interaction methods.
///
/// Wraps an SDK `Operation` and provides type-specific inspection methods
/// and callback interaction capabilities for testing.
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::DurableOperation;
///
/// // Get operation from test runner
/// let op = runner.get_operation("my-step").unwrap();
///
/// // Inspect basic properties
/// println!("ID: {}", op.get_id());
/// println!("Type: {}", op.get_type());
/// println!("Status: {}", op.get_status());
///
/// // Get type-specific details
/// if op.get_type() == OperationType::Step {
///     let details = op.get_step_details::<String>().unwrap();
///     println!("Result: {:?}", details.result);
/// }
/// ```
pub struct DurableOperation {
    /// The underlying SDK operation
    operation: Operation,
    /// Optional callback sender for callback operations
    callback_sender: Option<Arc<dyn CallbackSender>>,
    /// Optional status watcher for async waiting
    status_watcher: Option<watch::Receiver<OperationStatus>>,
}

impl std::fmt::Debug for DurableOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DurableOperation")
            .field("operation", &self.operation)
            .field("callback_sender", &self.callback_sender.is_some())
            .field("status_watcher", &self.status_watcher.is_some())
            .finish()
    }
}

impl Clone for DurableOperation {
    fn clone(&self) -> Self {
        Self {
            operation: self.operation.clone(),
            callback_sender: self.callback_sender.clone(),
            status_watcher: self.status_watcher.clone(),
        }
    }
}

impl DurableOperation {
    /// Creates a new DurableOperation wrapping the given SDK operation.
    pub fn new(operation: Operation) -> Self {
        Self {
            operation,
            callback_sender: None,
            status_watcher: None,
        }
    }

    /// Creates a new DurableOperation with a callback sender.
    pub fn with_callback_sender(
        operation: Operation,
        callback_sender: Arc<dyn CallbackSender>,
    ) -> Self {
        Self {
            operation,
            callback_sender: Some(callback_sender),
            status_watcher: None,
        }
    }

    /// Creates a new DurableOperation with a status watcher.
    pub fn with_status_watcher(
        operation: Operation,
        status_watcher: watch::Receiver<OperationStatus>,
    ) -> Self {
        Self {
            operation,
            callback_sender: None,
            status_watcher: Some(status_watcher),
        }
    }

    /// Creates a new DurableOperation with both callback sender and status watcher.
    pub fn with_all(
        operation: Operation,
        callback_sender: Option<Arc<dyn CallbackSender>>,
        status_watcher: Option<watch::Receiver<OperationStatus>>,
    ) -> Self {
        Self {
            operation,
            callback_sender,
            status_watcher,
        }
    }

    // =========================================================================
    // Basic Getters (Requirements 4.1, 4.2, 4.3, 4.4, 4.5)
    // =========================================================================

    /// Gets the operation ID.
    ///
    /// # Returns
    ///
    /// The unique identifier for this operation.
    ///
    /// # Requirements
    ///
    /// - 4.4: WHEN a developer calls get_operation_by_id(id) on Test_Runner, THE Test_Runner SHALL return the operation with that unique identifier
    pub fn get_id(&self) -> &str {
        &self.operation.operation_id
    }

    /// Gets the parent operation ID.
    ///
    /// # Returns
    ///
    /// The parent operation ID if this is a nested operation, None otherwise.
    pub fn get_parent_id(&self) -> Option<&str> {
        self.operation.parent_id.as_deref()
    }

    /// Gets the operation name.
    ///
    /// # Returns
    ///
    /// The human-readable name if set, None otherwise.
    ///
    /// # Requirements
    ///
    /// - 4.1: WHEN a developer calls get_operation(name) on Test_Runner, THE Test_Runner SHALL return the first operation with that name
    pub fn get_name(&self) -> Option<&str> {
        self.operation.name.as_deref()
    }

    /// Gets the operation type.
    ///
    /// # Returns
    ///
    /// The type of operation (Step, Wait, Callback, Invoke, Context, Execution).
    pub fn get_type(&self) -> OperationType {
        self.operation.operation_type
    }

    /// Gets the operation status.
    ///
    /// # Returns
    ///
    /// The current status of the operation.
    pub fn get_status(&self) -> OperationStatus {
        self.operation.status
    }

    /// Gets the start timestamp.
    ///
    /// # Returns
    ///
    /// The start timestamp as a DateTime if available.
    pub fn get_start_timestamp(&self) -> Option<DateTime<Utc>> {
        self.operation
            .start_timestamp
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
    }

    /// Gets the end timestamp.
    ///
    /// # Returns
    ///
    /// The end timestamp as a DateTime if available.
    pub fn get_end_timestamp(&self) -> Option<DateTime<Utc>> {
        self.operation
            .end_timestamp
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
    }

    /// Gets the raw operation data.
    ///
    /// # Returns
    ///
    /// A reference to the underlying SDK Operation.
    pub fn get_operation_data(&self) -> &Operation {
        &self.operation
    }

    /// Checks if this is a callback operation.
    ///
    /// # Returns
    ///
    /// True if this operation is of type Callback.
    pub fn is_callback(&self) -> bool {
        self.operation.operation_type == OperationType::Callback
    }

    /// Checks if the operation has completed.
    ///
    /// # Returns
    ///
    /// True if the operation status is terminal (Succeeded, Failed, Cancelled, TimedOut, Stopped).
    pub fn is_completed(&self) -> bool {
        self.operation.is_completed()
    }

    /// Checks if the operation succeeded.
    ///
    /// # Returns
    ///
    /// True if the operation status is Succeeded.
    pub fn is_succeeded(&self) -> bool {
        self.operation.is_succeeded()
    }

    /// Checks if the operation failed.
    ///
    /// # Returns
    ///
    /// True if the operation status is Failed, Cancelled, or TimedOut.
    pub fn is_failed(&self) -> bool {
        self.operation.is_failed()
    }
}

impl DurableOperation {
    // =========================================================================
    // Type-Specific Detail Methods (Requirements 5.1, 5.2, 5.3, 5.4, 5.5, 5.6)
    // =========================================================================

    /// Gets step-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(StepDetails<T>)` - The step details if this is a Step operation
    /// - `Err(TestError)` - Error if this is not a Step operation
    ///
    /// # Requirements
    ///
    /// - 5.1: WHEN a developer calls get_step_details() on a Step operation, THE Durable_Operation SHALL return attempt count, result, and error information
    /// - 5.6: IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error
    pub fn get_step_details<T: DeserializeOwned>(&self) -> Result<StepDetails<T>, TestError> {
        if self.operation.operation_type != OperationType::Step {
            return Err(TestError::type_mismatch(
                OperationType::Step,
                self.operation.operation_type,
            ));
        }

        let sdk_details = self.operation.step_details.as_ref();

        let result = if let Some(details) = sdk_details {
            if let Some(ref result_str) = details.result {
                Some(serde_json::from_str(result_str)?)
            } else {
                None
            }
        } else {
            None
        };

        let error = sdk_details
            .and_then(|d| d.error.as_ref())
            .map(|e| TestResultError::from(e.clone()));

        let next_attempt_timestamp = sdk_details
            .and_then(|d| d.next_attempt_timestamp)
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

        Ok(StepDetails {
            attempt: sdk_details.and_then(|d| d.attempt),
            next_attempt_timestamp,
            result,
            error,
        })
    }

    /// Gets wait-specific details.
    ///
    /// # Returns
    ///
    /// - `Ok(WaitDetails)` - The wait details if this is a Wait operation
    /// - `Err(TestError)` - Error if this is not a Wait operation
    ///
    /// # Requirements
    ///
    /// - 5.2: WHEN a developer calls get_wait_details() on a Wait operation, THE Durable_Operation SHALL return the wait duration and scheduled end timestamp
    /// - 5.6: IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error
    pub fn get_wait_details(&self) -> Result<WaitDetails, TestError> {
        if self.operation.operation_type != OperationType::Wait {
            return Err(TestError::type_mismatch(
                OperationType::Wait,
                self.operation.operation_type,
            ));
        }

        let sdk_details = self.operation.wait_details.as_ref();

        let scheduled_end_timestamp = sdk_details
            .and_then(|d| d.scheduled_end_timestamp)
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

        // Calculate wait_seconds from start and scheduled end if available
        let wait_seconds = match (self.operation.start_timestamp, sdk_details) {
            (Some(start), Some(details)) => details.scheduled_end_timestamp.map(|end| {
                let duration_ms = end.saturating_sub(start);
                (duration_ms / 1000) as u64
            }),
            _ => None,
        };

        Ok(WaitDetails {
            wait_seconds,
            scheduled_end_timestamp,
        })
    }

    /// Gets callback-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(CallbackDetails<T>)` - The callback details if this is a Callback operation
    /// - `Err(TestError)` - Error if this is not a Callback operation
    ///
    /// # Requirements
    ///
    /// - 5.3: WHEN a developer calls get_callback_details() on a Callback operation, THE Durable_Operation SHALL return the callback ID, result, and error
    /// - 5.6: IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error
    pub fn get_callback_details<T: DeserializeOwned>(
        &self,
    ) -> Result<CallbackDetails<T>, TestError> {
        if self.operation.operation_type != OperationType::Callback {
            return Err(TestError::type_mismatch(
                OperationType::Callback,
                self.operation.operation_type,
            ));
        }

        let sdk_details = self.operation.callback_details.as_ref();

        let result = if let Some(details) = sdk_details {
            if let Some(ref result_str) = details.result {
                Some(serde_json::from_str(result_str)?)
            } else {
                None
            }
        } else {
            None
        };

        let error = sdk_details
            .and_then(|d| d.error.as_ref())
            .map(|e| TestResultError::from(e.clone()));

        Ok(CallbackDetails {
            callback_id: sdk_details.and_then(|d| d.callback_id.clone()),
            result,
            error,
        })
    }

    /// Gets invoke-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(InvokeDetails<T>)` - The invoke details if this is an Invoke operation
    /// - `Err(TestError)` - Error if this is not an Invoke operation
    ///
    /// # Requirements
    ///
    /// - 5.4: WHEN a developer calls get_invoke_details() on an Invoke operation, THE Durable_Operation SHALL return the invocation result and error
    /// - 5.6: IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error
    pub fn get_invoke_details<T: DeserializeOwned>(&self) -> Result<InvokeDetails<T>, TestError> {
        if self.operation.operation_type != OperationType::Invoke {
            return Err(TestError::type_mismatch(
                OperationType::Invoke,
                self.operation.operation_type,
            ));
        }

        let sdk_details = self.operation.chained_invoke_details.as_ref();

        let result = if let Some(details) = sdk_details {
            if let Some(ref result_str) = details.result {
                Some(serde_json::from_str(result_str)?)
            } else {
                None
            }
        } else {
            None
        };

        let error = sdk_details
            .and_then(|d| d.error.as_ref())
            .map(|e| TestResultError::from(e.clone()));

        Ok(InvokeDetails { result, error })
    }

    /// Gets context-specific details.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The type to deserialize the result into
    ///
    /// # Returns
    ///
    /// - `Ok(ContextDetails<T>)` - The context details if this is a Context operation
    /// - `Err(TestError)` - Error if this is not a Context operation
    ///
    /// # Requirements
    ///
    /// - 5.5: WHEN a developer calls get_context_details() on a Context operation, THE Durable_Operation SHALL return the context result and error
    /// - 5.6: IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error
    pub fn get_context_details<T: DeserializeOwned>(&self) -> Result<ContextDetails<T>, TestError> {
        if self.operation.operation_type != OperationType::Context {
            return Err(TestError::type_mismatch(
                OperationType::Context,
                self.operation.operation_type,
            ));
        }

        let sdk_details = self.operation.context_details.as_ref();

        let result = if let Some(details) = sdk_details {
            if let Some(ref result_str) = details.result {
                Some(serde_json::from_str(result_str)?)
            } else {
                None
            }
        } else {
            None
        };

        let error = sdk_details
            .and_then(|d| d.error.as_ref())
            .map(|e| TestResultError::from(e.clone()));

        Ok(ContextDetails { result, error })
    }
}

impl DurableOperation {
    // =========================================================================
    // Callback Interaction Methods (Requirements 6.1, 6.2, 6.3, 6.4)
    // =========================================================================

    /// Sends a success response for a callback operation.
    ///
    /// # Arguments
    ///
    /// * `result` - The result value to send as a JSON string
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the callback response was sent successfully
    /// - `Err(TestError)` - If this is not a callback operation or sending failed
    ///
    /// # Requirements
    ///
    /// - 6.1: WHEN a developer calls send_callback_success(result) on a callback operation, THE Durable_Operation SHALL send a success response to the checkpoint service
    /// - 6.4: IF a developer calls callback methods on a non-callback operation, THEN THE Durable_Operation SHALL return an error
    pub async fn send_callback_success(&self, result: &str) -> Result<(), TestError> {
        if !self.is_callback() {
            return Err(TestError::NotCallbackOperation);
        }

        let callback_id = self.get_callback_id()?;

        if let Some(ref sender) = self.callback_sender {
            sender.send_success(&callback_id, result).await
        } else {
            // No callback sender configured - this is a local-only operation
            Ok(())
        }
    }

    /// Sends a failure response for a callback operation.
    ///
    /// # Arguments
    ///
    /// * `error` - The error information to send
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the callback response was sent successfully
    /// - `Err(TestError)` - If this is not a callback operation or sending failed
    ///
    /// # Requirements
    ///
    /// - 6.2: WHEN a developer calls send_callback_failure(error) on a callback operation, THE Durable_Operation SHALL send a failure response to the checkpoint service
    /// - 6.4: IF a developer calls callback methods on a non-callback operation, THEN THE Durable_Operation SHALL return an error
    pub async fn send_callback_failure(&self, error: &TestResultError) -> Result<(), TestError> {
        if !self.is_callback() {
            return Err(TestError::NotCallbackOperation);
        }

        let callback_id = self.get_callback_id()?;

        if let Some(ref sender) = self.callback_sender {
            sender.send_failure(&callback_id, error).await
        } else {
            // No callback sender configured - this is a local-only operation
            Ok(())
        }
    }

    /// Sends a heartbeat for a callback operation.
    ///
    /// # Returns
    ///
    /// - `Ok(())` - If the heartbeat was sent successfully
    /// - `Err(TestError)` - If this is not a callback operation or sending failed
    ///
    /// # Requirements
    ///
    /// - 6.3: WHEN a developer calls send_callback_heartbeat() on a callback operation, THE Durable_Operation SHALL send a heartbeat to keep the callback active
    /// - 6.4: IF a developer calls callback methods on a non-callback operation, THEN THE Durable_Operation SHALL return an error
    pub async fn send_callback_heartbeat(&self) -> Result<(), TestError> {
        if !self.is_callback() {
            return Err(TestError::NotCallbackOperation);
        }

        let callback_id = self.get_callback_id()?;

        if let Some(ref sender) = self.callback_sender {
            sender.send_heartbeat(&callback_id).await
        } else {
            // No callback sender configured - this is a local-only operation
            Ok(())
        }
    }

    /// Helper method to get the callback ID from callback details.
    fn get_callback_id(&self) -> Result<String, TestError> {
        self.operation
            .callback_details
            .as_ref()
            .and_then(|d| d.callback_id.clone())
            .ok_or_else(|| {
                TestError::result_not_available("Callback ID not available for this operation")
            })
    }
}

impl DurableOperation {
    // =========================================================================
    // Async Waiting Methods (Requirements 10.1, 10.2, 10.3, 10.4)
    // =========================================================================

    /// Waits for the operation to reach a specific status.
    ///
    /// # Arguments
    ///
    /// * `target_status` - The status to wait for
    ///
    /// # Returns
    ///
    /// - `Ok(&Self)` - Reference to self when the target status is reached
    /// - `Err(TestError)` - If the execution completes before reaching the target status
    ///
    /// # Requirements
    ///
    /// - 10.1: WHEN a developer calls wait_for_data() on a Durable_Operation, THE Durable_Operation SHALL return a future that resolves when the operation has started
    /// - 10.2: WHEN a developer calls wait_for_data(WaitingStatus::Completed) on a Durable_Operation, THE Durable_Operation SHALL return a future that resolves when the operation has completed
    /// - 10.3: WHEN a developer calls wait_for_data(WaitingStatus::Submitted) on a callback operation, THE Durable_Operation SHALL return a future that resolves when the callback is ready to receive responses
    /// - 10.4: IF the execution completes before the operation reaches the requested state, THEN THE Durable_Operation SHALL return an error
    pub async fn wait_for_data(
        &self,
        target_status: WaitingOperationStatus,
    ) -> Result<&Self, TestError> {
        // Check if we already meet the target status
        if self.has_reached_status(target_status) {
            return Ok(self);
        }

        // If we have a status watcher, use it to wait for updates
        if let Some(ref watcher) = self.status_watcher {
            let mut watcher = watcher.clone();

            loop {
                // Check current status
                let current_status = *watcher.borrow();

                if self.status_matches_target(current_status, target_status) {
                    return Ok(self);
                }

                // Check if execution completed before reaching target
                if current_status.is_terminal()
                    && !self.status_matches_target(current_status, target_status)
                {
                    return Err(TestError::execution_completed_early(
                        self.get_id(),
                        target_status,
                    ));
                }

                // Wait for next status change
                if watcher.changed().await.is_err() {
                    // Channel closed - execution completed
                    let final_status = *watcher.borrow();
                    if self.status_matches_target(final_status, target_status) {
                        return Ok(self);
                    }
                    return Err(TestError::execution_completed_early(
                        self.get_id(),
                        target_status,
                    ));
                }
            }
        } else {
            // No watcher available - check current status only
            if self.has_reached_status(target_status) {
                Ok(self)
            } else {
                Err(TestError::execution_completed_early(
                    self.get_id(),
                    target_status,
                ))
            }
        }
    }

    /// Checks if the operation has reached the target waiting status.
    fn has_reached_status(&self, target: WaitingOperationStatus) -> bool {
        self.status_matches_target(self.operation.status, target)
    }

    /// Checks if an operation status matches the target waiting status.
    fn status_matches_target(
        &self,
        current: OperationStatus,
        target: WaitingOperationStatus,
    ) -> bool {
        match target {
            WaitingOperationStatus::Started => {
                // Started means the operation has begun (not in initial state)
                // Any status other than a hypothetical "NotStarted" counts
                true // Operations are created in Started status
            }
            WaitingOperationStatus::Submitted => {
                // For callbacks, Submitted means the callback is ready to receive responses
                // This is indicated by the callback having a callback_id
                if self.is_callback() {
                    self.operation
                        .callback_details
                        .as_ref()
                        .map(|d| d.callback_id.is_some())
                        .unwrap_or(false)
                } else {
                    // For non-callbacks, treat Submitted as Started
                    true
                }
            }
            WaitingOperationStatus::Completed => {
                // Completed means the operation has finished (terminal status)
                current.is_terminal()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::{
        CallbackDetails as SdkCallbackDetails, ChainedInvokeDetails as SdkChainedInvokeDetails,
        ContextDetails as SdkContextDetails, Operation, OperationStatus, OperationType,
        StepDetails as SdkStepDetails, WaitDetails as SdkWaitDetails,
    };

    fn create_step_operation(name: &str, result: Option<&str>) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), OperationType::Step);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Succeeded;
        op.step_details = Some(SdkStepDetails {
            result: result.map(|s| s.to_string()),
            attempt: Some(1),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });
        op
    }

    fn create_wait_operation(name: &str) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), OperationType::Wait);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Succeeded;
        op.start_timestamp = Some(1000);
        op.wait_details = Some(SdkWaitDetails {
            scheduled_end_timestamp: Some(6000), // 5 seconds later
        });
        op
    }

    fn create_callback_operation(name: &str, callback_id: &str) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), OperationType::Callback);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Started;
        op.callback_details = Some(SdkCallbackDetails {
            callback_id: Some(callback_id.to_string()),
            result: None,
            error: None,
        });
        op
    }

    fn create_invoke_operation(name: &str, result: Option<&str>) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), OperationType::Invoke);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Succeeded;
        op.chained_invoke_details = Some(SdkChainedInvokeDetails {
            result: result.map(|s| s.to_string()),
            error: None,
        });
        op
    }

    fn create_context_operation(name: &str, result: Option<&str>) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), OperationType::Context);
        op.name = Some(name.to_string());
        op.status = OperationStatus::Succeeded;
        op.context_details = Some(SdkContextDetails {
            result: result.map(|s| s.to_string()),
            replay_children: None,
            error: None,
        });
        op
    }

    // =========================================================================
    // Basic Getter Tests
    // =========================================================================

    #[test]
    fn test_get_id() {
        let op = create_step_operation("my-step", None);
        let durable_op = DurableOperation::new(op);
        assert_eq!(durable_op.get_id(), "my-step-001");
    }

    #[test]
    fn test_get_name() {
        let op = create_step_operation("my-step", None);
        let durable_op = DurableOperation::new(op);
        assert_eq!(durable_op.get_name(), Some("my-step"));
    }

    #[test]
    fn test_get_type() {
        let step_op = DurableOperation::new(create_step_operation("step", None));
        assert_eq!(step_op.get_type(), OperationType::Step);

        let wait_op = DurableOperation::new(create_wait_operation("wait"));
        assert_eq!(wait_op.get_type(), OperationType::Wait);

        let callback_op = DurableOperation::new(create_callback_operation("callback", "cb-123"));
        assert_eq!(callback_op.get_type(), OperationType::Callback);
    }

    #[test]
    fn test_get_status() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);
        assert_eq!(durable_op.get_status(), OperationStatus::Succeeded);
    }

    #[test]
    fn test_is_callback() {
        let step_op = DurableOperation::new(create_step_operation("step", None));
        assert!(!step_op.is_callback());

        let callback_op = DurableOperation::new(create_callback_operation("callback", "cb-123"));
        assert!(callback_op.is_callback());
    }

    #[test]
    fn test_is_completed() {
        let mut op = create_step_operation("step", None);
        op.status = OperationStatus::Succeeded;
        let durable_op = DurableOperation::new(op);
        assert!(durable_op.is_completed());

        let mut op2 = create_step_operation("step2", None);
        op2.status = OperationStatus::Started;
        let durable_op2 = DurableOperation::new(op2);
        assert!(!durable_op2.is_completed());
    }

    // =========================================================================
    // Type-Specific Details Tests
    // =========================================================================

    #[test]
    fn test_get_step_details_success() {
        let op = create_step_operation("step", Some(r#""hello""#));
        let durable_op = DurableOperation::new(op);

        let details: StepDetails<String> = durable_op.get_step_details().unwrap();
        assert_eq!(details.attempt, Some(1));
        assert_eq!(details.result, Some("hello".to_string()));
        assert!(details.error.is_none());
    }

    #[test]
    fn test_get_step_details_wrong_type() {
        let op = create_wait_operation("wait");
        let durable_op = DurableOperation::new(op);

        let result: Result<StepDetails<String>, _> = durable_op.get_step_details();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::OperationTypeMismatch { .. }
        ));
    }

    #[test]
    fn test_get_wait_details_success() {
        let op = create_wait_operation("wait");
        let durable_op = DurableOperation::new(op);

        let details = durable_op.get_wait_details().unwrap();
        assert_eq!(details.wait_seconds, Some(5));
        assert!(details.scheduled_end_timestamp.is_some());
    }

    #[test]
    fn test_get_wait_details_wrong_type() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result = durable_op.get_wait_details();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::OperationTypeMismatch { .. }
        ));
    }

    #[test]
    fn test_get_callback_details_success() {
        let op = create_callback_operation("callback", "cb-123");
        let durable_op = DurableOperation::new(op);

        let details: CallbackDetails<String> = durable_op.get_callback_details().unwrap();
        assert_eq!(details.callback_id, Some("cb-123".to_string()));
        assert!(details.result.is_none());
        assert!(details.error.is_none());
    }

    #[test]
    fn test_get_callback_details_wrong_type() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result: Result<CallbackDetails<String>, _> = durable_op.get_callback_details();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::OperationTypeMismatch { .. }
        ));
    }

    #[test]
    fn test_get_invoke_details_success() {
        let op = create_invoke_operation("invoke", Some(r#"{"value": 42}"#));
        let durable_op = DurableOperation::new(op);

        let details: InvokeDetails<serde_json::Value> = durable_op.get_invoke_details().unwrap();
        assert!(details.result.is_some());
        assert_eq!(details.result.unwrap()["value"], 42);
        assert!(details.error.is_none());
    }

    #[test]
    fn test_get_invoke_details_wrong_type() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result: Result<InvokeDetails<String>, _> = durable_op.get_invoke_details();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::OperationTypeMismatch { .. }
        ));
    }

    #[test]
    fn test_get_context_details_success() {
        let op = create_context_operation("context", Some(r#""done""#));
        let durable_op = DurableOperation::new(op);

        let details: ContextDetails<String> = durable_op.get_context_details().unwrap();
        assert_eq!(details.result, Some("done".to_string()));
        assert!(details.error.is_none());
    }

    #[test]
    fn test_get_context_details_wrong_type() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result: Result<ContextDetails<String>, _> = durable_op.get_context_details();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::OperationTypeMismatch { .. }
        ));
    }

    // =========================================================================
    // Callback Method Tests
    // =========================================================================

    #[tokio::test]
    async fn test_send_callback_success_on_callback_operation() {
        let op = create_callback_operation("callback", "cb-123");
        let durable_op = DurableOperation::new(op);

        // Without a callback sender, this should succeed (no-op)
        let result = durable_op.send_callback_success(r#""result""#).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_callback_success_on_non_callback_operation() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result = durable_op.send_callback_success(r#""result""#).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::NotCallbackOperation
        ));
    }

    #[tokio::test]
    async fn test_send_callback_failure_on_callback_operation() {
        let op = create_callback_operation("callback", "cb-123");
        let durable_op = DurableOperation::new(op);

        let error = TestResultError::new("TestError", "Something went wrong");
        let result = durable_op.send_callback_failure(&error).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_callback_failure_on_non_callback_operation() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let error = TestResultError::new("TestError", "Something went wrong");
        let result = durable_op.send_callback_failure(&error).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::NotCallbackOperation
        ));
    }

    #[tokio::test]
    async fn test_send_callback_heartbeat_on_callback_operation() {
        let op = create_callback_operation("callback", "cb-123");
        let durable_op = DurableOperation::new(op);

        let result = durable_op.send_callback_heartbeat().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_callback_heartbeat_on_non_callback_operation() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result = durable_op.send_callback_heartbeat().await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TestError::NotCallbackOperation
        ));
    }

    // =========================================================================
    // Wait For Data Tests
    // =========================================================================

    #[tokio::test]
    async fn test_wait_for_data_started_already_started() {
        let op = create_step_operation("step", None);
        let durable_op = DurableOperation::new(op);

        let result = durable_op
            .wait_for_data(WaitingOperationStatus::Started)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_completed_already_completed() {
        let mut op = create_step_operation("step", None);
        op.status = OperationStatus::Succeeded;
        let durable_op = DurableOperation::new(op);

        let result = durable_op
            .wait_for_data(WaitingOperationStatus::Completed)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_submitted_callback_with_id() {
        let op = create_callback_operation("callback", "cb-123");
        let durable_op = DurableOperation::new(op);

        let result = durable_op
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_data_completed_not_yet_completed() {
        let mut op = create_step_operation("step", None);
        op.status = OperationStatus::Started;
        let durable_op = DurableOperation::new(op);

        // Without a watcher, this should fail since the operation isn't completed
        let result = durable_op
            .wait_for_data(WaitingOperationStatus::Completed)
            .await;
        assert!(result.is_err());
    }

    // =========================================================================
    // Details Struct Tests
    // =========================================================================

    #[test]
    fn test_step_details_default() {
        let details: StepDetails<String> = StepDetails::default();
        assert!(details.attempt.is_none());
        assert!(details.next_attempt_timestamp.is_none());
        assert!(details.result.is_none());
        assert!(details.error.is_none());
    }

    #[test]
    fn test_wait_details_default() {
        let details = WaitDetails::default();
        assert!(details.wait_seconds.is_none());
        assert!(details.scheduled_end_timestamp.is_none());
    }

    #[test]
    fn test_callback_details_default() {
        let details: CallbackDetails<String> = CallbackDetails::default();
        assert!(details.callback_id.is_none());
        assert!(details.result.is_none());
        assert!(details.error.is_none());
    }

    #[test]
    fn test_invoke_details_default() {
        let details: InvokeDetails<String> = InvokeDetails::default();
        assert!(details.result.is_none());
        assert!(details.error.is_none());
    }

    #[test]
    fn test_context_details_default() {
        let details: ContextDetails<String> = ContextDetails::default();
        assert!(details.result.is_none());
        assert!(details.error.is_none());
    }
}

/// Property-based tests for DurableOperation
///
/// These tests verify the correctness properties defined in the design document.
#[cfg(test)]
mod property_tests {
    use super::*;
    use aws_durable_execution_sdk::{
        CallbackDetails as SdkCallbackDetails, ChainedInvokeDetails as SdkChainedInvokeDetails,
        ContextDetails as SdkContextDetails, Operation, OperationStatus, OperationType,
        StepDetails as SdkStepDetails, WaitDetails as SdkWaitDetails,
    };
    use proptest::prelude::*;

    /// Strategy to generate a random operation type
    fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
        prop_oneof![
            Just(OperationType::Step),
            Just(OperationType::Wait),
            Just(OperationType::Callback),
            Just(OperationType::Invoke),
            Just(OperationType::Context),
        ]
    }

    /// Strategy to generate a random operation status
    fn operation_status_strategy() -> impl Strategy<Value = OperationStatus> {
        prop_oneof![
            Just(OperationStatus::Started),
            Just(OperationStatus::Pending),
            Just(OperationStatus::Ready),
            Just(OperationStatus::Succeeded),
            Just(OperationStatus::Failed),
            Just(OperationStatus::Cancelled),
            Just(OperationStatus::TimedOut),
            Just(OperationStatus::Stopped),
        ]
    }

    /// Strategy to generate a random operation ID
    fn operation_id_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_-]{1,32}".prop_map(|s| s)
    }

    /// Strategy to generate an optional JSON result string
    fn optional_result_strategy() -> impl Strategy<Value = Option<String>> {
        prop_oneof![
            Just(None),
            Just(Some(r#""hello""#.to_string())),
            Just(Some(r#"42"#.to_string())),
            Just(Some(r#"{"key": "value"}"#.to_string())),
            Just(Some(r#"true"#.to_string())),
        ]
    }

    /// Strategy to generate an optional callback ID
    fn optional_callback_id_strategy() -> impl Strategy<Value = Option<String>> {
        prop_oneof![Just(None), "[a-zA-Z0-9_-]{8,16}".prop_map(|s| Some(s)),]
    }

    /// Strategy to generate an optional timestamp
    fn optional_timestamp_strategy() -> impl Strategy<Value = Option<i64>> {
        prop_oneof![
            Just(None),
            (1577836800000i64..1893456000000i64).prop_map(Some),
        ]
    }

    /// Create an operation with the given type and appropriate details
    fn create_operation_with_type(
        op_type: OperationType,
        op_id: String,
        status: OperationStatus,
        result: Option<String>,
        callback_id: Option<String>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Operation {
        let mut op = Operation::new(op_id, op_type);
        op.status = status;
        op.start_timestamp = start_ts;
        op.end_timestamp = end_ts;

        match op_type {
            OperationType::Step => {
                op.step_details = Some(SdkStepDetails {
                    result,
                    attempt: Some(1),
                    next_attempt_timestamp: None,
                    error: None,
                    payload: None,
                });
            }
            OperationType::Wait => {
                op.wait_details = Some(SdkWaitDetails {
                    scheduled_end_timestamp: end_ts,
                });
            }
            OperationType::Callback => {
                op.callback_details = Some(SdkCallbackDetails {
                    callback_id,
                    result,
                    error: None,
                });
            }
            OperationType::Invoke => {
                op.chained_invoke_details = Some(SdkChainedInvokeDetails {
                    result,
                    error: None,
                });
            }
            OperationType::Context => {
                op.context_details = Some(SdkContextDetails {
                    result,
                    replay_children: None,
                    error: None,
                });
            }
            OperationType::Execution => {
                // Execution type doesn't have specific details we test
            }
        }

        op
    }

    // =========================================================================
    // Property 8: Type-Specific Details Availability
    // Feature: rust-testing-utilities, Property 8: Type-Specific Details Availability
    // Validates: Requirements 5.1, 5.2, 5.3, 5.4, 5.5, 5.6
    //
    // *For any* operation of type T, calling the corresponding `get_*_details()`
    // method SHALL return the details, and calling a different type's details
    // method SHALL return an error.
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn prop_type_specific_details_availability(
            op_type in operation_type_strategy(),
            op_id in operation_id_strategy(),
            status in operation_status_strategy(),
            result in optional_result_strategy(),
            callback_id in optional_callback_id_strategy(),
            start_ts in optional_timestamp_strategy(),
            end_ts in optional_timestamp_strategy(),
        ) {
            // Skip Execution type as it doesn't have type-specific details
            if op_type == OperationType::Execution {
                return Ok(());
            }

            let op = create_operation_with_type(
                op_type, op_id, status, result, callback_id, start_ts, end_ts
            );
            let durable_op = DurableOperation::new(op);

            // Test that the correct method succeeds
            match op_type {
                OperationType::Step => {
                    // Correct method should succeed
                    let step_result: Result<StepDetails<serde_json::Value>, _> =
                        durable_op.get_step_details();
                    prop_assert!(step_result.is_ok(), "get_step_details should succeed for Step operation");

                    // Wrong methods should fail
                    prop_assert!(durable_op.get_wait_details().is_err());
                    prop_assert!(durable_op.get_callback_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_invoke_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_context_details::<serde_json::Value>().is_err());
                }
                OperationType::Wait => {
                    // Correct method should succeed
                    prop_assert!(durable_op.get_wait_details().is_ok(),
                        "get_wait_details should succeed for Wait operation");

                    // Wrong methods should fail
                    prop_assert!(durable_op.get_step_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_callback_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_invoke_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_context_details::<serde_json::Value>().is_err());
                }
                OperationType::Callback => {
                    // Correct method should succeed
                    let callback_result: Result<CallbackDetails<serde_json::Value>, _> =
                        durable_op.get_callback_details();
                    prop_assert!(callback_result.is_ok(),
                        "get_callback_details should succeed for Callback operation");

                    // Wrong methods should fail
                    prop_assert!(durable_op.get_step_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_wait_details().is_err());
                    prop_assert!(durable_op.get_invoke_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_context_details::<serde_json::Value>().is_err());
                }
                OperationType::Invoke => {
                    // Correct method should succeed
                    let invoke_result: Result<InvokeDetails<serde_json::Value>, _> =
                        durable_op.get_invoke_details();
                    prop_assert!(invoke_result.is_ok(),
                        "get_invoke_details should succeed for Invoke operation");

                    // Wrong methods should fail
                    prop_assert!(durable_op.get_step_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_wait_details().is_err());
                    prop_assert!(durable_op.get_callback_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_context_details::<serde_json::Value>().is_err());
                }
                OperationType::Context => {
                    // Correct method should succeed
                    let context_result: Result<ContextDetails<serde_json::Value>, _> =
                        durable_op.get_context_details();
                    prop_assert!(context_result.is_ok(),
                        "get_context_details should succeed for Context operation");

                    // Wrong methods should fail
                    prop_assert!(durable_op.get_step_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_wait_details().is_err());
                    prop_assert!(durable_op.get_callback_details::<serde_json::Value>().is_err());
                    prop_assert!(durable_op.get_invoke_details::<serde_json::Value>().is_err());
                }
                OperationType::Execution => {
                    // Already handled above
                }
            }
        }
    }

    // =========================================================================
    // Property 9: Callback Method Type Safety
    // Feature: rust-testing-utilities, Property 9: Callback Method Type Safety
    // Validates: Requirements 6.1, 6.2, 6.3, 6.4
    //
    // *For any* callback operation, calling `send_callback_success()`,
    // `send_callback_failure()`, or `send_callback_heartbeat()` SHALL succeed.
    // *For any* non-callback operation, these methods SHALL return an error.
    // =========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn prop_callback_method_type_safety(
            op_type in operation_type_strategy(),
            op_id in operation_id_strategy(),
            status in operation_status_strategy(),
            result in optional_result_strategy(),
            callback_id in optional_callback_id_strategy(),
        ) {
            // Skip Execution type
            if op_type == OperationType::Execution {
                return Ok(());
            }

            let op = create_operation_with_type(
                op_type, op_id, status, result, callback_id.clone(), None, None
            );
            let durable_op = DurableOperation::new(op);

            // Use tokio runtime for async tests
            let rt = tokio::runtime::Runtime::new().unwrap();

            if op_type == OperationType::Callback {
                // For callback operations with a callback_id, methods should succeed
                // (without a callback sender, they are no-ops)
                if callback_id.is_some() {
                    let success_result = rt.block_on(durable_op.send_callback_success(r#""test""#));
                    prop_assert!(success_result.is_ok(),
                        "send_callback_success should succeed for Callback operation with callback_id");

                    let error = TestResultError::new("TestError", "test");
                    let failure_result = rt.block_on(durable_op.send_callback_failure(&error));
                    prop_assert!(failure_result.is_ok(),
                        "send_callback_failure should succeed for Callback operation with callback_id");

                    let heartbeat_result = rt.block_on(durable_op.send_callback_heartbeat());
                    prop_assert!(heartbeat_result.is_ok(),
                        "send_callback_heartbeat should succeed for Callback operation with callback_id");
                }
            } else {
                // For non-callback operations, all callback methods should fail
                let success_result = rt.block_on(durable_op.send_callback_success(r#""test""#));
                prop_assert!(success_result.is_err(),
                    "send_callback_success should fail for non-Callback operation");
                prop_assert!(matches!(success_result.unwrap_err(), TestError::NotCallbackOperation));

                let error = TestResultError::new("TestError", "test");
                let failure_result = rt.block_on(durable_op.send_callback_failure(&error));
                prop_assert!(failure_result.is_err(),
                    "send_callback_failure should fail for non-Callback operation");
                prop_assert!(matches!(failure_result.unwrap_err(), TestError::NotCallbackOperation));

                let heartbeat_result = rt.block_on(durable_op.send_callback_heartbeat());
                prop_assert!(heartbeat_result.is_err(),
                    "send_callback_heartbeat should fail for non-Callback operation");
                prop_assert!(matches!(heartbeat_result.unwrap_err(), TestError::NotCallbackOperation));
            }
        }
    }
}
