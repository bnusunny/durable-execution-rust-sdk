//! Tests for hello_world example using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};

/// Handler from hello_world example (without macro for testing)
async fn hello_world_handler(
    _event: serde_json::Value,
    _ctx: DurableContext,
) -> Result<String, DurableError> {
    Ok("Hello World!".to_string())
}

#[tokio::test]
async fn test_hello_world() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(hello_world_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "Hello World!");

    // Verify operations - should only have the execution operation
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures
    assert_event_signatures(operations, "tests/history/hello_world.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}
