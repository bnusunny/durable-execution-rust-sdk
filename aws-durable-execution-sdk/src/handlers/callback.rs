//! Callback operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the callback handler which waits for external
//! systems to signal completion via a unique callback ID.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::de::DeserializeOwned;

use crate::config::CallbackConfig;
use crate::context::{Logger, LogInfo, OperationIdentifier};
use crate::error::DurableError;
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::ExecutionState;

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

        self.logger.debug(&format!("Checking callback result: {}", self.callback_id), &log_info);

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
                let serdes_ctx = SerDesContext::new(&self.operation_id, self.state.durable_execution_arn());
                
                let result = serdes.deserialize(result_str, &serdes_ctx)
                    .map_err(|e| DurableError::SerDes {
                        message: format!("Failed to deserialize callback result: {}", e),
                    })?;
                
                self.logger.debug("Callback completed successfully", &log_info);
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
        self.logger.debug("Callback pending, suspending execution", &log_info);
        
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
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
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
                return Err(DurableError::NonDeterministic {
                    message: format!(
                        "Expected Callback operation but found {:?} at operation_id {}",
                        op_type, op_id.operation_id
                    ),
                    operation_id: Some(op_id.operation_id.clone()),
                });
            }
        }

        // Callback already exists - return the existing callback handle
        // The callback_id is stored in the operation's name or derived from operation_id
        let callback_id = op_id.name.clone().unwrap_or_else(|| op_id.operation_id.clone());
        
        logger.debug(&format!("Returning existing callback: {}", callback_id), &log_info);
        
        return Ok(Callback::new(
            callback_id,
            &op_id.operation_id,
            state.clone(),
            logger.clone(),
        ));
    }

    // Generate a unique callback_id (Requirement 6.1)
    // Use the operation_id as the callback_id for uniqueness
    let callback_id = op_id.operation_id.clone();

    // Create the callback checkpoint with configuration
    let start_update = create_callback_start_update(op_id, &callback_id, config);
    state.create_checkpoint(start_update, true).await?;

    logger.debug(&format!("Callback created with ID: {}", callback_id), &log_info);

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
    callback_id: &str,
    config: &CallbackConfig,
) -> OperationUpdate {
    // Store callback configuration in the result field as JSON
    let config_json = serde_json::json!({
        "callback_id": callback_id,
        "timeout_seconds": config.timeout.to_seconds(),
        "heartbeat_timeout_seconds": config.heartbeat_timeout.to_seconds(),
    });
    
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Callback);
    update.result = Some(config_json.to_string());
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
    use crate::client::{CheckpointResponse, MockDurableServiceClient, SharedDurableServiceClient};
    use crate::context::TracingLogger;
    use crate::duration::Duration;
    use crate::error::ErrorObject;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
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

        let result: Result<Callback<String>, DurableError> = callback_handler(
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

        assert!(result.is_ok());
        let callback = result.unwrap();
        assert_eq!(callback.id(), "test-callback-123");
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

        let result: Result<Callback<String>, DurableError> = callback_handler(
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

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

        let result: Result<Callback<String>, DurableError> = callback_handler(
            &state,
            &op_id,
            &config,
            &logger,
        ).await;

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
        let callback: Callback<String> = Callback::new(
            "test-callback-123",
            "test-callback-123",
            state,
            logger,
        );

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
        let callback: Callback<String> = Callback::new(
            "test-callback-123",
            "test-callback-123",
            state,
            logger,
        );

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
        let callback: Callback<String> = Callback::new(
            "test-callback-123",
            "test-callback-123",
            state,
            logger,
        );

        let result = callback.result().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback { message, callback_id } => {
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
        let callback: Callback<String> = Callback::new(
            "test-callback-123",
            "test-callback-123",
            state,
            logger,
        );

        let result = callback.result().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Callback { message, .. } => {
                assert!(message.contains("timed out"));
            }
            _ => panic!("Expected Callback error"),
        }
    }

    #[test]
    fn test_create_callback_start_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-callback".to_string()));
        let config = create_test_config();
        let update = create_callback_start_update(&op_id, "callback-id-123", &config);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Callback);
        assert!(update.result.is_some());
        assert!(update.result.as_ref().unwrap().contains("callback_id"));
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-callback".to_string()));
    }
}
