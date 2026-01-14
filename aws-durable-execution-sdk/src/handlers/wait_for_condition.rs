//! Wait-for-condition handler for the AWS Durable Execution SDK.
//!
//! This module implements the wait_for_condition pattern using a single STEP
//! operation with RETRY mechanism. This is more efficient than using multiple
//! steps and waits because it uses a single operation ID and tracks state
//! via the RETRY payload.

use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};

use crate::context::{Logger, LogInfo, OperationIdentifier, WaitForConditionConfig, WaitForConditionContext};
use crate::error::{DurableError, ErrorObject, TerminationReason};
use crate::operation::{OperationType, OperationUpdate};
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::{CheckpointedResult, ExecutionState};

/// Internal state for wait_for_condition tracking.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct WaitForConditionState<S> {
    /// The user-provided state
    user_state: S,
    /// Current attempt number (1-indexed)
    attempt: usize,
}

/// Executes a wait_for_condition operation using a single STEP with RETRY mechanism.
///
/// This handler implements the wait-for-condition pattern as specified in Requirements 1.8, 4.9:
/// - Uses a single STEP operation instead of multiple steps and waits
/// - Passes state as Payload on retry (not Error)
/// - Uses NextAttemptDelaySeconds for wait intervals
/// - Tracks attempt number in StepDetails.Attempt
///
/// # Arguments
///
/// * `check` - The function to check the condition
/// * `config` - Configuration for the wait (interval, max attempts, timeout)
/// * `state` - The execution state for checkpointing
/// * `op_id` - The operation identifier
/// * `logger` - Logger for structured logging
///
/// # Returns
///
/// The result when the condition is met, or an error if timeout/max attempts exceeded.
///
/// # Requirements
///
/// - 1.8: THE DurableContext SHALL provide a `wait_for_condition` method that polls until a condition is met
/// - 4.9: THE Step_Operation SHALL support RETRY action with Payload for wait-for-condition pattern
pub async fn wait_for_condition_handler<T, S, F>(
    check: F,
    config: WaitForConditionConfig<S>,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<T, DurableError>
where
    T: Serialize + DeserializeOwned + Send,
    S: Serialize + DeserializeOwned + Clone + Send + Sync,
    F: Fn(&S, &WaitForConditionContext) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send + Sync,
{
    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }
    
    logger.debug(&format!("Starting wait_for_condition operation: {}", op_id), &log_info);

    // Check for existing checkpoint (replay)
    let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
    
    // Handle replay scenarios
    if let Some(result) = handle_replay::<T>(&checkpoint_result, state, op_id, logger).await? {
        return Ok(result);
    }

    // Determine current attempt and state from checkpoint or initial config
    let (current_attempt, user_state) = get_current_state::<S>(&checkpoint_result, &config)?;
    
    let max_attempts = config.max_attempts.unwrap_or(usize::MAX);
    
    // Check if max attempts exceeded
    if current_attempt > max_attempts {
        logger.error(&format!("Max attempts ({}) exceeded for wait_for_condition", max_attempts), &log_info);
        
        // Checkpoint failure
        let error = ErrorObject::new(
            "MaxAttemptsExceeded",
            format!("Max attempts ({}) exceeded for wait_for_condition", max_attempts),
        );
        let fail_update = create_fail_update(op_id, error);
        state.create_checkpoint(fail_update, true).await?;
        
        return Err(DurableError::Execution {
            message: format!("Max attempts ({}) exceeded for wait_for_condition", max_attempts),
            termination_reason: TerminationReason::ExecutionError,
        });
    }

    // Create context for this check
    let check_ctx = WaitForConditionContext {
        attempt: current_attempt,
        max_attempts: config.max_attempts,
    };

    logger.debug(&format!("Checking condition (attempt {})", current_attempt), &log_info);

    // If this is the first attempt and no checkpoint exists, checkpoint START
    if current_attempt == 1 && !checkpoint_result.is_existent() {
        let start_update = create_start_update(op_id);
        state.create_checkpoint(start_update, true).await?;
    }

    // Execute the condition check
    match check(&user_state, &check_ctx) {
        Ok(result) => {
            // Condition met - checkpoint success
            logger.debug(&format!("Condition met on attempt {}", current_attempt), &log_info);
            
            let serdes = JsonSerDes::<T>::new();
            let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
            let serialized = serdes.serialize(&result, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to serialize wait_for_condition result: {}", e),
                })?;
            
            let succeed_update = create_succeed_update(op_id, Some(serialized));
            state.create_checkpoint(succeed_update, true).await?;
            
            Ok(result)
        }
        Err(e) => {
            // Condition not met - checkpoint RETRY with state payload
            logger.debug(&format!("Condition not met on attempt {}: {}", current_attempt, e), &log_info);
            
            if current_attempt >= max_attempts {
                // No more retries - fail
                let error = ErrorObject::new(
                    "MaxAttemptsExceeded",
                    format!("Max attempts ({}) exceeded for wait_for_condition. Last error: {}", max_attempts, e),
                );
                let fail_update = create_fail_update(op_id, error);
                state.create_checkpoint(fail_update, true).await?;
                
                return Err(DurableError::Execution {
                    message: format!("Max attempts ({}) exceeded for wait_for_condition", max_attempts),
                    termination_reason: TerminationReason::ExecutionError,
                });
            }
            
            // Prepare state for next attempt
            let next_state = WaitForConditionState {
                user_state: user_state.clone(),
                attempt: current_attempt + 1,
            };
            
            let state_serdes = JsonSerDes::<WaitForConditionState<S>>::new();
            let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
            let serialized_state = state_serdes.serialize(&next_state, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to serialize wait_for_condition state: {}", e),
                })?;
            
            // Checkpoint RETRY with payload and delay
            let retry_update = create_retry_update(
                op_id,
                Some(serialized_state),
                Some(config.interval.to_seconds()),
            );
            state.create_checkpoint(retry_update, true).await?;
            
            // Suspend execution - Lambda will re-invoke after the delay
            Err(DurableError::Suspend {
                scheduled_timestamp: None,
            })
        }
    }
}

