//! Replay detection utilities for the AWS Durable Execution SDK.
//!
//! This module provides common replay detection logic used by all operation handlers.
//! It centralizes the logic for:
//! - Checking if an operation was previously checkpointed
//! - Returning stored results for succeeded operations
//! - Returning stored errors for failed operations
//! - Detecting non-deterministic execution when operation types don't match

use serde::de::DeserializeOwned;

use crate::error::{DurableError, TerminationReason};
use crate::operation::OperationType;
use crate::serdes::{JsonSerDes, SerDes, SerDesContext};
use crate::state::CheckpointedResult;

/// Result of replay detection.
///
/// This enum represents the possible outcomes when checking for replay:
/// - `Replayed`: The operation was found in checkpoint and the result is returned
/// - `NotFound`: No checkpoint exists, operation should execute normally
/// - `NonDeterministic`: Operation type mismatch detected
/// - `InProgress`: Operation was started but not completed
#[derive(Debug)]
pub enum ReplayResult<T> {
    /// Operation was replayed successfully with the stored result
    Replayed(T),
    /// No checkpoint found, execute the operation normally
    NotFound,
    /// Operation is in progress (Started state), continue execution
    InProgress,
}

/// Checks for replay and returns the appropriate result.
///
/// This function implements the core replay detection logic:
/// 1. If no checkpoint exists, returns `NotFound`
/// 2. If checkpoint exists but operation type doesn't match, returns `NonDeterministic` error
/// 3. If operation succeeded, deserializes and returns the stored result
/// 4. If operation failed, returns the stored error
/// 5. If operation is in a terminal state (cancelled, timed out, stopped), returns appropriate error
/// 6. If operation is in progress (Started), returns `InProgress`
///
/// # Arguments
///
/// * `checkpoint_result` - The checkpoint result from `ExecutionState::get_checkpoint_result`
/// * `expected_type` - The expected operation type for non-determinism detection
/// * `operation_id` - The operation ID for error messages
/// * `durable_execution_arn` - The ARN for SerDes context
///
/// # Returns
///
/// - `Ok(ReplayResult::Replayed(value))` - Operation was replayed with stored result
/// - `Ok(ReplayResult::NotFound)` - No checkpoint, execute normally
/// - `Ok(ReplayResult::InProgress)` - Operation started but not completed
/// - `Err(DurableError::NonDeterministic)` - Operation type mismatch
/// - `Err(DurableError::UserCode)` - Operation failed with stored error
/// - `Err(DurableError::Execution)` - Operation was cancelled/timed out/stopped
///
/// # Requirements
///
/// - 3.2: Return stored result if operation completed successfully
/// - 3.3: Return stored error if operation failed
/// - 3.4: Execute normally if no checkpoint exists
/// - 3.5: Detect non-deterministic execution when operation types don't match
pub fn check_replay<T>(
    checkpoint_result: &CheckpointedResult,
    expected_type: OperationType,
    operation_id: &str,
    durable_execution_arn: &str,
) -> Result<ReplayResult<T>, DurableError>
where
    T: serde::Serialize + DeserializeOwned,
{
    // Requirement 3.4: Execute normally if no checkpoint exists
    if !checkpoint_result.is_existent() {
        return Ok(ReplayResult::NotFound);
    }

    // Requirement 3.5: Detect non-deterministic execution
    if let Some(op_type) = checkpoint_result.operation_type() {
        if op_type != expected_type {
            return Err(DurableError::NonDeterministic {
                message: format!(
                    "Expected {:?} operation but found {:?} at operation_id {}",
                    expected_type, op_type, operation_id
                ),
                operation_id: Some(operation_id.to_string()),
            });
        }
    }

    // Requirement 3.2: Return stored result if operation succeeded
    if checkpoint_result.is_succeeded() {
        if let Some(result_str) = checkpoint_result.result() {
            let serdes = JsonSerDes::<T>::new();
            let serdes_ctx = SerDesContext::new(operation_id, durable_execution_arn);
            let result = serdes.deserialize(result_str, &serdes_ctx)
                .map_err(|e| DurableError::SerDes {
                    message: format!("Failed to deserialize checkpointed result: {}", e),
                })?;
            return Ok(ReplayResult::Replayed(result));
        }
    }

    // Requirement 3.3: Return stored error if operation failed
    if checkpoint_result.is_failed() {
        if let Some(error) = checkpoint_result.error() {
            return Err(DurableError::UserCode {
                message: error.error_message.clone(),
                error_type: error.error_type.clone(),
                stack_trace: error.stack_trace.clone(),
            });
        } else {
            return Err(DurableError::execution("Operation failed with unknown error"));
        }
    }

    // Handle other terminal states (cancelled, timed out, stopped)
    if checkpoint_result.is_terminal() {
        let status = checkpoint_result.status().unwrap();
        return Err(DurableError::Execution {
            message: format!("Operation was {}", status),
            termination_reason: TerminationReason::StepInterrupted,
        });
    }

    // Operation exists but is not terminal (Started state) - continue execution
    Ok(ReplayResult::InProgress)
}

