//! Wait operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the wait handler which pauses execution
//! for a specified duration without blocking Lambda resources.

use std::sync::Arc;

use crate::context::{Logger, LogInfo, OperationIdentifier};
use crate::duration::Duration;
use crate::error::DurableError;
use crate::operation::{OperationType, OperationUpdate};
use crate::state::ExecutionState;

/// Minimum wait duration in seconds.
const MIN_WAIT_SECONDS: u64 = 1;

/// Executes a wait operation that pauses execution for a specified duration.
///
/// This handler implements the wait semantics:
/// - Validates that duration is at least 1 second
/// - Checkpoints the wait start time
/// - Checks if the wait has elapsed
/// - Suspends execution if the wait has not elapsed
///
/// # Arguments
///
/// * `duration` - The duration to wait
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// Ok(()) when the wait has elapsed, or a Suspend error if still waiting.
///
/// # Requirements
///
/// - 5.1: Checkpoint the wait start time
/// - 5.2: Suspend execution if duration has not elapsed
/// - 5.3: Allow execution to continue when duration has elapsed
/// - 5.4: Validate that duration is at least 1 second
pub async fn wait_handler(
    duration: Duration,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<(), DurableError> {
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting wait operation: {} for {} seconds", op_id, duration.to_seconds()), &log_info);

    // Validate duration (Requirement 5.4)
    let wait_seconds = duration.to_seconds();
    if wait_seconds < MIN_WAIT_SECONDS {
        return Err(DurableError::Validation {
            message: format!(
                "Wait duration must be at least {} second(s), got {} seconds",
                MIN_WAIT_SECONDS, wait_seconds
            ),
        });
    }

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;

    if checkpoint_result.is_existent() {
        // Check for non-deterministic execution
        if let Some(op_type) = checkpoint_result.operation_type() {
            if op_type != OperationType::Wait {
                return Err(DurableError::NonDeterministic {
                    message: format!(
                        "Expected Wait operation but found {:?} at operation_id {}",
                        op_type, op_id.operation_id
                    ),
                    operation_id: Some(op_id.operation_id.clone()),
                });
            }
        }

        // If the wait has succeeded, we're done
        if checkpoint_result.is_succeeded() {
            logger.debug(&format!("Wait already completed: {}", op_id), &log_info);
            state.track_replay(&op_id.operation_id).await;
            return Ok(());
        }

        // If the wait was started but not yet succeeded, suspend and let the service handle timing
        if checkpoint_result.is_existent() && !checkpoint_result.is_terminal() {
            logger.debug(&format!("Wait still in progress: {}", op_id), &log_info);
            return Err(DurableError::Suspend {
                scheduled_timestamp: None,
            });
        }
    }

    // New wait operation - checkpoint the start with wait duration (Requirement 5.1)
    let start_update = create_start_update(op_id, wait_seconds);
    state.create_checkpoint(start_update, true).await?;

    logger.debug(&format!("Wait started for {} seconds", wait_seconds), &log_info);

    // Suspend execution (Requirement 5.2)
    // The service will handle the timing and resume the execution when the wait is complete
    Err(DurableError::Suspend {
        scheduled_timestamp: None,
    })
}

/// Creates a Start operation update for wait with the wait duration.
fn create_start_update(op_id: &OperationIdentifier, wait_seconds: u64) -> OperationUpdate {
    let mut update = OperationUpdate::start_wait(&op_id.operation_id, wait_seconds);
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
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-2".to_string(),
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
        OperationIdentifier::new("test-wait-123", None, Some("test-wait".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    #[tokio::test]
    async fn test_wait_handler_validation_error() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        // Duration less than 1 second should fail
        let result = wait_handler(
            Duration::from_seconds(0),
            &state,
            &op_id,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Validation { message } => {
                assert!(message.contains("at least 1 second"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[tokio::test]
    async fn test_wait_handler_suspends_on_new_wait() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { scheduled_timestamp: _ } => {
                // The service handles the timing, so we don't set scheduled_timestamp
                // Just verify we got a Suspend error
            }
            _ => panic!("Expected Suspend error"),
        }
    }

    #[tokio::test]
    async fn test_wait_handler_replay_completed() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing succeeded wait operation
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Succeeded;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        // Should succeed immediately since wait was already completed
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_handler_non_deterministic_detection() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a Step operation at the same ID (wrong type)
        let mut op = Operation::new("test-wait-123", OperationType::Step);
        op.status = OperationStatus::Succeeded;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-wait-123".to_string()));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[tokio::test]
    async fn test_wait_handler_replay_still_waiting() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a started wait operation (not yet elapsed)
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Started;
        // Note: The service handles timing, so we don't store start time in result anymore
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        let result = wait_handler(
            Duration::from_seconds(3600), // 1 hour wait
            &state,
            &op_id,
            &logger,
        ).await;

        // Should suspend since wait is still in STARTED status (not SUCCEEDED)
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { scheduled_timestamp: _ } => {
                // Expected - wait is still in progress
            }
            _ => panic!("Expected Suspend error"),
        }
    }

    #[test]
    fn test_create_start_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-wait".to_string()));
        let update = create_start_update(&op_id, 60);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Wait);
        assert!(update.wait_options.is_some());
        assert_eq!(update.wait_options.unwrap().wait_seconds, 60);
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-wait".to_string()));
    }
}