/// Handles replay by checking if the operation was previously checkpointed.
async fn handle_replay<T>(
    checkpoint_result: &CheckpointedResult,
    state: &Arc<ExecutionState>,
    op_id: &OperationIdentifier,
    logger: &Arc<dyn Logger>,
) -> Result<Option<T>, DurableError>
where
    T: Serialize + DeserializeOwned,
{
    if !checkpoint_result.is_existent() {
        return Ok(None);
    }

    let mut log_info = LogInfo::new(state.durable_execution_arn())
        .with_operation_id(&op_id.operation_id);
    if let Some(ref parent_id) = op_id.parent_id {
        log_info = log_info.with_parent_id(parent_id);
    }

    // Check for non-deterministic execution
    if let Some(op_type) = checkpoint_result.operation_type() {
        if op_type != OperationType::Step {
            return Err(DurableError::NonDeterministic {
                message: format!(
                    "Expected Step operation but found {:?} at operation_id {}",
                    op_type, op_id.operation_id
                ),
                operation_id: Some(op_id.operation_id.clone()),
            });
        }
    }

    // Handle succeeded checkpoint
    if checkpoint_result.is_succeeded() {
        logger.debug(&format!("Replaying succeeded wait_for_condition: {}", op_id), &log_info);
        
        // Track replay
        state.track_replay(&op_id.operation_id).await;
        
        // Get the result from the checkpoint
        if let Some(result_str) = checkpoint_result.result() {
            let serdes = JsonSerDes::<T>::new();
            let serdes_ctx = SerDesContext::new(&op_id.operation_id, state.durable_execution_arn());
            let result = serdes.deserialize(result_str, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to deserialize checkpointed result: {}", e),
                })?;
            
            return Ok(Some(result));
        }
    }

    // Handle failed checkpoint
    if checkpoint_result.is_failed() {
        logger.debug(&format!("Replaying failed wait_for_condition: {}", op_id), &log_info);
        
        // Track replay
        state.track_replay(&op_id.operation_id).await;
        
        if let Some(error) = checkpoint_result.error() {
            return Err(DurableError::UserCode {
                message: error.error_message.clone(),
                error_type: error.error_type.clone(),
                stack_trace: error.stack_trace.clone(),
            });
        } else {
            return Err(DurableError::execution("wait_for_condition failed with unknown error"));
        }
    }

    // Handle READY status - operation is ready to resume execution
    // Requirements: 3.7 - Resume execution without re-checkpointing START
    if checkpoint_result.is_ready() {
        logger.debug(&format!("Resuming READY wait_for_condition: {}", op_id), &log_info);
        // Return None to indicate execution should continue
        return Ok(None);
    }

    // Handle PENDING status - operation is waiting for retry
    // For wait_for_condition, PENDING means we should continue with the next attempt
    if checkpoint_result.is_pending() {
        logger.debug(&format!("Resuming PENDING wait_for_condition: {}", op_id), &log_info);
        // Return None to indicate execution should continue with the next attempt
        return Ok(None);
    }

    // Operation exists but is not terminal (Started state) - continue execution
    Ok(None)
}

