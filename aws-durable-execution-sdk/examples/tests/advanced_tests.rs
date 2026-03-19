//! Tests for advanced examples using LocalDurableTestRunner.
//!
//! These tests verify advanced SDK features including checkpointing modes,
//! retry exhaustion, large payloads, and handler-level errors.

use durable_execution_sdk::{
    DurableContext, DurableError, Duration, ExponentialBackoff, StepConfig,
};
use durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures;
use durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Checkpointing Eager Example Handler
// ============================================================================

/// Handler from advanced/checkpointing_eager example (without macro for testing)
async fn checkpointing_eager_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "critical_step_1",
            |_step_ctx| async move { Ok("checkpoint_after_this".to_string()) },
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "critical_step_2",
            |_step_ctx| async move { Ok("also_checkpointed".to_string()) },
            None,
        )
        .await?;

    Ok(format!("eager: {} + {}", step1, step2))
}

#[tokio::test(flavor = "current_thread")]
async fn test_checkpointing_eager() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(checkpointing_eager_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(
        result.get_result().unwrap(),
        "eager: checkpoint_after_this + also_checkpointed"
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures(&result, "tests/history/checkpointing_eager.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Checkpointing Batched Example Handler
// ============================================================================

/// Handler from advanced/checkpointing_batched example (without macro for testing)
async fn checkpointing_batched_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "batched_step_1",
            |_step_ctx| async move { Ok("batched_1".to_string()) },
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "batched_step_2",
            |_step_ctx| async move { Ok("batched_2".to_string()) },
            None,
        )
        .await?;

    let step3: String = ctx
        .step_named(
            "batched_step_3",
            |_step_ctx| async move { Ok("batched_3".to_string()) },
            None,
        )
        .await?;

    Ok(format!("batched: {} + {} + {}", step1, step2, step3))
}

#[tokio::test(flavor = "current_thread")]
async fn test_checkpointing_batched() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(checkpointing_batched_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(
        result.get_result().unwrap(),
        "batched: batched_1 + batched_2 + batched_3"
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures(&result, "tests/history/checkpointing_batched.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Checkpointing Optimistic Example Handler
// ============================================================================

/// Handler from advanced/checkpointing_optimistic example (without macro for testing)
async fn checkpointing_optimistic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let step1: String = ctx
        .step_named(
            "optimistic_step_1",
            |_step_ctx| async move { Ok("fast_1".to_string()) },
            None,
        )
        .await?;

    let step2: String = ctx
        .step_named(
            "optimistic_step_2",
            |_step_ctx| async move { Ok("fast_2".to_string()) },
            None,
        )
        .await?;

    let step3: String = ctx
        .step_named(
            "optimistic_step_3",
            |_step_ctx| async move { Ok("fast_3".to_string()) },
            None,
        )
        .await?;

    Ok(format!("optimistic: {} + {} + {}", step1, step2, step3))
}

#[tokio::test(flavor = "current_thread")]
async fn test_checkpointing_optimistic() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(checkpointing_optimistic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(
        result.get_result().unwrap(),
        "optimistic: fast_1 + fast_2 + fast_3"
    );

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/checkpointing_optimistic.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Retry Exhaustion Example Handler
// ============================================================================

/// Handler from advanced/retry_exhaustion example (without macro for testing)
async fn retry_exhaustion_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retry_strategy: Some(Box::new(ExponentialBackoff::new(
            2,
            Duration::from_seconds(1),
        ))),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "always_failing_step",
            |_step_ctx| async move { Err::<String, _>("step_always_fails".into()) },
            Some(config),
        )
        .await?;

    Ok(result)
}

#[tokio::test(flavor = "current_thread")]
async fn test_retry_exhaustion() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(retry_exhaustion_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The step exhausts all retries, so the execution should fail
    assert_eq!(result.get_status(), ExecutionStatus::Failed);

    assert_nodejs_event_signatures(&result, "tests/history/retry_exhaustion.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Large Payload Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeInput {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LargeOutput {
    pub processed: String,
    pub size: usize,
}

/// Handler from advanced/large_payload example (without macro for testing)
async fn large_payload_handler(
    event: LargeInput,
    ctx: DurableContext,
) -> Result<LargeOutput, DurableError> {
    let processed: String = ctx
        .step_named(
            "process_large_data",
            |_step_ctx| async move { Ok(format!("processed_{}_bytes", event.data.len())) },
            None,
        )
        .await?;

    let result = LargeOutput {
        size: processed.len(),
        processed,
    };

    if ctx.complete_execution_if_large(&result).await? {
        return Ok(LargeOutput::default());
    }

    Ok(result)
}

#[tokio::test(flavor = "current_thread")]
async fn test_large_payload() {
    LocalDurableTestRunner::<LargeInput, LargeOutput>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let input = LargeInput {
        data: "a]".repeat(1000),
    };

    let mut runner = LocalDurableTestRunner::new(large_payload_handler);
    let result = runner.run(input).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert_eq!(output.processed, "processed_2000_bytes");
    assert_eq!(output.size, 20); // length of "processed_2000_bytes"

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures(&result, "tests/history/large_payload.history.json");

    LocalDurableTestRunner::<LargeInput, LargeOutput>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Handler Error Example Handler
// ============================================================================

/// Handler from advanced/handler_error example (without macro for testing)
async fn handler_error_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let _setup: String = ctx
        .step_named(
            "setup_step",
            |_step_ctx| async move { Ok("setup_done".to_string()) },
            None,
        )
        .await?;

    Err(DurableError::execution("handler_level_failure"))
}

#[tokio::test(flavor = "current_thread")]
async fn test_handler_error() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(handler_error_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The handler returns an error, so execution should fail
    assert_eq!(result.get_status(), ExecutionStatus::Failed);

    assert_nodejs_event_signatures(&result, "tests/history/handler_error.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}
