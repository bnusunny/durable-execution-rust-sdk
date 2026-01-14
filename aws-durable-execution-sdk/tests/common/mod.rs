//! Shared test utilities for integration tests.
//!
//! This module provides helper functions and proptest strategies for creating
//! test fixtures used across integration tests.

#![allow(dead_code)] // These utilities are used by other integration test files

use std::sync::Arc;

use aws_durable_execution_sdk::client::{
    CheckpointResponse, DurableServiceClient, GetOperationsResponse, NewExecutionState,
};
use aws_durable_execution_sdk::error::{DurableError, ErrorObject};
use aws_durable_execution_sdk::operation::{
    CallbackDetails, ChainedInvokeDetails, ContextDetails,
    ExecutionDetails, Operation, OperationAction, OperationStatus, OperationType,
    OperationUpdate, StepDetails, WaitDetails,
};
use async_trait::async_trait;
use proptest::prelude::*;
use std::sync::Mutex;

// =============================================================================
// Mock Client for Integration Tests
// =============================================================================

/// A mock implementation of DurableServiceClient for integration tests.
///
/// This mock client allows tests to configure specific responses for checkpoint
/// and get_operations calls, enabling testing of various scenarios without
/// requiring actual AWS infrastructure.
pub struct MockDurableServiceClient {
    checkpoint_responses: Mutex<Vec<Result<CheckpointResponse, DurableError>>>,
    get_operations_responses: Mutex<Vec<Result<GetOperationsResponse, DurableError>>>,
    /// Track checkpoint calls for verification
    checkpoint_calls: Mutex<Vec<CheckpointCall>>,
}

/// Record of a checkpoint call for test verification.
#[derive(Debug, Clone)]
pub struct CheckpointCall {
    pub durable_execution_arn: String,
    pub checkpoint_token: String,
    pub operations: Vec<OperationUpdate>,
}

impl MockDurableServiceClient {
    /// Creates a new MockDurableServiceClient with no pre-configured responses.
    pub fn new() -> Self {
        Self {
            checkpoint_responses: Mutex::new(Vec::new()),
            get_operations_responses: Mutex::new(Vec::new()),
            checkpoint_calls: Mutex::new(Vec::new()),
        }
    }

    /// Adds a checkpoint response to be returned on the next checkpoint call.
    pub fn with_checkpoint_response(self, response: Result<CheckpointResponse, DurableError>) -> Self {
        self.checkpoint_responses.lock().unwrap().push(response);
        self
    }

    /// Adds a get_operations response to be returned on the next get_operations call.
    pub fn with_get_operations_response(
        self,
        response: Result<GetOperationsResponse, DurableError>,
    ) -> Self {
        self.get_operations_responses.lock().unwrap().push(response);
        self
    }

    /// Adds multiple default checkpoint responses.
    pub fn with_checkpoint_responses(self, count: usize) -> Self {
        let mut responses = self.checkpoint_responses.lock().unwrap();
        for i in 0..count {
            responses.push(Ok(CheckpointResponse {
                checkpoint_token: format!("token-{}", i),
                new_execution_state: None,
            }));
        }
        drop(responses);
        self
    }

    /// Adds a checkpoint response that includes new execution state with operations.
    pub fn with_checkpoint_response_with_operations(
        self,
        token: &str,
        operations: Vec<Operation>,
    ) -> Self {
        self.checkpoint_responses.lock().unwrap().push(Ok(CheckpointResponse {
            checkpoint_token: token.to_string(),
            new_execution_state: Some(NewExecutionState {
                operations,
                next_marker: None,
            }),
        }));
        self
    }