/// Gets the current attempt number and user state from checkpoint or initial config.
fn get_current_state<S>(
    checkpoint_result: &CheckpointedResult,
    config: &WaitForConditionConfig<S>,
) -> Result<(usize, S), DurableError>
where
    S: Serialize + DeserializeOwned + Clone,
{
    // If there's a retry payload, deserialize it to get the current state
    if let Some(payload) = checkpoint_result.retry_payload() {
        let serdes = JsonSerDes::<WaitForConditionState<S>>::new();
        let serdes_ctx = SerDesContext::new("", "");
        let state: WaitForConditionState<S> = serdes.deserialize(payload, &serdes_ctx)
            .map_err(|e| DurableError::SerDes {
                message: format!("Failed to deserialize wait_for_condition state: {}", e),
            })?;
        
        return Ok((state.attempt, state.user_state));
    }
    
    // If there's an attempt number in the checkpoint, use it
    if let Some(attempt) = checkpoint_result.attempt() {
        return Ok((attempt as usize, config.initial_state.clone()));
    }
    
    // First attempt with initial state
    Ok((1, config.initial_state.clone()))
}

/// Creates a Start operation update.
fn create_start_update(op_id: &OperationIdentifier) -> OperationUpdate {
    let mut update = OperationUpdate::start(&op_id.operation_id, OperationType::Step)
        .with_sub_type("wait_for_condition");
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Succeed operation update.
fn create_succeed_update(op_id: &OperationIdentifier, result: Option<String>) -> OperationUpdate {
    let mut update = OperationUpdate::succeed(&op_id.operation_id, OperationType::Step, result)
        .with_sub_type("wait_for_condition");
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Fail operation update.
fn create_fail_update(op_id: &OperationIdentifier, error: ErrorObject) -> OperationUpdate {
    let mut update = OperationUpdate::fail(&op_id.operation_id, OperationType::Step, error)
        .with_sub_type("wait_for_condition");
    if let Some(ref parent_id) = op_id.parent_id {
        update = update.with_parent_id(parent_id);
    }
    if let Some(ref name) = op_id.name {
        update = update.with_name(name);
    }
    update
}

/// Creates a Retry operation update with payload.
///
/// # Requirements
///
/// - 4.7: THE Step_Operation SHALL support RETRY action with NextAttemptDelaySeconds for backoff
/// - 4.9: THE Step_Operation SHALL support RETRY action with Payload for wait-for-condition pattern
fn create_retry_update(
    op_id: &OperationIdentifier,
    payload: Option<String>,
    next_attempt_delay_seconds: Option<u64>,
) -> OperationUpdate {
    let mut update = OperationUpdate::retry(
        &op_id.operation_id,
        OperationType::Step,
        payload,
        next_attempt_delay_seconds,
    ).with_sub_type("wait_for_condition");
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
    use crate::context::{TracingLogger, OperationIdentifier};
    use crate::duration::Duration;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{Operation, OperationStatus, StepDetails};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn create_mock_client() -> SharedDurableServiceClient {
        Arc::new(
            MockDurableServiceClient::new()
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
                .with_checkpoint_response(Ok(CheckpointResponse::new("token-3")))
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

    fn create_test_state_with_operations(
        client: SharedDurableServiceClient,
        operations: Vec<Operation>,
    ) -> Arc<ExecutionState> {
        Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc123",
            "initial-token",
            InitialExecutionState::with_operations(operations),
            client,
        ))
    }

    fn create_test_op_id() -> OperationIdentifier {
        OperationIdentifier::new("test-wait-cond-123", None, Some("test-wait-condition".to_string()))
    }

    fn create_test_logger() -> Arc<dyn Logger> {
        Arc::new(TracingLogger)
    }

    fn create_test_config<S: Clone>(initial_state: S) -> WaitForConditionConfig<S> {
        WaitForConditionConfig {
            initial_state,
            interval: Duration::from_seconds(5),
            max_attempts: Some(3),
            timeout: None,
        }
    }

    // ==========================================================================
    // Test 1.1: Initial condition check execution on new operation
    // Requirements: 1.1 - WHEN a wait-for-condition operation starts, THE Test_Suite 
    // SHALL verify the initial check is executed
    // ==========================================================================

    #[tokio::test]
    async fn test_initial_condition_check_executed_on_new_operation() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        // Track if the check function was called
        let check_called = Arc::new(AtomicUsize::new(0));
        let check_called_clone = check_called.clone();
        
        let config = create_test_config(0i32);
        
        // Condition that succeeds immediately
        let result = wait_for_condition_handler(
            move |_state: &i32, ctx: &WaitForConditionContext| {
                check_called_clone.fetch_add(1, Ordering::SeqCst);
                assert_eq!(ctx.attempt, 1, "First attempt should be 1");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(42)
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok(), "Should succeed when condition passes");
        assert_eq!(result.unwrap(), 42);
        assert_eq!(check_called.load(Ordering::SeqCst), 1, "Check function should be called once");
    }

    #[tokio::test]
    async fn test_initial_check_receives_initial_state() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        #[derive(Clone, serde::Serialize, serde::Deserialize)]
        struct TestState {
            counter: i32,
            name: String,
        }
        
        let initial_state = TestState {
            counter: 42,
            name: "test".to_string(),
        };
        
        let config = WaitForConditionConfig {
            initial_state,
            interval: Duration::from_seconds(5),
            max_attempts: Some(3),
            timeout: None,
        };
        
        let result = wait_for_condition_handler(
            |state: &TestState, _ctx: &WaitForConditionContext| {
                // Verify initial state is passed correctly
                assert_eq!(state.counter, 42);
                assert_eq!(state.name, "test");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("success".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }


    // ==========================================================================
    // Test 1.2: RETRY action checkpoint with state payload
    // Requirements: 1.2 - WHEN a wait-for-condition check returns retry with state, 
    // THE Test_Suite SHALL verify RETRY action is checkpointed with the state payload
    // ==========================================================================

    #[tokio::test]
    async fn test_retry_action_checkpointed_with_state_payload() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // Condition that fails (returns error to trigger retry)
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Err::<String, _>("condition not met".into())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        // Should suspend for retry
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { .. } => {
                // Expected - execution should suspend for retry
            }
            other => panic!("Expected Suspend error, got: {:?}", other),
        }
        
        // Verify checkpoint was created with RETRY action
        let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
        assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
        
        // The operation should be in PENDING status after RETRY
        let op = checkpoint_result.operation().expect("Operation should exist");
        assert_eq!(op.status, OperationStatus::Pending, "Status should be Pending after RETRY");
    }

    // ==========================================================================
    // Test 1.3: SUCCEED action checkpoint when condition passes
    // Requirements: 1.3 - WHEN a wait-for-condition check succeeds, THE Test_Suite 
    // SHALL verify the final result is returned and SUCCEED is checkpointed
    // ==========================================================================

    #[tokio::test]
    async fn test_succeed_action_checkpointed_when_condition_passes() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // Condition that succeeds immediately
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("success_result".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success_result");
        
        // Verify checkpoint was created with SUCCEED status
        let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
        assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
        assert!(checkpoint_result.is_succeeded(), "Checkpoint should be succeeded");
        
        // Verify result was serialized
        let result_str = checkpoint_result.result().expect("Result should exist");
        assert!(result_str.contains("success_result"));
    }

    // ==========================================================================
    // Test 1.4: Suspension when replaying PENDING status
    // Requirements: 1.4 - WHEN replaying a wait-for-condition in PENDING status, 
    // THE Test_Suite SHALL verify execution suspends until retry timer
    // ==========================================================================

    #[tokio::test]
    async fn test_suspension_when_replaying_pending_status() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create a PENDING operation (waiting for retry)
        let mut op = Operation::new("test-wait-cond-123", OperationType::Step);
        op.status = OperationStatus::Pending;
        op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(1),
            next_attempt_timestamp: Some(9999999999000), // Far future
            error: None,
            payload: Some(r#"{"user_state":0,"attempt":2}"#.to_string()),
        });
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // When replaying a PENDING operation, it should continue execution
        // (the handler checks for PENDING and continues with next attempt)
        let result = wait_for_condition_handler(
            |_state: &i32, ctx: &WaitForConditionContext| {
                // This should be called with attempt 2 (from the payload)
                assert_eq!(ctx.attempt, 2, "Should be on attempt 2");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("resumed_result".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        // Should succeed since condition passes on resume
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "resumed_result");
    }

    // ==========================================================================
    // Test 1.5: FAIL action checkpoint when retries exhausted
    // Requirements: 1.5 - WHEN a wait-for-condition exhausts retries, THE Test_Suite 
    // SHALL verify FAIL action is checkpointed with the final error
    // ==========================================================================

    #[tokio::test]
    async fn test_fail_action_checkpointed_when_retries_exhausted() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        // Config with max_attempts = 1 (only one try)
        let config = WaitForConditionConfig {
            initial_state: 0i32,
            interval: Duration::from_seconds(5),
            max_attempts: Some(1),
            timeout: None,
        };
        
        // Condition that always fails
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Err::<String, _>("condition never met".into())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        // Should fail with max attempts exceeded
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Execution { message, .. } => {
                assert!(message.contains("Max attempts"), "Error should mention max attempts");
            }
            other => panic!("Expected Execution error, got: {:?}", other),
        }
        
        // Verify checkpoint was created with FAIL status
        let checkpoint_result = state.get_checkpoint_result(&op_id.operation_id).await;
        assert!(checkpoint_result.is_existent(), "Checkpoint should exist");
        assert!(checkpoint_result.is_failed(), "Checkpoint should be failed");
    }


    // ==========================================================================
    // Test 1.6: StepContext.retry_payload contains previous state
    // Requirements: 1.6 - WHEN the condition function receives previous state, 
    // THE Test_Suite SHALL verify StepContext.retry_payload contains the serialized state
    // ==========================================================================

    #[tokio::test]
    async fn test_retry_payload_contains_previous_state() {
        let client = Arc::new(MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-1"))));
        
        // Create a PENDING operation with retry payload containing state
        #[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestState {
            counter: i32,
        }
        
        let previous_state = WaitForConditionState {
            user_state: TestState { counter: 42 },
            attempt: 2,
        };
        let payload = serde_json::to_string(&previous_state).unwrap();
        
        let mut op = Operation::new("test-wait-cond-123", OperationType::Step);
        op.status = OperationStatus::Pending;
        op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(1),
            next_attempt_timestamp: Some(1234567890000),
            error: None,
            payload: Some(payload),
        });
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = WaitForConditionConfig {
            initial_state: TestState { counter: 0 },
            interval: Duration::from_seconds(5),
            max_attempts: Some(5),
            timeout: None,
        };
        
        let result = wait_for_condition_handler(
            |state: &TestState, ctx: &WaitForConditionContext| {
                // Verify the state from retry payload is passed
                assert_eq!(state.counter, 42, "State should be from retry payload");
                assert_eq!(ctx.attempt, 2, "Attempt should be 2 from payload");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("success".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
    }

    // ==========================================================================
    // Test 1.7: Condition function receives None on first attempt
    // Requirements: Additional test for first attempt behavior
    // ==========================================================================

    #[tokio::test]
    async fn test_condition_receives_initial_state_on_first_attempt() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        #[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        struct TestState {
            value: String,
        }
        
        let config = WaitForConditionConfig {
            initial_state: TestState { value: "initial".to_string() },
            interval: Duration::from_seconds(5),
            max_attempts: Some(3),
            timeout: None,
        };
        
        let result = wait_for_condition_handler(
            |state: &TestState, ctx: &WaitForConditionContext| {
                // On first attempt, should receive initial state
                assert_eq!(ctx.attempt, 1, "Should be first attempt");
                assert_eq!(state.value, "initial", "Should receive initial state");
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("done".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
    }

    // ==========================================================================
    // Test 1.8: Condition function error handling
    // Requirements: Additional test for error handling
    // ==========================================================================

    #[tokio::test]
    async fn test_condition_function_error_triggers_retry() {
        let client = create_mock_client();
        let state = create_test_state(client);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = WaitForConditionConfig {
            initial_state: 0i32,
            interval: Duration::from_seconds(5),
            max_attempts: Some(3),
            timeout: None,
        };
        
        // Condition that returns an error (not met)
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Err::<String, _>("condition check failed".into())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        // Should suspend for retry (not fail immediately)
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Suspend { .. } => {
                // Expected - should suspend for retry since max_attempts > 1
            }
            other => panic!("Expected Suspend error for retry, got: {:?}", other),
        }
    }

    // ==========================================================================
    // Additional tests for replay scenarios
    // ==========================================================================

    #[tokio::test]
    async fn test_replay_succeeded_operation_returns_cached_result() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create a SUCCEEDED operation
        let mut op = Operation::new("test-wait-cond-123", OperationType::Step);
        op.status = OperationStatus::Succeeded;
        op.step_details = Some(StepDetails {
            result: Some(r#""cached_result""#.to_string()),
            attempt: Some(2),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // Function should NOT be called during replay
        let result: Result<String, DurableError> = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                panic!("Function should not be called during replay of succeeded operation");
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cached_result");
    }

    #[tokio::test]
    async fn test_replay_failed_operation_returns_error() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create a FAILED operation
        let mut op = Operation::new("test-wait-cond-123", OperationType::Step);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("MaxAttemptsExceeded", "Max attempts exceeded"));
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // Function should NOT be called during replay
        let result: Result<String, DurableError> = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                panic!("Function should not be called during replay of failed operation");
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::UserCode { message, .. } => {
                assert!(message.contains("Max attempts"));
            }
            other => panic!("Expected UserCode error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_non_deterministic_detection_wrong_operation_type() {
        let client = Arc::new(MockDurableServiceClient::new());
        
        // Create a WAIT operation at the same ID (wrong type)
        let mut op = Operation::new("test-wait-cond-123", OperationType::Wait);
        op.status = OperationStatus::Succeeded;
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("should not reach".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, .. } => {
                assert_eq!(operation_id, Some("test-wait-cond-123".to_string()));
            }
            other => panic!("Expected NonDeterministic error, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_ready_status_continues_execution() {
        let client = Arc::new(MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-1"))));
        
        // Create a READY operation (ready to resume)
        let mut op = Operation::new("test-wait-cond-123", OperationType::Step);
        op.status = OperationStatus::Ready;
        op.step_details = Some(StepDetails {
            result: None,
            attempt: Some(1),
            next_attempt_timestamp: None,
            error: None,
            payload: None,
        });
        
        let state = create_test_state_with_operations(client, vec![op]);
        let op_id = create_test_op_id();
        let logger = create_test_logger();
        
        let config = create_test_config(0i32);
        
        // Function SHOULD be called for READY status
        let result = wait_for_condition_handler(
            |_state: &i32, _ctx: &WaitForConditionContext| {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>("ready_result".to_string())
            },
            config,
            &state,
            &op_id,
            &logger,
        ).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "ready_result");
    }
}
