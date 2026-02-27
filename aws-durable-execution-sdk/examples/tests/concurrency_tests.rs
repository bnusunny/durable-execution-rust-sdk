//! Tests for concurrency examples using LocalDurableTestRunner.
//!
//! These tests verify that concurrent durable operations using `tokio::join!`
//! execute correctly and produce deterministic replay behavior.

use aws_durable_execution_sdk::{CallbackConfig, DurableContext, DurableError, Duration, OperationType};
use aws_durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures_unordered;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Concurrent Operations Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConcurrentResult {
    pub step_1: String,
    pub step_2: String,
    pub step_3: String,
}

/// Handler from concurrency/concurrent_operations example (without macro for testing).
async fn concurrent_operations_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ConcurrentResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let (r1, r2, r3) = tokio::join!(
        ctx1.step(|_| Ok("step_1".to_string()), None),
        ctx2.step(|_| Ok("step_2".to_string()), None),
        ctx3.step(|_| Ok("step_3".to_string()), None),
    );

    Ok(ConcurrentResult {
        step_1: r1?,
        step_2: r2?,
        step_3: r3?,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_operations() {
    LocalDurableTestRunner::<serde_json::Value, ConcurrentResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(concurrent_operations_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert_eq!(output.step_1, "step_1");
    assert_eq!(output.step_2, "step_2");
    assert_eq!(output.step_3, "step_3");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert_eq!(step_ops.len(), 3, "Should have 3 step operations");

    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/concurrent_operations.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, ConcurrentResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Concurrent Wait Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConcurrentWaitResult {
    pub status: String,
}

/// Handler from concurrency/concurrent_wait example (without macro for testing).
async fn concurrent_wait_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ConcurrentWaitResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let (r1, r2, r3) = tokio::join!(
        ctx1.wait(Duration::from_seconds(1), Some("wait_1")),
        ctx2.wait(Duration::from_seconds(2), Some("wait_2")),
        ctx3.wait(Duration::from_seconds(3), Some("wait_3")),
    );

    r1?;
    r2?;
    r3?;

    Ok(ConcurrentWaitResult {
        status: "all_waits_completed".to_string(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_wait() {
    LocalDurableTestRunner::<serde_json::Value, ConcurrentWaitResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(concurrent_wait_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let output = result.get_result().unwrap();
    assert_eq!(output.status, "all_waits_completed");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert_eq!(wait_ops.len(), 3, "Should have 3 wait operations");

    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/concurrent_wait.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, ConcurrentWaitResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Concurrent Callback Wait Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallbackWaitResult {
    pub step_result: String,
    pub callback_status: String,
}

/// Handler from concurrency/concurrent_callback_wait example (without macro for testing).
async fn concurrent_callback_wait_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackWaitResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let config = CallbackConfig {
        timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    let (step_result, callback_result) = tokio::join!(
        ctx1.step(|_| Ok("step_done".to_string()), None),
        async {
            let cb = ctx2
                .create_callback_named::<String>("concurrent_cb", Some(config))
                .await?;
            println!("Callback ID: {}", cb.callback_id);
            cb.result().await
        },
    );

    Ok(CallbackWaitResult {
        step_result: step_result?,
        callback_status: callback_result?,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_callback_wait() {
    LocalDurableTestRunner::<serde_json::Value, CallbackWaitResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(concurrent_callback_wait_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback suspends execution waiting for an external response.
    // The step may complete but the callback keeps the execution Running.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/concurrent_callback_wait.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, CallbackWaitResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Concurrent Callback Submitter Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultiCallbackResult {
    pub callback_a: String,
    pub callback_b: String,
}

/// Handler from concurrency/concurrent_callback_submitter example (without macro for testing).
async fn concurrent_callback_submitter_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MultiCallbackResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let config = CallbackConfig {
        timeout: Duration::from_minutes(10),
        ..Default::default()
    };

    let (result_a, result_b) = tokio::join!(
        ctx1.wait_for_callback(
            move |callback_id| async move {
                println!("Submitter A: callback_id={}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config.clone()),
        ),
        ctx2.wait_for_callback(
            move |callback_id| async move {
                println!("Submitter B: callback_id={}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        ),
    );

    Ok(MultiCallbackResult {
        callback_a: result_a?,
        callback_b: result_b?,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_concurrent_callback_submitter() {
    LocalDurableTestRunner::<serde_json::Value, MultiCallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(concurrent_callback_submitter_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Both callbacks suspend execution waiting for external responses.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    assert_nodejs_event_signatures_unordered(
        &result,
        "tests/history/concurrent_callback_submitter.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, MultiCallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}