    /// Adds a checkpoint response that returns a callback_id for callback operations.
    /// This is a special response that dynamically includes the callback_id based on
    /// the operation_id from the checkpoint request.
    pub fn with_checkpoint_response_with_callback(
        self,
        token: &str,
        callback_id: &str,
    ) -> Self {
        // Store the callback_id to be used when generating the response
        let callback_id = callback_id.to_string();
        let token = token.to_string();
        
        // We create a special marker that the checkpoint method will recognize
        // and replace with the actual operation_id from the request
        self.checkpoint_responses.lock().unwrap().push(Ok(CheckpointResponse {
            checkpoint_token: token,
            new_execution_state: Some(NewExecutionState {
                operations: vec![{
                    // Use a special marker that will be replaced with the actual operation_id
                    let mut op = Operation::new("__CALLBACK_PLACEHOLDER__", OperationType::Callback);
                    op.status = OperationStatus::Started;
                    op.callback_details = Some(CallbackDetails {
                        callback_id: Some(callback_id),
                        result: None,
                        error: None,
                    });
                    op
                }],
                next_marker: None,
            }),
        }));
        self
    }

    /// Returns all checkpoint calls made to this mock.
    pub fn get_checkpoint_calls(&self) -> Vec<CheckpointCall> {
        self.checkpoint_calls.lock().unwrap().clone()
    }

    /// Clears all recorded checkpoint calls.
    pub fn clear_checkpoint_calls(&self) {
        self.checkpoint_calls.lock().unwrap().clear();
    }
}

impl Default for MockDurableServiceClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DurableServiceClient for MockDurableServiceClient {
    async fn checkpoint(
        &self,
        durable_execution_arn: &str,
        checkpoint_token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        // Record the call
        self.checkpoint_calls.lock().unwrap().push(CheckpointCall {
            durable_execution_arn: durable_execution_arn.to_string(),
            checkpoint_token: checkpoint_token.to_string(),
            operations: operations.clone(),
        });

        let mut responses = self.checkpoint_responses.lock().unwrap();
        if responses.is_empty() {
            Ok(CheckpointResponse {
                checkpoint_token: "mock-token".to_string(),
                new_execution_state: None,
            })
        } else {
            let mut response = responses.remove(0)?;
            
            // Handle callback placeholder replacement
            // If the response contains a callback operation with placeholder ID,
            // replace it with the actual operation_id from the request
            if let Some(ref mut new_state) = response.new_execution_state {
                for op in &mut new_state.operations {
                    if op.operation_id == "__CALLBACK_PLACEHOLDER__" {
                        // Find the callback operation in the request
                        if let Some(callback_op) = operations.iter().find(|o| o.operation_type == OperationType::Callback) {
                            op.operation_id = callback_op.operation_id.clone();
                        }
                    }
                }
            }
            
            Ok(response)
        }
    }

    async fn get_operations(
        &self,
        _durable_execution_arn: &str,
        _next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        let mut responses = self.get_operations_responses.lock().unwrap();
        if responses.is_empty() {
            Ok(GetOperationsResponse {
                operations: Vec::new(),
                next_marker: None,
            })
        } else {
            responses.remove(0)
        }
    }
}

// =============================================================================
// Test Helper Functions
// =============================================================================

/// Creates a mock client wrapped in Arc for use with ExecutionState.
pub fn create_mock_client() -> Arc<MockDurableServiceClient> {
    Arc::new(MockDurableServiceClient::new().with_checkpoint_responses(10))
}

/// Creates a mock client with specific checkpoint responses.
pub fn create_mock_client_with_responses(
    responses: Vec<Result<CheckpointResponse, DurableError>>,
) -> Arc<MockDurableServiceClient> {
    let mut client = MockDurableServiceClient::new();
    for response in responses {
        client = client.with_checkpoint_response(response);
    }
    Arc::new(client)
}

/// Creates a simple CheckpointResponse with just a token.
pub fn create_checkpoint_response(token: &str) -> CheckpointResponse {
    CheckpointResponse {
        checkpoint_token: token.to_string(),
        new_execution_state: None,
    }
}

/// Creates a CheckpointResponse with operations in the new execution state.
pub fn create_checkpoint_response_with_operations(
    token: &str,
    operations: Vec<Operation>,
) -> CheckpointResponse {
    CheckpointResponse {
        checkpoint_token: token.to_string(),
        new_execution_state: Some(NewExecutionState {
            operations,
            next_marker: None,
        }),
    }
}

