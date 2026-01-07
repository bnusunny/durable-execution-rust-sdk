//! Wait operation handler for the AWS Durable Execution SDK.
//!
//! This module implements the wait handler which pauses execution
//! for a specified duration without blocking Lambda resources.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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

        // If the wait was started, check if it has elapsed
        if checkpoint_result.is_existent() && !checkpoint_result.is_terminal() {
            // Get the start time from the checkpoint result
            if let Some(result_str) = checkpoint_result.result() {
                if let Ok(start_time) = result_str.parse::<f64>() {
                    let current_time = get_current_timestamp();
                    let elapsed = current_time - start_time;
                    
                    if elapsed >= wait_seconds as f64 {
                        // Wait has elapsed, checkpoint success
                        logger.debug(&format!("Wait elapsed after {} seconds", elapsed), &log_info);
                        
                        let succeed_update = create_succeed_update(op_id);
                        state.create_checkpoint(succeed_update, true).await?;
                        state.track_replay(&op_id.operation_id).await;
                        
                        return Ok(());
                    } else {
                        // Still waiting, suspend
                        let remaining = wait_seconds as f64 - elapsed;
                        logger.debug(&format!("Wait not elapsed, {} seconds remaining", remaining), &log_info);
                        
                        let resume_time = start_time + wait_seconds as f64;
                        return Err(DurableError::Suspend {
                            scheduled_timestamp: Some(resume_time),
                        });
                    }
                }
            }
        }
    }

    // New wait operation - checkpoint the start time (Requirement 5.1)
    let start_time = get_current_timestamp();
    let start_update = create_start_update(op_id, start_time);
    state.create_checkpoint(start_update, true).await?;

    logger.debug(&format!("Wait started at timestamp {}", start_time), &log_info);

    // Calculate when to resume
    let resume_time = start_time + wait_seconds as f64;

    // Suspend execution (Requirement 5.2)
    Err(DurableError::Suspend {
        scheduled_timestamp: Some(resume_time),
    })
}

/// Gets the current timestamp as seconds since UNIX epoch.
fn get_current_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Creates a Start operation update for wait with the start timestamp.
fn create_start_update(op_id: &OperationIdentifier, start_time: f64) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Wait);
    update.result = Some(start_time.to_string());
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Succeed operation update for wait.
fn create_succeed_update(op_id: &OperationIdentifier) -> OperationUpdate {
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Wait, None);
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
            DurableError::Suspend { scheduled_timestamp } => {
                assert!(scheduled_timestamp.is_some());
                // The scheduled timestamp should be approximately now + 60 seconds
                let ts = scheduled_timestamp.unwrap();
                let now = get_current_timestamp();
                assert!(ts > now);
                assert!(ts < now + 120.0); // Within 2 minutes
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
        // Set start time to now (so wait hasn't elapsed)
        op.result = Some(get_current_timestamp().to_string());
        
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

        // Should suspend since wait hasn't elapsed
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { scheduled_timestamp } => {
                assert!(scheduled_timestamp.is_some());
            }
            _ => panic!("Expected Suspend error"),
        }
    }

    #[tokio::test]
    async fn test_wait_handler_replay_elapsed() {
        let client = Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse {
                    checkpoint_token: "token-1".to_string(),
                }))
        );
        
        // Create state with a started wait operation that has elapsed
        let mut op = Operation::new("test-wait-123", OperationType::Wait);
        op.status = OperationStatus::Started;
        // Set start time to 10 seconds ago
        op.result = Some((get_current_timestamp() - 10.0).to_string());
        
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
            Duration::from_seconds(5), // 5 second wait (already elapsed)
            &state,
            &op_id,
            &logger,
        ).await;

        // Should succeed since wait has elapsed
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_start_update() {
        let op_id = OperationIdentifier::new("op-123", Some("parent-456".to_string()), Some("my-wait".to_string()));
        let update = create_start_update(&op_id, 1234567890.0);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Wait);
        assert_eq!(update.result, Some("1234567890".to_string()));
        assert_eq!(update.parent_id, Some("parent-456".to_string()));
        assert_eq!(update.name, Some("my-wait".to_string()));
    }

    #[test]
    fn test_create_succeed_update() {
        let op_id = OperationIdentifier::new("op-123", None, None);
        let update = create_succeed_update(&op_id);
        
        assert_eq!(update.operation_id, "op-123");
        assert_eq!(update.operation_type, OperationType::Wait);
        assert!(update.result.is_none());
    }

    #[test]
    fn test_get_current_timestamp() {
        let ts = get_current_timestamp();
        // Should be a reasonable timestamp (after year 2020)
        assert!(ts > 1577836800.0); // Jan 1, 2020
        // Should be before year 2100
        assert!(ts < 4102444800.0); // Jan 1, 2100
    }
}
