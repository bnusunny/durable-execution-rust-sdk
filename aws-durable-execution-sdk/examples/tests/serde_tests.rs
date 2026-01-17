//! Tests for serde examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError, OperationType};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SensitiveData {
    pub user_id: String,
    pub data: String,
}

/// Handler from serde/custom_serialization example (without macro for testing)
async fn custom_serialization_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<SensitiveData, DurableError> {
    let data: SensitiveData = ctx
        .step_named(
            "process_data",
            |_| {
                Ok(SensitiveData {
                    user_id: "user-123".to_string(),
                    data: "sensitive information".to_string(),
                })
            },
            None,
        )
        .await?;

    Ok(data)
}

#[tokio::test]
async fn test_custom_serialization() {
    LocalDurableTestRunner::<serde_json::Value, SensitiveData>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(custom_serialization_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let data = result.get_result().unwrap();
    assert_eq!(data.user_id, "user-123");
    assert_eq!(data.data, "sensitive information");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have step operations");

    let process_step = step_ops.first().unwrap();
    assert_eq!(process_step.name, Some("process_data".to_string()));

    assert_event_signatures(
        operations,
        "tests/history/custom_serialization.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, SensitiveData>::teardown_test_environment()
        .await
        .unwrap();
}