/// Creates an Operation with the specified type and status.
pub fn create_operation(id: &str, op_type: OperationType, status: OperationStatus) -> Operation {
    let mut op = Operation::new(id, op_type);
    op.status = status;
    op
}

/// Creates a completed STEP operation with a result.
pub fn create_completed_step(id: &str, result: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Step);
    op.status = OperationStatus::Succeeded;
    op.step_details = Some(StepDetails {
        result: Some(result.to_string()),
        attempt: Some(0),
        next_attempt_timestamp: None,
        error: None,
        payload: None,
    });
    op
}

/// Creates a pending STEP operation (waiting for retry).
pub fn create_pending_step(id: &str, attempt: u32, next_attempt_timestamp: i64) -> Operation {
    let mut op = Operation::new(id, OperationType::Step);
    op.status = OperationStatus::Pending;
    op.step_details = Some(StepDetails {
        result: None,
        attempt: Some(attempt),
        next_attempt_timestamp: Some(next_attempt_timestamp),
        error: None,
        payload: None,
    });
    op
}

/// Creates a failed STEP operation with an error.
pub fn create_failed_step(id: &str, error_type: &str, error_message: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Step);
    op.status = OperationStatus::Failed;
    op.step_details = Some(StepDetails {
        result: None,
        attempt: None,
        next_attempt_timestamp: None,
        error: Some(ErrorObject::new(error_type, error_message)),
        payload: None,
    });
    op
}

/// Creates a completed WAIT operation.
pub fn create_completed_wait(id: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Wait);
    op.status = OperationStatus::Succeeded;
    op.wait_details = Some(WaitDetails {
        scheduled_end_timestamp: Some(1234567890000),
    });
    op
}

/// Creates a pending WAIT operation.
pub fn create_pending_wait(id: &str, scheduled_end_timestamp: i64) -> Operation {
    let mut op = Operation::new(id, OperationType::Wait);
    op.status = OperationStatus::Started;
    op.wait_details = Some(WaitDetails {
        scheduled_end_timestamp: Some(scheduled_end_timestamp),
    });
    op
}

/// Creates a completed CALLBACK operation with a result.
pub fn create_completed_callback(id: &str, callback_id: &str, result: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Callback);
    op.status = OperationStatus::Succeeded;
    op.callback_details = Some(CallbackDetails {
        callback_id: Some(callback_id.to_string()),
        result: Some(result.to_string()),
        error: None,
    });
    op
}

/// Creates a pending CALLBACK operation.
pub fn create_pending_callback(id: &str, callback_id: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Callback);
    op.status = OperationStatus::Started;
    op.callback_details = Some(CallbackDetails {
        callback_id: Some(callback_id.to_string()),
        result: None,
        error: None,
    });
    op
}

/// Creates a timed out CALLBACK operation.
pub fn create_timed_out_callback(id: &str, callback_id: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Callback);
    op.status = OperationStatus::TimedOut;
    op.callback_details = Some(CallbackDetails {
        callback_id: Some(callback_id.to_string()),
        result: None,
        error: Some(ErrorObject::new("TimeoutError", "Callback timed out")),
    });
    op
}

/// Creates a completed INVOKE operation with a result.
pub fn create_completed_invoke(id: &str, result: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Invoke);
    op.status = OperationStatus::Succeeded;
    op.chained_invoke_details = Some(ChainedInvokeDetails {
        result: Some(result.to_string()),
        error: None,
    });
    op
}

/// Creates a started INVOKE operation (in progress).
pub fn create_started_invoke(id: &str) -> Operation {
    let mut op = Operation::new(id, OperationType::Invoke);
    op.status = OperationStatus::Started;
    op
}

/// Creates a CONTEXT operation with the specified status.
pub fn create_context_operation(id: &str, status: OperationStatus, parent_id: Option<&str>) -> Operation {
    let mut op = Operation::new(id, OperationType::Context);
    op.status = status;
    if let Some(pid) = parent_id {
        op.parent_id = Some(pid.to_string());
    }
    op
}