/// Checks for replay without deserializing the result.
///
/// This is useful for operations like `wait` that don't have a typed result,
/// or when you need to handle the result deserialization separately.
///
/// # Arguments
///
/// * `checkpoint_result` - The checkpoint result from `ExecutionState::get_checkpoint_result`
/// * `expected_type` - The expected operation type for non-determinism detection
/// * `operation_id` - The operation ID for error messages
///
/// # Returns
///
/// - `Ok(true)` - Operation was completed (succeeded or failed handled)
/// - `Ok(false)` - No checkpoint or operation in progress
/// - `Err(DurableError)` - Non-deterministic execution or operation failed
pub fn check_replay_status(
    checkpoint_result: &CheckpointedResult,
    expected_type: OperationType,
    operation_id: &str,
) -> Result<bool, DurableError> {
    // Execute normally if no checkpoint exists
    if !checkpoint_result.is_existent() {
        return Ok(false);
    }

    // Detect non-deterministic execution
    if let Some(op_type) = checkpoint_result.operation_type() {
        if op_type != expected_type {
            return Err(DurableError::NonDeterministic {
                message: format!(
                    "Expected {:?} operation but found {:?} at operation_id {}",
                    expected_type, op_type, operation_id
                ),
                operation_id: Some(operation_id.to_string()),
            });
        }
    }

    // Return true if operation succeeded
    if checkpoint_result.is_succeeded() {
        return Ok(true);
    }

    // Return error if operation failed
    if checkpoint_result.is_failed() {
        if let Some(error) = checkpoint_result.error() {
            return Err(DurableError::UserCode {
                message: error.error_message.clone(),
                error_type: error.error_type.clone(),
                stack_trace: error.stack_trace.clone(),
            });
        } else {
            return Err(DurableError::execution("Operation failed with unknown error"));
        }
    }

    // Handle other terminal states
    if checkpoint_result.is_terminal() {
        let status = checkpoint_result.status().unwrap();
        return Err(DurableError::Execution {
            message: format!("Operation was {}", status),
            termination_reason: TerminationReason::StepInterrupted,
        });
    }

    // Operation in progress
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorObject;
    use crate::operation::{Operation, OperationStatus};

    fn create_succeeded_operation(op_type: OperationType, result: &str) -> CheckpointedResult {
        let mut op = Operation::new("test-op", op_type);
        op.status = OperationStatus::Succeeded;
        op.result = Some(result.to_string());
        CheckpointedResult::new(Some(op))
    }

    fn create_failed_operation(op_type: OperationType, error_msg: &str) -> CheckpointedResult {
        let mut op = Operation::new("test-op", op_type);
        op.status = OperationStatus::Failed;
        op.error = Some(ErrorObject::new("TestError", error_msg));
        CheckpointedResult::new(Some(op))
    }

    fn create_started_operation(op_type: OperationType) -> CheckpointedResult {
        let op = Operation::new("test-op", op_type);
        CheckpointedResult::new(Some(op))
    }

    fn create_cancelled_operation(op_type: OperationType) -> CheckpointedResult {
        let mut op = Operation::new("test-op", op_type);
        op.status = OperationStatus::Cancelled;
        CheckpointedResult::new(Some(op))
    }

    #[test]
    fn test_check_replay_not_found() {
        let checkpoint = CheckpointedResult::empty();
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ReplayResult::NotFound));
    }

    #[test]
    fn test_check_replay_succeeded() {
        let checkpoint = create_succeeded_operation(OperationType::Step, "42");
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_ok());
        match result.unwrap() {
            ReplayResult::Replayed(value) => assert_eq!(value, 42),
            _ => panic!("Expected Replayed"),
        }
    }

    #[test]
    fn test_check_replay_failed() {
        let checkpoint = create_failed_operation(OperationType::Step, "test error");
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::UserCode { message, .. } => {
                assert!(message.contains("test error"));
            }
            _ => panic!("Expected UserCode error"),
        }
    }

    #[test]
    fn test_check_replay_non_deterministic() {
        let checkpoint = create_succeeded_operation(OperationType::Wait, "null");
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step, // Wrong type!
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { operation_id, message } => {
                assert_eq!(operation_id, Some("test-op".to_string()));
                assert!(message.contains("Expected Step"));
                assert!(message.contains("found Wait"));
            }
            _ => panic!("Expected NonDeterministic error"),
        }
    }

    #[test]
    fn test_check_replay_in_progress() {
        let checkpoint = create_started_operation(OperationType::Step);
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ReplayResult::InProgress));
    }

    #[test]
    fn test_check_replay_cancelled() {
        let checkpoint = create_cancelled_operation(OperationType::Step);
        let result: Result<ReplayResult<i32>, DurableError> = check_replay(
            &checkpoint,
            OperationType::Step,
            "test-op",
            "arn:test",
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::Execution { message, .. } => {
                assert!(message.contains("Cancelled"));
            }
            _ => panic!("Expected Execution error"),
        }
    }

    #[test]
    fn test_check_replay_status_not_found() {
        let checkpoint = CheckpointedResult::empty();
        let result = check_replay_status(
            &checkpoint,
            OperationType::Step,
            "test-op",
        );
        
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_check_replay_status_succeeded() {
        let checkpoint = create_succeeded_operation(OperationType::Step, "42");
        let result = check_replay_status(
            &checkpoint,
            OperationType::Step,
            "test-op",
        );
        
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_check_replay_status_failed() {
        let checkpoint = create_failed_operation(OperationType::Step, "test error");
        let result = check_replay_status(
            &checkpoint,
            OperationType::Step,
            "test-op",
        );
        
        assert!(result.is_err());
    }

    #[test]
    fn test_check_replay_status_non_deterministic() {
        let checkpoint = create_succeeded_operation(OperationType::Wait, "null");
        let result = check_replay_status(
            &checkpoint,
            OperationType::Step,
            "test-op",
        );
        
        assert!(result.is_err());
        match result.unwrap_err() {
            DurableError::NonDeterministic { .. } => {}
            _ => panic!("Expected NonDeterministic error"),
        }
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::error::ErrorObject;
    use crate::operation::{Operation, OperationStatus};
    use proptest::prelude::*;

    /// **Feature: durable-execution-rust-sdk, Property 1: Replay Round-Trip Consistency**
    /// **Validates: Requirements 3.2, 3.3, 3.4**
    ///
    /// For any sequence of operations that complete successfully and are checkpointed,
    /// when the function is replayed, each operation SHALL return its checkpointed result
    /// without re-executing the operation's closure.
    mod replay_round_trip_tests {
        use super::*;

        /// Strategy for generating valid operation types
        fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
            prop_oneof![
                Just(OperationType::Step),
                Just(OperationType::Wait),
                Just(OperationType::Callback),
                Just(OperationType::Invoke),
                Just(OperationType::Context),
            ]
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: Replay returns checkpointed result for succeeded operations
            /// For any operation that succeeded with a serialized result,
            /// check_replay SHALL return the deserialized result.
            #[test]
            fn prop_replay_returns_checkpointed_result_for_success(
                result_value in any::<i32>(),
                op_type in operation_type_strategy(),
            ) {
                // Create a succeeded operation with the result
                let mut op = Operation::new("test-op", op_type);
                op.status = OperationStatus::Succeeded;
                op.result = Some(result_value.to_string());
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                // Should return the replayed result
                prop_assert!(replay_result.is_ok(), "Replay should succeed");
                match replay_result.unwrap() {
                    ReplayResult::Replayed(value) => {
                        prop_assert_eq!(value, result_value, "Replayed value should match original");
                    }
                    _ => prop_assert!(false, "Expected Replayed result"),
                }
            }

            /// Property test: Replay returns error for failed operations
            /// For any operation that failed with an error,
            /// check_replay SHALL return the stored error.
            #[test]
            fn prop_replay_returns_error_for_failed_operations(
                error_msg in "[a-zA-Z0-9 ]{1,50}",
                op_type in operation_type_strategy(),
            ) {
                // Create a failed operation with the error
                let mut op = Operation::new("test-op", op_type);
                op.status = OperationStatus::Failed;
                op.error = Some(ErrorObject::new("TestError", &error_msg));
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                // Should return the error
                prop_assert!(replay_result.is_err(), "Replay should return error");
                match replay_result.unwrap_err() {
                    DurableError::UserCode { message, error_type, .. } => {
                        prop_assert!(message.contains(&error_msg), "Error message should match");
                        prop_assert_eq!(error_type, "TestError", "Error type should match");
                    }
                    _ => prop_assert!(false, "Expected UserCode error"),
                }
            }

            /// Property test: No checkpoint returns NotFound
            /// For any operation with no checkpoint,
            /// check_replay SHALL return NotFound.
            #[test]
            fn prop_no_checkpoint_returns_not_found(
                op_type in operation_type_strategy(),
            ) {
                let checkpoint = CheckpointedResult::empty();

                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                prop_assert!(replay_result.is_ok(), "Should succeed");
                prop_assert!(matches!(replay_result.unwrap(), ReplayResult::NotFound), "Should return NotFound");
            }

            /// Property test: Round-trip consistency for serializable types
            /// For any serializable value, checkpointing and replaying
            /// SHALL produce an equivalent value.
            #[test]
            fn prop_round_trip_consistency_string(
                value in "[a-zA-Z0-9]{1,100}",
                op_type in operation_type_strategy(),
            ) {
                // Serialize the value as JSON
                let serialized = serde_json::to_string(&value).unwrap();

                // Create a succeeded operation with the serialized result
                let mut op = Operation::new("test-op", op_type);
                op.status = OperationStatus::Succeeded;
                op.result = Some(serialized);
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay
                let replay_result: Result<ReplayResult<String>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                // Should return the original value
                prop_assert!(replay_result.is_ok(), "Replay should succeed");
                match replay_result.unwrap() {
                    ReplayResult::Replayed(replayed_value) => {
                        prop_assert_eq!(replayed_value, value, "Round-trip should preserve value");
                    }
                    _ => prop_assert!(false, "Expected Replayed result"),
                }
            }

            /// Property test: Round-trip consistency for complex types
            /// For any complex serializable value (struct), checkpointing and replaying
            /// SHALL produce an equivalent value.
            #[test]
            fn prop_round_trip_consistency_complex(
                field1 in any::<i32>(),
                field2 in "[a-zA-Z0-9]{1,50}",
                field3 in any::<bool>(),
                op_type in operation_type_strategy(),
            ) {
                #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
                struct TestStruct {
                    field1: i32,
                    field2: String,
                    field3: bool,
                }

                let value = TestStruct {
                    field1,
                    field2: field2.clone(),
                    field3,
                };

                // Serialize the value as JSON
                let serialized = serde_json::to_string(&value).unwrap();

                // Create a succeeded operation with the serialized result
                let mut op = Operation::new("test-op", op_type);
                op.status = OperationStatus::Succeeded;
                op.result = Some(serialized);
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay
                let replay_result: Result<ReplayResult<TestStruct>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                // Should return the original value
                prop_assert!(replay_result.is_ok(), "Replay should succeed");
                match replay_result.unwrap() {
                    ReplayResult::Replayed(replayed_value) => {
                        prop_assert_eq!(replayed_value, value, "Round-trip should preserve complex value");
                    }
                    _ => prop_assert!(false, "Expected Replayed result"),
                }
            }

            /// Property test: In-progress operations return InProgress
            /// For any operation in Started state,
            /// check_replay SHALL return InProgress.
            #[test]
            fn prop_in_progress_returns_in_progress(
                op_type in operation_type_strategy(),
            ) {
                // Create a started operation (not completed)
                let op = Operation::new("test-op", op_type);
                let checkpoint = CheckpointedResult::new(Some(op));

                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    op_type,
                    "test-op",
                    "arn:test",
                );

                prop_assert!(replay_result.is_ok(), "Should succeed");
                prop_assert!(matches!(replay_result.unwrap(), ReplayResult::InProgress), "Should return InProgress");
            }
        }
    }

    /// **Feature: durable-execution-rust-sdk, Property 10: Non-Deterministic Execution Detection**
    /// **Validates: Requirements 3.5**
    ///
    /// For any replay where the operation type at a given operation_id differs from
    /// the checkpointed operation type, the system SHALL raise NonDeterministicExecutionError.
    mod non_deterministic_detection_tests {
        use super::*;
        use crate::error::ErrorObject;
        use crate::operation::{Operation, OperationStatus};
        use proptest::prelude::*;

        /// Strategy for generating valid operation types
        fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
            prop_oneof![
                Just(OperationType::Step),
                Just(OperationType::Wait),
                Just(OperationType::Callback),
                Just(OperationType::Invoke),
                Just(OperationType::Context),
            ]
        }

        /// Strategy for generating pairs of different operation types
        fn different_operation_types_strategy() -> impl Strategy<Value = (OperationType, OperationType)> {
            (operation_type_strategy(), operation_type_strategy())
                .prop_filter("Types must be different", |(a, b)| a != b)
        }

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]

            /// Property test: Mismatched operation types raise NonDeterministicExecutionError
            /// For any checkpoint with operation type A, when check_replay is called
            /// expecting operation type B (where A != B), it SHALL return NonDeterministicExecutionError.
            #[test]
            fn prop_mismatched_types_raise_non_deterministic_error(
                (checkpointed_type, expected_type) in different_operation_types_strategy(),
                result_value in any::<i32>(),
            ) {
                // Create a succeeded operation with the checkpointed type
                let mut op = Operation::new("test-op", checkpointed_type);
                op.status = OperationStatus::Succeeded;
                op.result = Some(result_value.to_string());
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay with a different expected type
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    expected_type, // Different from checkpointed_type
                    "test-op",
                    "arn:test",
                );

                // Should return NonDeterministic error
                prop_assert!(replay_result.is_err(), "Should return error for mismatched types");
                match replay_result.unwrap_err() {
                    DurableError::NonDeterministic { operation_id, message } => {
                        prop_assert_eq!(operation_id, Some("test-op".to_string()), "Operation ID should match");
                        prop_assert!(message.contains(&format!("{:?}", expected_type)), "Message should mention expected type");
                        prop_assert!(message.contains(&format!("{:?}", checkpointed_type)), "Message should mention found type");
                    }
                    other => prop_assert!(false, "Expected NonDeterministic error, got {:?}", other),
                }
            }

            /// Property test: Matching operation types do not raise error
            /// For any checkpoint with operation type A, when check_replay is called
            /// expecting the same operation type A, it SHALL NOT return NonDeterministicExecutionError.
            #[test]
            fn prop_matching_types_do_not_raise_error(
                op_type in operation_type_strategy(),
                result_value in any::<i32>(),
            ) {
                // Create a succeeded operation
                let mut op = Operation::new("test-op", op_type);
                op.status = OperationStatus::Succeeded;
                op.result = Some(result_value.to_string());
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay with the same expected type
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    op_type, // Same as checkpointed type
                    "test-op",
                    "arn:test",
                );

                // Should NOT return NonDeterministic error
                match &replay_result {
                    Err(DurableError::NonDeterministic { .. }) => {
                        prop_assert!(false, "Should not return NonDeterministic error for matching types");
                    }
                    _ => {} // Any other result is acceptable
                }

                // Should return the replayed result
                prop_assert!(replay_result.is_ok(), "Should succeed for matching types");
                match replay_result.unwrap() {
                    ReplayResult::Replayed(value) => {
                        prop_assert_eq!(value, result_value, "Should return correct value");
                    }
                    _ => prop_assert!(false, "Expected Replayed result"),
                }
            }

            /// Property test: Non-deterministic detection works for failed operations too
            /// For any failed checkpoint with operation type A, when check_replay is called
            /// expecting operation type B (where A != B), it SHALL return NonDeterministicExecutionError
            /// (not the stored error).
            #[test]
            fn prop_non_deterministic_detected_before_error_returned(
                (checkpointed_type, expected_type) in different_operation_types_strategy(),
                error_msg in "[a-zA-Z0-9 ]{1,50}",
            ) {
                // Create a failed operation with the checkpointed type
                let mut op = Operation::new("test-op", checkpointed_type);
                op.status = OperationStatus::Failed;
                op.error = Some(ErrorObject::new("TestError", &error_msg));
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay with a different expected type
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    expected_type, // Different from checkpointed_type
                    "test-op",
                    "arn:test",
                );

                // Should return NonDeterministic error (not UserCode error)
                prop_assert!(replay_result.is_err(), "Should return error");
                match replay_result.unwrap_err() {
                    DurableError::NonDeterministic { .. } => {
                        // This is the expected behavior - non-determinism is detected first
                    }
                    DurableError::UserCode { .. } => {
                        prop_assert!(false, "Should detect non-determinism before returning stored error");
                    }
                    other => prop_assert!(false, "Expected NonDeterministic error, got {:?}", other),
                }
            }

            /// Property test: Non-deterministic detection works for in-progress operations
            /// For any in-progress checkpoint with operation type A, when check_replay is called
            /// expecting operation type B (where A != B), it SHALL return NonDeterministicExecutionError.
            #[test]
            fn prop_non_deterministic_detected_for_in_progress(
                (checkpointed_type, expected_type) in different_operation_types_strategy(),
            ) {
                // Create an in-progress operation (Started state)
                let op = Operation::new("test-op", checkpointed_type);
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay with a different expected type
                let replay_result: Result<ReplayResult<i32>, DurableError> = check_replay(
                    &checkpoint,
                    expected_type, // Different from checkpointed_type
                    "test-op",
                    "arn:test",
                );

                // Should return NonDeterministic error
                prop_assert!(replay_result.is_err(), "Should return error for mismatched types");
                match replay_result.unwrap_err() {
                    DurableError::NonDeterministic { operation_id, .. } => {
                        prop_assert_eq!(operation_id, Some("test-op".to_string()), "Operation ID should match");
                    }
                    other => prop_assert!(false, "Expected NonDeterministic error, got {:?}", other),
                }
            }

            /// Property test: check_replay_status also detects non-determinism
            /// For any checkpoint with operation type A, when check_replay_status is called
            /// expecting operation type B (where A != B), it SHALL return NonDeterministicExecutionError.
            #[test]
            fn prop_check_replay_status_detects_non_determinism(
                (checkpointed_type, expected_type) in different_operation_types_strategy(),
            ) {
                // Create a succeeded operation
                let mut op = Operation::new("test-op", checkpointed_type);
                op.status = OperationStatus::Succeeded;
                let checkpoint = CheckpointedResult::new(Some(op));

                // Check replay status with a different expected type
                let result = check_replay_status(
                    &checkpoint,
                    expected_type, // Different from checkpointed_type
                    "test-op",
                );

                // Should return NonDeterministic error
                prop_assert!(result.is_err(), "Should return error for mismatched types");
                match result.unwrap_err() {
                    DurableError::NonDeterministic { operation_id, .. } => {
                        prop_assert_eq!(operation_id, Some("test-op".to_string()), "Operation ID should match");
                    }
                    other => prop_assert!(false, "Expected NonDeterministic error, got {:?}", other),
                }
            }
        }
    }
}