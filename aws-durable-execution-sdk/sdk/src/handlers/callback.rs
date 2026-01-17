//! Callback operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the callback handler which waits for external
//! systems to signal completion via a unique callback ID.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::config::CallbackConfig;
use crate::context::{create_operation_span, LogInfo, Logger, OperationIdentifier};
use crate::error::DurableError;
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;
use crate::types::CallbackId;

/// A callback handle that can be used to wait for external signals.
///
/// The callback ID can be shared with external systems, which can then
/// signal completion by calling the Lambda durable execution callback API.
pub struct Callback<T> {
    /// The unique callback ID
    pub callback_id: String,
    /// The operation identifier
    operation_id: String,
    /// Reference to the execution state
    state: Arc<ExecutionState>,
    /// Logger for structured logging
    logger: Arc<dyn Logger>,
    /// Phantom data for the result type (using fn() -> T for Send + Sync)
    _marker: PhantomData<fn() -> T>,
}

// Ensure Callback is Send + Sync
unsafe impl<T> Send for Callback<T> {}
unsafe impl<T> Sync for Callback<T> {}

impl<T> std::fmt::Debug for Callback<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Callback")
            .field("callback_id", &self.callback_id)
            .field("operation_id", &self.operation_id)
            .finish_non_exhaustive()
    }
}

impl<T> Callback<T>
where
    T: serde::Serialize + DeserializeOwned,
{
    /// Creates a new Callback.
    pub(crate) fn new(
        callback_id: impl Into<String>,
        operation_id: impl Into<String>,
        state: Arc<ExecutionState>,
        logger: Arc<dyn Logger>,
    ) -> Self {
        Self {
            callback_id: callback_id.into(),
            operation_id: operation_id.into(),
            state,
            logger,
            _marker: PhantomData,
        }
    }

    /// Returns the callback ID that external systems should use.
    pub fn id(&self) -> &str {
        &self.callback_id
    }

    /// Returns the callback ID as a `CallbackId` newtype.
    #[inline]
    pub fn id_typed(&self) -> CallbackId {
        CallbackId::from(self.callback_id.clone())
    }

    /// Waits for the callback result.
    ///
    /// This method checks if the callback has been signaled. If not,
    /// it suspends execution until the callback is received.
    ///
    /// # Returns
    ///
    /// The callback result on success, or an error if the callback
    /// failed, timed out, or execution was suspended.
    ///
    /// # Requirements
    ///
    /// - 6.4: Suspend execution when result is not available
    /// - 6.5: Return the result when callback receives success signal
    /// - 6.6: Return CallbackError when callback receives failure signal
    /// - 6.7: Return timeout error when callback times out
    pub async fn result(&self) -> Result<T, DurableError> {
        let log_info = LogInfo::new(self.state.durable_execution_arn())
            .with_operation_id(&self.operation_id)
            .with_extra("callback_id", &self.callback_id);

        self.logger.debug(
            &format!("Checking callback result: {}", self.callback_id),
            &log_info,
        );

        // Check the checkpoint for the callback result
        let checkpoint_result = self.state.get_checkpoint_result(&self.operation_id).await;

        if !checkpoint_result.is_existent() {
            // Callback not yet created - this shouldn't happen if used correctly
            return Err(DurableError::Callback {
                message: "Callback not found".to_string(),
                callback_id: Some(self.callback_id.clone()),
            });
        }

        // Check if callback succeeded
        if checkpoint_result.is_succeeded() {
            if let Some(result_str) = checkpoint_result.result() {
                let serdes = JsonSerDes::<T>::new();
                let serdes_ctx =
                    SerDesContext::new(&self.operation_id, self.state.durable_execution_arn());

                let result = serdes.deserialize(result_str, &serdes_ctx).map_err(|e| {
                    DurableError::SerDes {
                        message: format!("Failed to deserialize callback result: {}", e),
                    }
                })?;

                self.logger
                    .debug("Callback completed successfully", &log_info);
                self.state.track_replay(&self.operation_id).await;

                return Ok(result);
            }
        }

        // Check if callback failed
        if checkpoint_result.is_failed() {
            self.state.track_replay(&self.operation_id).await;

            if let Some(error) = checkpoint_result.error() {
                return Err(DurableError::Callback {
                    message: error.error_message.clone(),
                    callback_id: Some(self.callback_id.clone()),
                });
            } else {
                return Err(DurableError::Callback {
                    message: "Callback failed with unknown error".to_string(),
                    callback_id: Some(self.callback_id.clone()),
                });
            }
        }

        // Check if callback timed out
        if checkpoint_result.is_timed_out() {
            self.state.track_replay(&self.operation_id).await;

            return Err(DurableError::Callback {
                message: "Callback timed out".to_string(),
                callback_id: Some(self.callback_id.clone()),
            });
        }

        // Callback is still pending - suspend execution (Requirement 6.4)
        self.logger
            .debug("Callback pending, suspending execution", &log_info);

        Err(DurableError::Suspend {
            scheduled_timestamp: None,
        })
    }
}