/// Creates an EXECUTION operation (root operation).
pub fn create_execution_operation(id: &str, status: OperationStatus, input: Option<&str>) -> Operation {
    let mut op = Operation::new(id, OperationType::Execution);
    op.status = status;
    if let Some(inp) = input {
        op.execution_details = Some(ExecutionDetails {
            input_payload: Some(inp.to_string()),
        });
    }
    op
}

/// Test ARN constant for use in tests.
pub const TEST_EXECUTION_ARN: &str =
    "arn:aws:lambda:us-east-1:123456789012:function:test-function:durable:test-execution-id";

/// Test checkpoint token constant.
pub const TEST_CHECKPOINT_TOKEN: &str = "initial-test-token";

// =============================================================================
// Proptest Strategies
// =============================================================================

/// Strategy for generating valid OperationType values.
pub fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
    prop_oneof![
        Just(OperationType::Execution),
        Just(OperationType::Step),
        Just(OperationType::Wait),
        Just(OperationType::Callback),
        Just(OperationType::Invoke),
        Just(OperationType::Context),
    ]
}

/// Strategy for generating valid OperationStatus values.
pub fn operation_status_strategy() -> impl Strategy<Value = OperationStatus> {
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

/// Strategy for generating terminal OperationStatus values.
pub fn terminal_status_strategy() -> impl Strategy<Value = OperationStatus> {
    prop_oneof![
        Just(OperationStatus::Succeeded),
        Just(OperationStatus::Failed),
        Just(OperationStatus::Cancelled),
        Just(OperationStatus::TimedOut),
        Just(OperationStatus::Stopped),
    ]
}

/// Strategy for generating non-terminal OperationStatus values.
pub fn non_terminal_status_strategy() -> impl Strategy<Value = OperationStatus> {
    prop_oneof![
        Just(OperationStatus::Started),
        Just(OperationStatus::Pending),
        Just(OperationStatus::Ready),
    ]
}

/// Strategy for generating valid OperationAction values.
pub fn operation_action_strategy() -> impl Strategy<Value = OperationAction> {
    prop_oneof![
        Just(OperationAction::Start),
        Just(OperationAction::Succeed),
        Just(OperationAction::Fail),
        Just(OperationAction::Cancel),
        Just(OperationAction::Retry),
    ]
}

/// Strategy for generating non-empty strings suitable for operation IDs.
pub fn non_empty_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,64}".prop_map(|s| s)
}

/// Strategy for generating valid operation ID strings.
pub fn operation_id_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_-]{0,63}".prop_map(|s| s)
}

/// Strategy for generating valid ExecutionArn strings.
pub fn execution_arn_strategy() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("aws"), Just("aws-cn"), Just("aws-us-gov")],
        prop_oneof![
            Just("us-east-1"),
            Just("us-west-2"),
            Just("eu-west-1"),
            Just("ap-northeast-1"),
            Just("cn-north-1"),
        ],
        "[0-9]{12}",
        "[a-zA-Z][a-zA-Z0-9_-]{0,31}",
        "[a-zA-Z0-9]{8,32}",
    )
        .prop_map(|(partition, region, account, func, exec_id)| {
            format!(
                "arn:{}:lambda:{}:{}:function:{}:durable:{}",
                partition, region, account, func, exec_id
            )
        })
}

/// Strategy for generating optional strings.
pub fn optional_string_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        non_empty_string_strategy().prop_map(Some),
    ]
}

/// Strategy for generating optional result payloads (JSON strings).
pub fn optional_result_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![
        Just(None),
        Just(Some("null".to_string())),
        Just(Some("42".to_string())),
        Just(Some("\"test-result\"".to_string())),
        Just(Some("{\"key\":\"value\"}".to_string())),
        Just(Some("[1,2,3]".to_string())),
    ]
}

