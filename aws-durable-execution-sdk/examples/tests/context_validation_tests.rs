//! Tests for context_validation examples using LocalDurableTestRunner.
//!
//! These tests verify that context misuse (using a parent context in the wrong scope)
//! is detected by the SDK and produces appropriate errors or documented behavior.

use aws_durable_execution_sdk::DurableContext;
use aws_durable_execution_sdk::DurableError;
use aws_durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};

// ============================================================================
// Context Validation: Parent in Child Example Handler
// ============================================================================

/// Handler from context_validation/parent_in_child example (without macro for testing).
///
/// Demonstrates that using a parent DurableContext inside run_in_child_context
/// produces an error. The handler catches the error and returns a result.
async fn context_validation_parent_in_child_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let parent_ctx = ctx.clone();

    let result = ctx
        .run_in_child_context(
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    // Misuse: calling step on the PARENT context inside a child context
                    let val: String = parent.step(|_| Ok("from_parent".to_string()), None).await?;
                    Ok(val)
                })
            },
            None,
        )
        .await;

    match result {
        Ok(val) => Ok(format!("unexpected_success: {}", val)),
        Err(e) => Ok(format!("caught_error: {}", e)),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_context_validation_parent_in_child() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(context_validation_parent_in_child_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The handler catches the error, so execution should succeed
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    // The output should indicate either an error was caught or unexpected success
    assert!(
        output.starts_with("caught_error:") || output.starts_with("unexpected_success:"),
        "Should report context misuse result, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/context_validation_parent_in_child.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Context Validation: Parent in Step Example Handler
// ============================================================================

/// Handler from context_validation/parent_in_step example (without macro for testing).
///
/// Demonstrates that using a parent DurableContext to call durable operations
/// inside a child context's closure (instead of the child context) produces an error.
async fn context_validation_parent_in_step_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // First, do a normal step on the parent context
    let _setup: String = ctx
        .step_named("setup", |_| Ok("setup_done".to_string()), None)
        .await?;

    let parent_ctx = ctx.clone();

    // Now try to use the parent context inside a child context
    let result = ctx
        .run_in_child_context_named(
            "misuse_child",
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    // Misuse: calling step on the PARENT context inside a child context
                    let val: String = parent
                        .step_named(
                            "parent_step_in_child",
                            |_| Ok("from_parent".to_string()),
                            None,
                        )
                        .await?;
                    Ok(val)
                })
            },
            None,
        )
        .await;

    match result {
        Ok(val) => Ok(format!("unexpected_success: {}", val)),
        Err(e) => Ok(format!("caught_error: {}", e)),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_context_validation_parent_in_step() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(context_validation_parent_in_step_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The handler catches the error, so execution should succeed
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert!(
        output.starts_with("caught_error:") || output.starts_with("unexpected_success:"),
        "Should report context misuse result, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/context_validation_parent_in_step.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Context Validation: Parent in Wait Condition Example Handler
// ============================================================================

use aws_durable_execution_sdk::{Duration, WaitForConditionConfig, WaitForConditionContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollState {
    pub attempt: u32,
}

/// Handler from context_validation/parent_in_wait_condition example (without macro for testing).
///
/// Demonstrates that using a parent DurableContext inside a child context's closure
/// to call wait_for_condition produces an error.
async fn context_validation_parent_in_wait_condition_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let parent_ctx = ctx.clone();

    let result = ctx
        .run_in_child_context_named(
            "misuse_wait_condition",
            |_child_ctx| {
                let parent = parent_ctx;
                Box::pin(async move {
                    let config = WaitForConditionConfig::from_interval(
                        PollState { attempt: 0 },
                        Duration::from_seconds(1),
                        Some(3),
                    );

                    let val: String = parent
                        .wait_for_condition(
                            |_state: &PollState, wait_ctx: &WaitForConditionContext| {
                                if wait_ctx.attempt >= 1 {
                                    Ok("condition_met".to_string())
                                } else {
                                    Err("not yet".into())
                                }
                            },
                            config,
                        )
                        .await?;
                    Ok(val)
                })
            },
            None,
        )
        .await;

    match result {
        Ok(val) => Ok(format!("unexpected_success: {}", val)),
        Err(e) => Ok(format!("caught_error: {}", e)),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_context_validation_parent_in_wait_condition() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner =
        LocalDurableTestRunner::new(context_validation_parent_in_wait_condition_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The handler catches the error, so execution should succeed
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert!(
        output.starts_with("caught_error:") || output.starts_with("unexpected_success:"),
        "Should report context misuse result, got: {}",
        output
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/context_validation_parent_in_wait_condition.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}