/// Creates a callback and returns a Callback handle.
///
/// This handler implements the callback creation semantics:
/// - Generates a unique callback_id via checkpoint
/// - Creates a Callback struct with result() method
/// - Handles timeout and heartbeat configuration
///
/// # Arguments
///
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `config` - Callback configuration (timeout, heartbeat)
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// A Callback handle that can be used to wait for the result.
///
/// # Requirements
///
/// - 6.1: Generate unique callback_id via checkpoint
/// - 6.2: Support configurable timeout duration
/// - 6.3: Support configurable heartbeat timeout
pub async fn callback_handler<T>(
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    config: &CallbackConfig,
    logger: &Arc<dyn Logger>,
) -> Result<Callback<T>, DurableError>
where
    T: serde::Serialize + DeserializeOwned,
{
    // Create tracing span for this operation
    // Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6
    let span = create_operation_span("callback", op_id, state.durable_execution_arn());
    let _guard = span.enter();

    let mut log_info =
        LogInfo::new(state.durable_execution_arn()).with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Creating callback: {}", op_id), &log_info);

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;

    if checkpoint_result.is_existent() {
        // Check for non-deterministic execution
        if let Some(op_type) = checkpoint_result.operation_type() {
            if op_type != OperationType::Callback {
                span.record("status", "non_deterministic");
                return Err(DurableError::NonDeterministic {
                    message: format!(
                        "Expected Callback operation but found {:?} at operation_id {}",
                        op_type, op_id.operation_id
                    ),
                    operation_id: Some(op_id.operation_id.clone()),
                });
            }
        }

        // Callback already exists - get the callback_id from CallbackDetails
        // The service generates the callback_id and stores it in CallbackDetails.CallbackId
        let callback_id = checkpoint_result.callback_id().unwrap_or_else(|| {
            op_id
                .name
                .clone()
                .unwrap_or_else(|| op_id.operation_id.clone())
        });

        logger.debug(
            &format!("Returning existing callback: {}", callback_id),
            &log_info,
        );
        span.record("status", "replayed");

        return Ok(Callback::new(
            callback_id,
            &op_id.operation_id,
            state.clone(),
            logger.clone(),
        ));
    }

    // Create the callback checkpoint with configuration
    let start_update = create_callback_start_update(op_id, config);

    // Use create_checkpoint_with_response to get the service-generated callback_id
    let response = state.create_checkpoint_with_response(start_update).await?;

    // Extract the callback_id from the response's NewExecutionState
    // The service MUST return a callback_id - if not present, it's an error
    let callback_id = response
        .new_execution_state
        .as_ref()
        .and_then(|new_state| new_state.find_operation(&op_id.operation_id))
        .and_then(|op| op.callback_details.as_ref())
        .and_then(|details| details.callback_id.clone())
        .ok_or_else(|| {
            span.record("status", "failed");
            DurableError::Callback {
                message: format!(
                    "Service did not return callback_id in checkpoint response for operation {}",
                    op_id.operation_id
                ),
                callback_id: None,
            }
        })?;

    logger.debug(
        &format!("Callback created with ID: {}", callback_id),
        &log_info,
    );
    span.record("status", "created");

    Ok(Callback::new(
        callback_id,
        &op_id.operation_id,
        state.clone(),
        logger.clone(),
    ))
}

