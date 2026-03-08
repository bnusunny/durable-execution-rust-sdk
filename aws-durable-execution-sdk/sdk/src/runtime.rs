//! Runtime support for the `#[durable_execution]` macro.
//!
//! This module contains the runtime logic that was previously generated inline
//! by the proc macro. By extracting it into library code, we get:
//!
//! - Unit-testable runtime logic
//! - A single copy in the binary (no per-handler duplication)
//! - Readable error messages pointing to real source files
//! - Bug fixes ship in the SDK crate, not the macro crate
//!
//! Users typically don't interact with this module directly — the
//! `#[durable_execution]` macro calls [`run_durable_handler`] automatically.
//! However, advanced users can call it directly to skip the macro.
//!
//! # Requirements
//!
//! - 15.1: THE Lambda_Integration SHALL provide a `#[durable_execution]` attribute macro for handler functions
//! - 15.3: THE Lambda_Integration SHALL create ExecutionState and DurableContext for the handler

use std::future::Future;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::client::{LambdaDurableServiceClient, SharedDurableServiceClient};
use crate::context::DurableContext;
use crate::error::{DurableError, ErrorObject};
use crate::lambda::{DurableExecutionInvocationInput, DurableExecutionInvocationOutput};
use crate::operation::OperationType;
use crate::state::{CheckpointBatcherConfig, ExecutionState};
use crate::termination::TerminationManager;

/// SDK name for user-agent identification.
const SDK_NAME: &str = "durable-execution-sdk-rust";

/// SDK version for user-agent identification (from Cargo.toml).
const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Maximum response payload size (6MB Lambda limit).
const MAX_RESPONSE_SIZE: usize = 6 * 1024 * 1024;

/// Queue buffer size for the checkpoint batcher.
const CHECKPOINT_QUEUE_BUFFER: usize = 100;

/// Timeout in seconds for waiting on the batcher to drain during cleanup.
const BATCHER_DRAIN_TIMEOUT_SECS: u64 = 5;

/// Extracts the user's event from a [`DurableExecutionInvocationInput`].
///
/// Tries these sources in order:
/// 1. Top-level `Input` field (JSON value)
/// 2. `ExecutionDetails.InputPayload` from the EXECUTION operation (JSON string)
/// 3. `null` deserialization (for types with defaults, e.g. `Option<T>` or `()`)
///
/// # Errors
///
/// Returns a [`DurableExecutionInvocationOutput`] with `FAILED` status if
/// deserialization fails from all sources.
pub fn extract_event<E: DeserializeOwned>(
    input: &DurableExecutionInvocationInput,
) -> Result<E, DurableExecutionInvocationOutput> {
    // Try top-level Input first
    if let Some(value) = &input.input {
        return serde_json::from_value(value.clone()).map_err(|e| {
            DurableExecutionInvocationOutput::failed(ErrorObject::new(
                "DeserializationError",
                format!("Failed to deserialize event from Input: {}", e),
            ))
        });
    }

    // Try ExecutionDetails.InputPayload from the EXECUTION operation
    let execution_op = input
        .initial_execution_state
        .operations
        .iter()
        .find(|op| op.operation_type == OperationType::Execution);

    if let Some(op) = execution_op {
        if let Some(details) = &op.execution_details {
            if let Some(payload) = &details.input_payload {
                return serde_json::from_str::<E>(payload).map_err(|e| {
                    DurableExecutionInvocationOutput::failed(ErrorObject::new(
                        "DeserializationError",
                        format!(
                            "Failed to deserialize event from ExecutionDetails.InputPayload: {}",
                            e
                        ),
                    ))
                });
            }
        }
    }

    // Fall back to null deserialization (supports Option<T>, (), etc.)
    serde_json::from_value(serde_json::Value::Null).map_err(|_| {
        DurableExecutionInvocationOutput::failed(ErrorObject::new(
            "DeserializationError",
            "No input provided and event type does not support default",
        ))
    })
}

