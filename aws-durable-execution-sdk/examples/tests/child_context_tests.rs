//! Tests for child_context examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError, OperationType};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Child Context Basic Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubWorkflowResult {
    pub step1: String,
    pub step2: String,
}

/// Handler from child_context/basic example (without macro for testing)
async fn child_context_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<SubWorkflowResult, DurableError> {
    let result = ctx
        .run_in_child_context_named(
            "sub_workflow",
            |child_ctx| {
                Box::pin(async move {
                    let step1: String = child_ctx
                        .step_named(
                            "child_step_1",
                            |_| Ok("child step 1 done".to_string()),
                            None,
                        )
                        .await?;

                    let step2: String = child_ctx
                        .step_named(
                            "child_step_2",
                            |_| Ok("child step 2 done".to_string()),
                            None,
                        )
                        .await?;

                    Ok(SubWorkflowResult { step1, step2 })
                })
            },
            None,
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_child_context_basic() {
    LocalDurableTestRunner::<serde_json::Value, SubWorkflowResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(child_context_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let sub_result = result.get_result().unwrap();
    assert_eq!(sub_result.step1, "child step 1 done");
    assert_eq!(sub_result.step2, "child step 2 done");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(!context_ops.is_empty(), "Should have context operations");

    assert_event_signatures(operations, "tests/history/child_context_basic.history.json");

    LocalDurableTestRunner::<serde_json::Value, SubWorkflowResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Child Context Nested Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NestedResult {
    pub level: u32,
    pub value: String,
}

/// Handler from child_context/nested example (without macro for testing)
async fn child_context_nested_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<NestedResult>, DurableError> {
    let mut results = Vec::new();

    let level1_result = ctx
        .run_in_child_context_named(
            "level_1",
            |child_ctx| {
                Box::pin(async move {
                    let result: NestedResult = child_ctx
                        .step(
                            |_| {
                                Ok(NestedResult {
                                    level: 1,
                                    value: "level 1 complete".to_string(),
                                })
                            },
                            None,
                        )
                        .await?;

                    let level2_result = child_ctx
                        .run_in_child_context_named(
                            "level_2",
                            |nested_ctx| {
                                Box::pin(async move {
                                    nested_ctx
                                        .step(
                                            |_| {
                                                Ok(NestedResult {
                                                    level: 2,
                                                    value: "level 2 complete".to_string(),
                                                })
                                            },
                                            None,
                                        )
                                        .await
                                })
                            },
                            None,
                        )
                        .await?;

                    Ok((result, level2_result))
                })
            },
            None,
        )
        .await?;

    results.push(level1_result.0);
    results.push(level1_result.1);

    Ok(results)
}

#[tokio::test]
async fn test_child_context_nested() {
    LocalDurableTestRunner::<serde_json::Value, Vec<NestedResult>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(child_context_nested_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let nested_results = result.get_result().unwrap();
    assert_eq!(nested_results.len(), 2);
    assert_eq!(nested_results[0].level, 1);
    assert_eq!(nested_results[0].value, "level 1 complete");
    assert_eq!(nested_results[1].level, 2);
    assert_eq!(nested_results[1].value, "level 2 complete");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(
        context_ops.len() >= 2,
        "Should have at least 2 context operations for nested contexts"
    );

    assert_event_signatures(operations, "tests/history/child_context_nested.history.json");

    LocalDurableTestRunner::<serde_json::Value, Vec<NestedResult>>::teardown_test_environment()
        .await
        .unwrap();
}
