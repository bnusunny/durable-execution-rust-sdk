//! Tests for child_context examples using LocalDurableTestRunner.

use aws_durable_execution_sdk::{DurableContext, DurableError, OperationType};
use aws_durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures;
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

    assert_nodejs_event_signatures(&result, "tests/history/child_context_basic.history.json");

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

    assert_nodejs_event_signatures(&result, "tests/history/child_context_nested.history.json");

    LocalDurableTestRunner::<serde_json::Value, Vec<NestedResult>>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Child Context Large Data Example Handler
// ============================================================================

/// Handler demonstrating a child context with large data and summary generator.
async fn child_context_large_data_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    use aws_durable_execution_sdk::ChildConfig;
    use std::sync::Arc;

    let config = ChildConfig::new().set_summary_generator(Arc::new(|serialized: &str| {
        format!("summary: {} bytes", serialized.len())
    }));

    let result = ctx
        .run_in_child_context_named(
            "large_data_child",
            |child_ctx| {
                Box::pin(async move {
                    let large_data: String = child_ctx
                        .step(
                            |_| {
                                let data = "x".repeat(300_000);
                                Ok(data)
                            },
                            None,
                        )
                        .await?;
                    Ok(large_data)
                })
            },
            Some(config),
        )
        .await?;

    Ok(format!("result_length={}", result.len()))
}

#[tokio::test(flavor = "current_thread")]
async fn test_child_context_large_data() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(child_context_large_data_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    // The handler returns the length of the result from the child context
    assert!(
        output.starts_with("result_length="),
        "Should report result length, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(
        !context_ops.is_empty(),
        "Should have context operations"
    );

    assert_nodejs_event_signatures(
        &result,
        "tests/history/child_context_large_data.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Child Context Checkpoint Size Limit Example Handler
// ============================================================================

/// Handler demonstrating checkpoint size limit mitigation with summary generator.
async fn child_context_checkpoint_size_limit_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    use aws_durable_execution_sdk::ChildConfig;
    use std::sync::Arc;

    let config = ChildConfig::new().set_summary_generator(Arc::new(|serialized: &str| {
        format!(
            "checkpoint_summary: original_size={}, truncated=true",
            serialized.len()
        )
    }));

    let result = ctx
        .run_in_child_context_named(
            "checkpoint_limit_child",
            |child_ctx| {
                Box::pin(async move {
                    let data: String = child_ctx
                        .step(
                            |_| {
                                let payload = "y".repeat(300_000);
                                Ok(payload)
                            },
                            None,
                        )
                        .await?;
                    Ok(data)
                })
            },
            Some(config),
        )
        .await?;

    Ok(format!("result_length={}", result.len()))
}

#[tokio::test(flavor = "current_thread")]
async fn test_child_context_checkpoint_size_limit() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(child_context_checkpoint_size_limit_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert!(
        output.starts_with("result_length="),
        "Should report result length, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let context_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Context)
        .collect();
    assert!(
        !context_ops.is_empty(),
        "Should have context operations"
    );

    assert_nodejs_event_signatures(
        &result,
        "tests/history/child_context_checkpoint_size_limit.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Child Context with Failing Step Example Handler
// ============================================================================

/// Handler demonstrating error propagation with error mapper in child context.
async fn child_context_with_failing_step_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    use aws_durable_execution_sdk::ChildConfig;
    use std::sync::Arc;

    let config = ChildConfig::new().set_error_mapper(Arc::new(|err: DurableError| {
        DurableError::execution(&format!("mapped_error: {}", err))
    }));

    let result = ctx
        .run_in_child_context_named(
            "failing_child",
            |child_ctx| {
                Box::pin(async move {
                    let _: String = child_ctx
                        .step(
                            |_| Err::<String, _>("step failed intentionally".into()),
                            None,
                        )
                        .await?;
                    Ok("should not reach here".to_string())
                })
            },
            Some(config),
        )
        .await;

    match result {
        Ok(val) => Ok(val),
        Err(e) => Ok(format!("caught error: {}", e)),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_child_context_with_failing_step() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(child_context_with_failing_step_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert!(
        output.contains("caught error"),
        "Should catch the mapped error, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/child_context_with_failing_step.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}
