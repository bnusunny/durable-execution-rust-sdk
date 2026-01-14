//! Wait operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the wait handler which pauses execution
//! for a specified duration without blocking Lambda resources.

use std::sync::Arc;

use crate::context::{create_operation_span, Logger, LogInfo, OperationIdentifier};
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
    // Create tracing span for this operation
    // Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6
    let span = create_operation_span("wait", op_id, state.durable_execution_arn());
    let _guard = span.enter();

    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    logger.debug(&format!("Starting wait operation: {} for {} seconds", op_id, duration.to_seconds()), &log_info);

    // Validate duration (Requirement 5.4)
    let wait_seconds = duration.to_seconds();
    if wait_seconds < MIN_WAIT_SECONDS {
        span.record("status", "validation_failed");
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
                span.record("status", "non_deterministic");
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
            span.record("status", "replayed_succeeded");
            return Ok(());
        }

        // If the wait was cancelled, return Ok(()) - the wait is considered complete
        // Requirements: 5.5
        if checkpoint_result.is_cancelled() {
            logger.debug(&format!("Wait was cancelled: {}", op_id), &log_info);
            state.track_replay(&op_id.operation_id).await;
            span.record("status", "replayed_cancelled");
            return Ok(());
        }

        // If the wait was started but not yet succeeded, suspend and let the service handle timing
        if checkpoint_result.is_existent() && !checkpoint_result.is_terminal() {
            logger.debug(&format!("Wait still in progress: {}", op_id), &log_info);
            span.record("status", "suspended");
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
    span.record("status", "suspended");
    Err(DurableError::Suspend {
        scheduled_timestamp: None,
    })
}