/// Strategy for generating ErrorObject instances.
pub fn error_object_strategy() -> impl Strategy<Value = ErrorObject> {
    (non_empty_string_strategy(), non_empty_string_strategy()).prop_map(|(error_type, error_message)| {
        ErrorObject::new(error_type, error_message)
    })
}

/// Strategy for generating optional ErrorObject instances.
pub fn optional_error_strategy() -> impl Strategy<Value = Option<ErrorObject>> {
    prop_oneof![
        Just(None),
        error_object_strategy().prop_map(Some),
    ]
}

/// Strategy for generating valid Operation instances.
///
/// This generates operations with consistent type-specific details based on
/// the operation type.
pub fn operation_strategy() -> impl Strategy<Value = Operation> {
    (
        operation_id_strategy(),
        operation_type_strategy(),
        operation_status_strategy(),
        optional_string_strategy(), // parent_id
        optional_string_strategy(), // name
    )
        .prop_flat_map(|(id, op_type, status, parent_id, name)| {
            // Generate type-specific details based on operation type
            let details_strategy = match op_type {
                OperationType::Step => optional_result_strategy()
                    .prop_map(move |result| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.step_details = Some(StepDetails {
                            result,
                            attempt: Some(0),
                            next_attempt_timestamp: None,
                            error: None,
                            payload: None,
                        });
                        op
                    })
                    .boxed(),
                OperationType::Wait => Just(())
                    .prop_map(move |_| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.wait_details = Some(WaitDetails {
                            scheduled_end_timestamp: Some(1234567890000),
                        });
                        op
                    })
                    .boxed(),
                OperationType::Callback => optional_result_strategy()
                    .prop_map(move |result| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.callback_details = Some(CallbackDetails {
                            callback_id: Some(format!("cb-{}", id.clone())),
                            result,
                            error: None,
                        });
                        op
                    })
                    .boxed(),
                OperationType::Invoke => optional_result_strategy()
                    .prop_map(move |result| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.chained_invoke_details = Some(ChainedInvokeDetails {
                            result,
                            error: None,
                        });
                        op
                    })
                    .boxed(),
                OperationType::Context => optional_result_strategy()
                    .prop_map(move |result| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.context_details = Some(ContextDetails {
                            result,
                            replay_children: Some(true),
                            error: None,
                        });
                        op
                    })
                    .boxed(),
                OperationType::Execution => optional_result_strategy()
                    .prop_map(move |input| {
                        let mut op = Operation::new(id.clone(), op_type);
                        op.status = status;
                        op.parent_id = parent_id.clone();
                        op.name = name.clone();
                        op.execution_details = Some(ExecutionDetails {
                            input_payload: input,
                        });
                        op
                    })
                    .boxed(),
            };
            details_strategy
        })
}

/// Strategy for generating valid OperationUpdate instances.
pub fn operation_update_strategy() -> impl Strategy<Value = OperationUpdate> {
    (
        operation_id_strategy(),
        operation_action_strategy(),
        operation_type_strategy(),
        optional_result_strategy(),
        optional_error_strategy(),
        optional_string_strategy(), // parent_id
        optional_string_strategy(), // name
    )
        .prop_map(|(id, action, op_type, result, error, parent_id, name)| {
            let mut update = match action {
                OperationAction::Start => OperationUpdate::start(&id, op_type),
                OperationAction::Succeed => OperationUpdate::succeed(&id, op_type, result.clone()),
                OperationAction::Fail => {
                    let err = error.clone().unwrap_or_else(|| ErrorObject::new("TestError", "Test error message"));
                    OperationUpdate::fail(&id, op_type, err)
                }
                OperationAction::Cancel => OperationUpdate::cancel(&id, op_type),
                OperationAction::Retry => OperationUpdate::retry(&id, op_type, result.clone(), None),
            };
            update.parent_id = parent_id;
            update.name = name;
            update
        })
}

/// Strategy for generating a batch of OperationUpdate instances.
pub fn operation_update_batch_strategy(max_size: usize) -> impl Strategy<Value = Vec<OperationUpdate>> {
    prop::collection::vec(operation_update_strategy(), 0..=max_size)
}