/// Creates a Start operation update for callback.
fn create_callback_start_update(
    op_id: &OperationIdentifier,
    config: &CallbackConfig,
) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Callback);

    // Set callback options for timeout configuration
    update.callback_options = Some(crate::operation::CallbackOptions {
        timeout_seconds: Some(config.timeout.to_seconds()),
        heartbeat_timeout_seconds: Some(config.heartbeat_timeout.to_seconds()),
    });

    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        CheckpointResponse, MockDurableServiceClient, NewExecutionState, SharedDurableServiceClient,
    };
    use crate::context::TracingLogger;
    use crate::duration::Duration;
    use crate::error::ErrorObject;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{CallbackDetails, Operation, OperationStatus};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new().with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "token-1".to_string(),
                new_execution_state: Some(NewExecutionState {
                    operations: vec![{
                        let mut op = Operation::new("test-callback-123", OperationType::Callback);
                        op.callback_details = Some(CallbackDetails {
                            callback_id: Some("service-generated-callback-id".to_string()),
                            result: None,
                            error: None,
                        });
                        op
                    }],
                    next_marker: None,
                }),
            })),
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

    fn create_test_op_id() -> OperationIdentifier {
        OperationIdentifier::new("test-callback-123", None, Some("test-callback".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config() -> CallbackConfig {
        CallbackConfig {
            timeout: Duration::from_hours(24),
            heartbeat_timeout: Duration::from_minutes(5),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_callback_handler_creates_callback() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_ok());
        let callback = result.unwrap();
        assert_eq!(callback.id(), "service-generated-callback-id");
    }

    #[tokio::test]
    async fn test_callback_handler_error_when_no_callback_id_returned() {
        // Create a mock client that returns a response without callback_id
        let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_response(Ok(
            CheckpointResponse {
                checkpoint_token: "token-1".to_string(),
                new_execution_state: None, // No NewExecutionState means no callback_id
            },
        )));
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback {
                message,
                callback_id,
            } => {
                assert!(message.contains("did not return callback_id"));
                assert!(callback_id.is_none());
            }
            e => panic!("Expected Callback error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_callback_handler_error_when_operation_not_in_response() {
        // Create a mock client that returns operations but none match our operation_id
        let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_response(Ok(
            CheckpointResponse {
                checkpoint_token: "token-1".to_string(),
                new_execution_state: Some(NewExecutionState {
                    operations: vec![{
                        // Different operation_id than what we're looking for
                        let mut op =
                            Operation::new("different-operation-id", OperationType::Callback);
                        op.callback_details = Some(CallbackDetails {
                            callback_id: Some("some-callback-id".to_string()),
                            result: None,
                            error: None,
                        });
                        op
                    }],
                    next_marker: None,
                }),
            },
        )));
        let state = create_test_state(client);
        let op_id = create_test_op_id(); // Uses "test-callback-123"
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback {
                message,
                callback_id,
            } => {
                assert!(message.contains("did not return callback_id"));
                assert!(callback_id.is_none());
            }
            e => panic!("Expected Callback error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_callback_handler_error_when_callback_details_missing() {
        // Create a mock client that returns the operation but without CallbackDetails
        let client = Arc::new(MockDurableServiceClient::new().with_checkpoint_response(Ok(
            CheckpointResponse {
                checkpoint_token: "token-1".to_string(),
                new_execution_state: Some(NewExecutionState {
                    operations: vec![{
                        // Correct operation_id but no callback_details
                        Operation::new("test-callback-123", OperationType::Callback)
                    }],
                    next_marker: None,
                }),
            },
        )));
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback {
                message,
                callback_id,
            } => {
                assert!(message.contains("did not return callback_id"));
                assert!(callback_id.is_none());
            }
            e => panic!("Expected Callback error, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_callback_handler_replay_existing() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a pre-existing callback operation
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::Started;
        op.name = Some("test-callback".to_string());

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_ok());
        let callback = result.unwrap();
        // Should return the existing callback
        assert_eq!(callback.id(), "test-callback");
    }

    #[tokio::test]
    async fn test_callback_handler_non_deterministic_detection() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a Step operation at the same ID (wrong type)
        let mut op = Operation::new("test-callback-123", OperationType::Step);
        op.status = OperationStatus::Succeeded;

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-callback-123".to_string()));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[tokio::test]
    async fn test_callback_result_pending() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a started callback (pending)
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::Started;

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let logger = create_test_logger();
        let callback: Callback<String> =
            Callback::new("test-callback-123", "test-callback-123", state, logger);

        let result = callback.result().await;

        // Should suspend since callback is pending
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { .. } => {}
            _ => panic!("Expected Suspend error"),
        }
    }

    #[tokio::test]
    async fn test_callback_result_succeeded() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a succeeded callback
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::Succeeded;
        op.result = Some(r#""callback_result""#.to_string());

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let logger = create_test_logger();
        let callback: Callback<String> =
            Callback::new("test-callback-123", "test-callback-123", state, logger);

        let result = callback.result().await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "callback_result");
    }

    #[tokio::test]
    async fn test_callback_result_failed() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a failed callback
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("CallbackError", "External system failed"));

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let logger = create_test_logger();
        let callback: Callback<String> =
            Callback::new("test-callback-123", "test-callback-123", state, logger);

        let result = callback.result().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback {
                message,
                callback_id,
            } => {
                assert!(message.contains("External system failed"));
                assert_eq!(callback_id, Some("test-callback-123".to_string()));
            }
            _ => panic!("Expected Callback error"),
        }
    }

    #[tokio::test]
    async fn test_callback_result_timed_out() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a timed out callback
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::TimedOut;

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let logger = create_test_logger();
        let callback: Callback<String> =
            Callback::new("test-callback-123", "test-callback-123", state, logger);

        let result = callback.result().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback { message, .. } => {
                assert!(message.contains("timed out"));
            }
            _ => panic!("Expected Callback error"),
        }
    }

    // Gap Test 8.3: Replaying FAILED callback returns callback_id (error deferred to result())
    #[tokio::test]
    async fn test_callback_handler_replay_failed_returns_callback_id() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a FAILED callback operation that has callback_details
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::Failed;
        op.name = Some("test-callback".to_string());
        op.callback_details = Some(CallbackDetails {
            callback_id: Some("failed-callback-id".to_string()),
            result: None,
            error: Some(ErrorObject::new("CallbackError", "External system failed")),
        });
        op.error = Some(ErrorObject::new("CallbackError", "External system failed"));

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        // The handler should return the Callback with callback_id even though it's FAILED
        // The error is deferred to result()
        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(
            result.is_ok(),
            "Handler should return Ok with callback_id for FAILED callback"
        );
        let callback = result.unwrap();
        assert_eq!(callback.id(), "failed-callback-id");

        // Now calling result() should return the error
        // Note: The callback_id in the error is the Callback struct's callback_id field
        let result_err = callback.result().await;
        assert!(result_err.is_err());
        match result_err.unwrap_err() {
            DurableError::Callback {
                message,
                callback_id,
            } => {
                assert!(message.contains("External system failed"));
                // The callback_id in the error comes from the Callback struct
                assert_eq!(callback_id, Some("failed-callback-id".to_string()));
            }
            e => panic!("Expected Callback error, got {:?}", e),
        }
    }

    // Gap Test 8.4: Replaying TIMED_OUT callback returns callback_id (error deferred to result())
    #[tokio::test]
    async fn test_callback_handler_replay_timed_out_returns_callback_id() {
        let client = Arc::new(MockDurableServiceClient::new());

        // Create state with a TIMED_OUT callback operation that has callback_details
        let mut op = Operation::new("test-callback-123", OperationType::Callback);
        op.status = OperationStatus::TimedOut;
        op.name = Some("test-callback".to_string());
        op.callback_details = Some(CallbackDetails {
            callback_id: Some("timed-out-callback-id".to_string()),
            result: None,
            error: None,
        });

        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));

        let op_id = create_test_op_id();
        let config = create_test_config();
        let logger = create_test_logger();

        // The handler should return the Callback with callback_id even though it's TIMED_OUT
        // The error is deferred to result()
        let result: Result<Callback<String>, DurableError> =
            callback_handler(&state, &op_id, &config, &logger).await;

        assert!(
            result.is_ok(),
            "Handler should return Ok with callback_id for TIMED_OUT callback"
        );
        let callback = result.unwrap();
        assert_eq!(callback.id(), "timed-out-callback-id");

        // Now calling result() should return the timeout error
        let result_err = callback.result().await;
        assert!(result_err.is_err());
        match result_err.unwrap_err() {
            DurableError::Callback { message, .. } => {
                assert!(message.contains("timed out"));
            }
            e => panic!("Expected Callback error with timeout message, got {:?}", e),
        }
    }

    #[test]
    fn test_create_callback_start_update() {
        let op_id = OperationIdentifier::new(
            "op-123",
            Some("parent-456".to_string()),
            Some("my-callback".to_string()),
        );
        let config = create_test_config();
        let update = create_callback_start_update(&op_id, &config);

        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Callback);
        assert!(update.callback_options.is_some());
        let callback_opts = update.callback_options.unwrap();
        assert_eq!(callback_opts.timeout_seconds, Some(86400)); // 24 hours in seconds
        assert_eq!(callback_opts.heartbeat_timeout_seconds, Some(300)); // 5 minutes in seconds
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-callback".to_string()));
    }
}
