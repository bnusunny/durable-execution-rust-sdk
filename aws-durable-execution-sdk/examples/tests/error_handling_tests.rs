//! Tests for error_handling examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError, OperationType, TerminationReason};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessingInput {
    pub value: i32,
    pub should_fail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessingResult {
    pub success: bool,
    pub message: String,
}

/// Handler from error_handling/step_error example (without macro for testing)
async fn step_error_handler(
    event: ProcessingInput,
    ctx: DurableContext,
) -> Result<ProcessingResult, DurableError> {
    let result = ctx
        .step_named(
            "process_value",
            |_| {
                if event.should_fail {
                    Err("Processing failed due to invalid input".into())
                } else {
                    Ok(event.value * 2)
                }
            },
            None,
        )
        .await;

    match result {
        Ok(processed_value) => Ok(ProcessingResult {
            success: true,
            message: format!("Processed value: {}", processed_value),
        }),
        Err(_) => Err(DurableError::Execution {
            message: "Step processing failed".to_string(),
            termination_reason: TerminationReason::ExecutionError,
        }),
    }
}

#[tokio::test]
async fn test_step_error_success() {
    LocalDurableTestRunner::<ProcessingInput, ProcessingResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_error_handler);
    let input = ProcessingInput {
        value: 42,
        should_fail: false,
    };
    let result = runner.run(input).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let processing_result = result.get_result().unwrap();
    assert!(processing_result.success);
    assert_eq!(processing_result.message, "Processed value: 84");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have step operations");

    let process_step = step_ops.first().unwrap();
    assert_eq!(process_step.name, Some("process_value".to_string()));

    assert_event_signatures(operations, "tests/history/step_error_success.history.json");

    LocalDurableTestRunner::<ProcessingInput, ProcessingResult>::teardown_test_environment()
        .await
        .unwrap();
}

#[tokio::test]
async fn test_step_error_failure() {
    LocalDurableTestRunner::<ProcessingInput, ProcessingResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_error_handler);
    let input = ProcessingInput {
        value: 42,
        should_fail: true,
    };
    let result = runner.run(input).await.unwrap();

    // The handler returns an error when should_fail is true
    assert_eq!(result.get_status(), ExecutionStatus::Failed);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_event_signatures(operations, "tests/history/step_error_failure.history.json");

    LocalDurableTestRunner::<ProcessingInput, ProcessingResult>::teardown_test_environment()
        .await
        .unwrap();
}
