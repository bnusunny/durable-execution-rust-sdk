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
