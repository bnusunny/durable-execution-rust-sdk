//! Tests for invoke examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError, OperationType};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChildRequest {
    pub task_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChildResponse {
    pub task_id: String,
    pub result: String,
}

/// Handler from invoke/basic example (without macro for testing)
/// Note: This test will result in Running status since invoke operations
/// require external Lambda invocation which is not available in local testing.
async fn invoke_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ChildResponse, DurableError> {
    let request = ChildRequest {
        task_id: "task-123".to_string(),
        data: "process this".to_string(),
    };

    let response: ChildResponse = ctx
        .invoke(
            "arn:aws:lambda:us-east-1:123456789012:function:child-function",
            request,
            None,
        )
        .await?;

    Ok(response)
}

#[tokio::test]
async fn test_invoke_basic() {
    LocalDurableTestRunner::<serde_json::Value, ChildResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(invoke_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Invoke operations suspend execution when waiting for the Lambda response
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    let invoke_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Invoke)
        .collect();
    assert!(!invoke_ops.is_empty(), "Should have an invoke operation");

    assert_event_signatures(operations, "tests/history/invoke_basic.history.json");

    LocalDurableTestRunner::<serde_json::Value, ChildResponse>::teardown_test_environment()
        .await
        .unwrap();
}