/// Processes the handler result into a [`DurableExecutionInvocationOutput`].
///
/// Handles three cases:
/// - `Ok(value)` → serialize, checkpoint if >6MB, return `SUCCEEDED`
/// - `Err(Suspend)` → return `PENDING`
/// - `Err(other)` → return `FAILED` with error details
async fn process_result<R: Serialize>(
    result: Result<R, DurableError>,
    state: &Arc<ExecutionState>,
    durable_execution_arn: &str,
) -> DurableExecutionInvocationOutput {
    match result {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(json) => {
                if json.len() > MAX_RESPONSE_SIZE {
                    checkpoint_large_result(&json, state, durable_execution_arn).await
                } else {
                    DurableExecutionInvocationOutput::succeeded(Some(json))
                }
            }
            Err(e) => DurableExecutionInvocationOutput::failed(ErrorObject::new(
                "SerializationError",
                format!("Failed to serialize result: {}", e),
            )),
        },
        Err(DurableError::Suspend { .. }) => DurableExecutionInvocationOutput::pending(),
        Err(error) => DurableExecutionInvocationOutput::failed(ErrorObject::from(&error)),
    }
}

/// Checkpoints a large result (>6MB) and returns a reference to it.
async fn checkpoint_large_result(
    json: &str,
    state: &Arc<ExecutionState>,
    durable_execution_arn: &str,
) -> DurableExecutionInvocationOutput {
    let result_op_id = format!(
        "__result__{}",
        crate::replay_safe::uuid_string_from_operation(durable_execution_arn, 0)
    );

    let update = crate::operation::OperationUpdate::succeed(
        &result_op_id,
        OperationType::Execution,
        Some(json.to_string()),
    );

    match state.create_checkpoint(update, true).await {
        Ok(()) => DurableExecutionInvocationOutput::checkpointed_result(&result_op_id, json.len()),
        Err(e) => DurableExecutionInvocationOutput::failed(ErrorObject::new(
            "CheckpointError",
            format!("Failed to checkpoint large result: {}", e),
        )),
    }
}