/// Cancels an active wait operation.
///
/// This handler implements wait cancellation:
/// - Checks if the wait operation exists
/// - Validates that the operation is a WAIT type
/// - Handles already-completed waits gracefully
/// - Checkpoints the CANCEL action for active waits
///
/// # Arguments
///
/// * `state` - The execution state for checkpointing
/// * `operation_id` - The operation ID of the wait to cancel
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// Ok(()) if the wait was cancelled or was already completed, or an error if:
/// - The operation doesn't exist
/// - The operation is not a WAIT operation
/// - The checkpoint fails
///
/// # Requirements
///
/// - 5.5: THE Wait_Operation SHALL support cancellation of active waits via CANCEL action
pub async fn wait_cancel_handler(
    state: &Arc<ExecutionState>,
    operation_id: &str,
    logger: &Arc<dyn Logger>,
) -> Result<(), DurableError> {
    let log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(operation_id);

    logger.debug(&format!("Attempting to cancel wait operation: {}", operation_id), &log_info);

    // Check if the operation exists
    let checkpoint_result = state.get_checkpoint_result(operation_id).await;

    if !checkpoint_result.is_existent() {
        // Operation doesn't exist - this could be a race condition or invalid ID
        // We'll treat this as a no-op since there's nothing to cancel
        logger.debug(&format!("Wait operation not found, nothing to cancel: {}", operation_id), &log_info);
        return Ok(());
    }

    // Validate that this is a WAIT operation
    if let Some(op_type) = checkpoint_result.operation_type() {
        if op_type != OperationType::Wait {
            return Err(DurableError::Validation {
                message: format!(
                    "Cannot cancel operation {}: expected WAIT operation but found {:?}",
                    operation_id, op_type
                ),
            });
        }
    }

    // Check if the wait is already in a terminal state
    if checkpoint_result.is_terminal() {
        // Already completed (succeeded, failed, cancelled, timed out, etc.)
        // Nothing to do - return success
        logger.debug(&format!("Wait already completed, nothing to cancel: {}", operation_id), &log_info);
        return Ok(());
    }

    // The wait is still active - checkpoint the CANCEL action
    let cancel_update = OperationUpdate::cancel(operation_id, OperationType::Wait);
    state.create_checkpoint(cancel_update, true).await?;

    logger.info(&format!("Wait operation cancelled: {}", operation_id), &log_info);

    Ok(())
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
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
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

    // Tests for wait cancellation (Requirements: 5.5)

    #[tokio::test]
    async fn test_wait_cancel_handler_cancels_active_wait() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
        );
        
        // Create state with a started wait operation (active wait)
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Started;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let logger = create_test_logger();

        // Cancel the wait
        let result = wait_cancel_handler(&state, "test-wait-123", &logger).await;

        // Should succeed
        assert!(result.is_ok());
        
        // Verify the operation is now cancelled in local cache
        let checkpoint_result = state.get_checkpoint_result("test-wait-123").await;
        assert!(checkpoint_result.is_cancelled());
    }

    #[tokio::test]
    async fn test_wait_cancel_handler_handles_already_completed_wait() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a succeeded wait operation (already completed)
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Succeeded;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let logger = create_test_logger();

        // Try to cancel the already completed wait
        let result = wait_cancel_handler(&state, "test-wait-123", &logger).await;

        // Should succeed (no-op for already completed waits)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_cancel_handler_handles_nonexistent_wait() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create empty state (no operations)
        let initial_state = InitialExecutionState::new();
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let logger = create_test_logger();

        // Try to cancel a non-existent wait
        let result = wait_cancel_handler(&state, "nonexistent-wait", &logger).await;

        // Should succeed (no-op for non-existent waits)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_cancel_handler_rejects_non_wait_operation() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a Step operation (not a Wait)
        let mut op = Operation::new("test-step-123", OperationType::Step);
        op.status = OperationStatus::Started;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let logger = create_test_logger();

        // Try to cancel a Step operation (should fail)
        let result = wait_cancel_handler(&state, "test-step-123", &logger).await;

        // Should fail with validation error
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Validation { message } => {
                assert!(message.contains("expected WAIT operation"));
            }
            _ => panic!("Expected Validation error"),
        }
    }

    #[tokio::test]
    async fn test_wait_handler_replay_cancelled_wait() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a cancelled wait operation
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Cancelled;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        // Replay the cancelled wait
        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        // Should succeed immediately since wait was cancelled
        assert!(result.is_ok());
    }

    // Gap Tests for Wait Handler (Requirements: 11.1, 11.2, 11.3)

    /// Test 9.1: Verifies that status is checked before checkpoint creation.
    /// When a new wait operation is created, the handler first checks if an operation
    /// already exists at that ID (replay scenario), then creates the checkpoint.
    /// This test verifies the initial status check behavior.
    /// Requirements: 11.1
    #[tokio::test]
    async fn test_wait_handler_checks_status_before_checkpoint() {
        // Create a mock client that will receive the checkpoint
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
        );
        
        // Create empty state - no pre-existing operations
        let initial_state = InitialExecutionState::new();
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client.clone(),
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        // Execute wait handler - should check status first (no existing op), then checkpoint
        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        // Should suspend after creating checkpoint
        assert!(matches!(result, Err(DurableError::Suspend { .. })));
        
        // Verify checkpoint was created (operation should now exist in state)
        let checkpoint_result = state.get_checkpoint_result("test-wait-123").await;
        assert!(checkpoint_result.is_existent(), "Checkpoint should have been created");
        assert_eq!(checkpoint_result.operation_type(), Some(OperationType::Wait));
    }

    /// Test 9.1 (continued): Verifies that when replaying, the status check before
    /// checkpoint detects the existing operation and handles it appropriately.
    /// This tests the "before checkpoint" status check in replay scenarios.
    /// Requirements: 11.1
    #[tokio::test]
    async fn test_wait_handler_status_check_detects_existing_operation() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create state with a pre-existing wait operation in PENDING status
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Pending;
        
        let initial_state = InitialExecutionState::with_operations(vec![op]);
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        // Execute wait handler - should detect existing operation in status check
        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        // Should suspend because wait is still pending (not terminal)
        assert!(matches!(result, Err(DurableError::Suspend { .. })));
    }

    /// Test 9.1 (continued): Verifies immediate response handling when wait
    /// completes between initial check and checkpoint response.
    /// This simulates the scenario where the service immediately completes the wait.
    /// Requirements: 11.1
    #[tokio::test]
    async fn test_wait_handler_immediate_completion_via_checkpoint_response() {
        use crate::client::{CheckpointResponse, NewExecutionState};
        
        // Create a mock client that returns a checkpoint response with SUCCEEDED status
        // This simulates the service immediately completing the wait
        let mut succeeded_op = Operation::new("test-wait-123", OperationType::Wait);
        succeeded_op.status = OperationStatus::Succeeded;
        
        let checkpoint_response = CheckpointResponse {
            checkpoint_token: "token-1".to_string(),
            new_execution_state: Some(NewExecutionState {
                operations: vec![succeeded_op],
                next_marker: None,
            }),
        };
        
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(checkpoint_response))
        );
        
        // Create empty state - no pre-existing operations
        let initial_state = InitialExecutionState::new();
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            initial_state,
            client,
        ));
        
        let op_id = create_test_op_id();
        let logger = create_test_logger();

        // Execute wait handler
        let result = wait_handler(
            Duration::from_seconds(60),
            &state,
            &op_id,
            &logger,
        ).await;

        // Current implementation suspends after checkpoint regardless of response
        // This test documents the current behavior - the handler suspends even if
        // the checkpoint response indicates immediate completion.
        // A future enhancement could check the response and return Ok(()) immediately.
        assert!(matches!(result, Err(DurableError::Suspend { .. })));
        
        // However, the state should reflect the succeeded status from the response
        let checkpoint_result = state.get_checkpoint_result("test-wait-123").await;
        assert!(checkpoint_result.is_succeeded(), 
            "State should reflect succeeded status from checkpoint response");
    }
}