/// Runs a durable execution handler within the Lambda runtime.
///
/// This is the core runtime function that the `#[durable_execution]` macro delegates to.
/// It handles the full lifecycle:
///
/// 1. Extract the user's event from the Lambda input
/// 2. Set up `ExecutionState`, checkpoint batcher, and `DurableContext`
/// 3. Call the user's handler
/// 4. Process the result (serialize, checkpoint large results, map errors)
/// 5. Clean up (drain batcher, drop state)
///
/// # Type Parameters
///
/// - `E`: The user's event type (must implement `DeserializeOwned`)
/// - `R`: The user's result type (must implement `Serialize`)
/// - `Fut`: The future returned by the handler
/// - `F`: The handler function
///
/// # Example
///
/// ```rust,ignore
/// use durable_execution_sdk::runtime::run_durable_handler;
///
/// // Called automatically by #[durable_execution], but can be used directly:
/// pub async fn my_handler(
///     event: LambdaEvent<DurableExecutionInvocationInput>,
/// ) -> Result<DurableExecutionInvocationOutput, lambda_runtime::Error> {
///     run_durable_handler(event, |event: MyEvent, ctx| async move {
///         let result = ctx.step(|_| Ok(42), None).await?;
///         Ok(MyResult { value: result })
///     }).await
/// }
/// ```
///
/// # Requirements
///
/// - 15.1: THE Lambda_Integration SHALL provide a `#[durable_execution]` attribute macro for handler functions
/// - 15.3: THE Lambda_Integration SHALL create ExecutionState and DurableContext for the handler
/// - 15.5: WHEN the handler returns successfully, THE Lambda_Integration SHALL return SUCCEEDED status
/// - 15.6: WHEN the handler fails, THE Lambda_Integration SHALL return FAILED status with error
/// - 15.7: WHEN execution suspends, THE Lambda_Integration SHALL return PENDING status
/// - 15.8: THE Lambda_Integration SHALL handle large responses by checkpointing before returning
pub async fn run_durable_handler<E, R, Fut, F>(
    lambda_event: lambda_runtime::LambdaEvent<DurableExecutionInvocationInput>,
    handler: F,
) -> Result<DurableExecutionInvocationOutput, lambda_runtime::Error>
where
    E: DeserializeOwned,
    R: Serialize,
    Fut: Future<Output = Result<R, DurableError>>,
    F: FnOnce(E, DurableContext) -> Fut,
{
    let (durable_input, lambda_context) = lambda_event.into_parts();

    // Extract the user's event
    let user_event: E = match extract_event(&durable_input) {
        Ok(event) => event,
        Err(output) => return Ok(output),
    };

    // Create termination manager from Lambda context
    let termination_mgr = TerminationManager::from_lambda_context(&lambda_context);

    // Create the service client
    let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let service_client: SharedDurableServiceClient =
        Arc::new(LambdaDurableServiceClient::from_aws_config_with_user_agent(
            &aws_config,
            SDK_NAME,
            SDK_VERSION,
        ));

    // Create ExecutionState with batcher
    let batcher_config = CheckpointBatcherConfig::default();
    let (state, mut batcher) = ExecutionState::with_batcher(
        &durable_input.durable_execution_arn,
        &durable_input.checkpoint_token,
        durable_input.initial_execution_state,
        service_client,
        batcher_config,
        CHECKPOINT_QUEUE_BUFFER,
    );
    let state = Arc::new(state);

    // Spawn the checkpoint batcher task
    let batcher_handle = tokio::spawn(async move {
        batcher.run().await;
    });

    // Create DurableContext and call the handler, racing against timeout
    let durable_ctx = DurableContext::from_lambda_context(state.clone(), lambda_context);

    let output = tokio::select! {
        result = handler(user_event, durable_ctx) => {
            // Handler completed normally (Req 5.3)
            process_result(result, &state, &durable_input.durable_execution_arn).await
        }
        _ = termination_mgr.wait_for_timeout() => {
            // Timeout approaching — flush pending checkpoints and return PENDING (Req 5.2)
            DurableExecutionInvocationOutput::pending()
        }
    };

    // Drop the state to close the checkpoint queue and stop the batcher
    drop(state);

    // Wait for batcher to finish (with timeout)
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(BATCHER_DRAIN_TIMEOUT_SECS),
        batcher_handle,
    )
    .await;

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lambda::InitialExecutionState;
    use crate::operation::{ExecutionDetails, Operation};
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
    struct TestEvent {
        order_id: String,
        amount: f64,
    }

    fn make_input(
        input: Option<serde_json::Value>,
        operations: Vec<Operation>,
    ) -> DurableExecutionInvocationInput {
        DurableExecutionInvocationInput {
            durable_execution_arn:
                "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc".to_string(),
            checkpoint_token: "token".to_string(),
            initial_execution_state: InitialExecutionState {
                operations,
                next_marker: None,
            },
            input,
        }
    }

    // ========================================================================
    // extract_event tests
    // ========================================================================

    #[test]
    fn test_extract_event_from_top_level_input() {
        let input = make_input(
            Some(serde_json::json!({"order_id": "ORD-1", "amount": 99.99})),
            vec![],
        );
        let event: TestEvent = extract_event(&input).unwrap();
        assert_eq!(event.order_id, "ORD-1");
        assert_eq!(event.amount, 99.99);
    }

    #[test]
    fn test_extract_event_from_execution_details_payload() {
        let mut op = Operation::new("exec-1", OperationType::Execution);
        op.execution_details = Some(ExecutionDetails {
            input_payload: Some(r#"{"order_id":"ORD-2","amount":50.0}"#.to_string()),
        });
        let input = make_input(None, vec![op]);
        let event: TestEvent = extract_event(&input).unwrap();
        assert_eq!(event.order_id, "ORD-2");
        assert_eq!(event.amount, 50.0);
    }

    #[test]
    fn test_extract_event_falls_back_to_null_for_option() {
        let input = make_input(None, vec![]);
        let event: Option<TestEvent> = extract_event(&input).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn test_extract_event_fails_when_no_input_and_type_requires_fields() {
        let input = make_input(None, vec![]);
        let result: Result<TestEvent, _> = extract_event(&input);
        assert!(result.is_err());
        let output = result.unwrap_err();
        assert!(output.is_failed());
        assert!(output
            .error
            .unwrap()
            .error_message
            .contains("does not support default"));
    }

    #[test]
    fn test_extract_event_top_level_input_takes_priority() {
        let mut op = Operation::new("exec-1", OperationType::Execution);
        op.execution_details = Some(ExecutionDetails {
            input_payload: Some(r#"{"order_id":"FROM-PAYLOAD","amount":1.0}"#.to_string()),
        });
        let input = make_input(
            Some(serde_json::json!({"order_id": "FROM-INPUT", "amount": 2.0})),
            vec![op],
        );
        let event: TestEvent = extract_event(&input).unwrap();
        assert_eq!(event.order_id, "FROM-INPUT");
    }

    #[test]
    fn test_extract_event_bad_top_level_input_returns_error() {
        let input = make_input(Some(serde_json::json!({"wrong_field": true})), vec![]);
        let result: Result<TestEvent, _> = extract_event(&input);
        assert!(result.is_err());
        let output = result.unwrap_err();
        assert!(output.is_failed());
        assert!(output
            .error
            .unwrap()
            .error_message
            .contains("Failed to deserialize event from Input"));
    }

    #[test]
    fn test_extract_event_bad_payload_returns_error() {
        let mut op = Operation::new("exec-1", OperationType::Execution);
        op.execution_details = Some(ExecutionDetails {
            input_payload: Some("not valid json".to_string()),
        });
        let input = make_input(None, vec![op]);
        let result: Result<TestEvent, _> = extract_event(&input);
        assert!(result.is_err());
        let output = result.unwrap_err();
        assert!(output
            .error
            .unwrap()
            .error_message
            .contains("ExecutionDetails.InputPayload"));
    }

    #[test]
    fn test_extract_event_execution_op_without_details_falls_back() {
        let op = Operation::new("exec-1", OperationType::Execution);
        // No execution_details set
        let input = make_input(None, vec![op]);
        let event: Option<TestEvent> = extract_event(&input).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn test_extract_event_execution_op_without_payload_falls_back() {
        let mut op = Operation::new("exec-1", OperationType::Execution);
        op.execution_details = Some(ExecutionDetails {
            input_payload: None,
        });
        let input = make_input(None, vec![op]);
        let event: Option<TestEvent> = extract_event(&input).unwrap();
        assert!(event.is_none());
    }

    // ========================================================================
    // process_result tests
    // ========================================================================

    #[tokio::test]
    async fn test_process_result_success() {
        let client = Arc::new(crate::client::MockDurableServiceClient::new());
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc",
            "token",
            InitialExecutionState::new(),
            client,
        ));
        let output = process_result(Ok("hello"), &state, "test-arn").await;
        assert!(output.is_succeeded());
        assert_eq!(output.result.unwrap(), "\"hello\"");
    }

    #[tokio::test]
    async fn test_process_result_suspend() {
        let client = Arc::new(crate::client::MockDurableServiceClient::new());
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc",
            "token",
            InitialExecutionState::new(),
            client,
        ));
        let result: Result<String, DurableError> = Err(DurableError::suspend());
        let output = process_result(result, &state, "test-arn").await;
        assert!(output.is_pending());
    }

    #[tokio::test]
    async fn test_process_result_error() {
        let client = Arc::new(crate::client::MockDurableServiceClient::new());
        let state = Arc::new(ExecutionState::new(
            "arn:aws:lambda:us-east-1:123456789012:function:test:durable:abc",
            "token",
            InitialExecutionState::new(),
            client,
        ));
        let result: Result<String, DurableError> = Err(DurableError::execution("something broke"));
        let output = process_result(result, &state, "test-arn").await;
        assert!(output.is_failed());
        assert!(output
            .error
            .unwrap()
            .error_message
            .contains("something broke"));
    }
}
