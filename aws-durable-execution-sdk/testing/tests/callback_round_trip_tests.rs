//! Integration tests for the concurrent callback testing pattern.
//!
//! These tests verify the idiomatic callback interaction pattern:
//! pre-register an operation handle, start a non-blocking run, wait for
//! callback readiness, send a callback response, and await the final result.
//!
//! # Requirements Validated
//!
//! - 2.3: Concurrent `wait_for_data()` on pre-registered OperationHandles during `run()`
//! - 2.5: Callback response delivered to checkpoint server and handler resumes
//! - 5.1: Full callback round-trip pattern completes with callback result incorporated

use durable_execution_sdk::{CallbackConfig, DurableContext, DurableError, Duration};
use durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig, TestResultError,
    WaitingOperationStatus,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Single Callback Round-Trip Test
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CallbackResult {
    pub message: String,
}

/// A handler that creates a single named callback operation and waits for its result.
async fn single_callback_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackResult, DurableError> {
    let config = CallbackConfig {
        timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<CallbackResult>("callback-op", Some(config))
        .await?;

    // Wait for the external callback response
    let result = callback.result().await?;

    Ok(result)
}

/// Test: single callback round-trip
///
/// Pre-register a handle for "callback-op", start `run()`, wait for the callback
/// to reach Submitted status, send a success response, and verify the final
/// TestResult contains the callback result.
///
/// **Validates: Requirements 2.3, 2.5, 5.1**
#[tokio::test]
async fn test_single_callback_round_trip() {
    // Setup test environment with time skipping
    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(single_callback_handler);

    // Pre-register handle before run()
    let handle = runner.get_operation_handle("callback-op");

    // Start non-blocking run — returns an inline future that must be driven
    // concurrently with handle interactions.
    let run_future = runner.run(serde_json::json!({}));

    // Drive the run future and the callback interaction concurrently.
    // The callback_interaction task waits for Submitted, sends the response,
    // and the run_future completes once the handler receives the callback result.
    let (result, _) = tokio::join!(run_future, async {
        // Wait for the callback operation to reach Submitted status
        handle
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await
            .unwrap();

        // Send callback success with a result
        let callback_result = serde_json::json!({"message": "hello from callback"});
        handle
            .send_callback_success(&callback_result.to_string())
            .await
            .unwrap();
    });

    let result = result.unwrap();

    // Assert execution succeeded
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed after callback response"
    );

    // Assert the result contains the callback data
    let output = result.get_result().unwrap();
    assert_eq!(
        output,
        &CallbackResult {
            message: "hello from callback".to_string(),
        }
    );

    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Failure Propagation Test
// ============================================================================

/// Test: callback failure propagation
///
/// Pre-register a handle for "callback-op", start `run()`, wait for the callback
/// to reach Submitted status, send a failure response, and verify the handler
/// receives the failure — the final TestResult should have Failed status with
/// the error details propagated.
///
/// **Validates: Requirements 2.4, 5.2**
#[tokio::test]
async fn test_callback_failure_propagation() {
    // Setup test environment with time skipping
    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(single_callback_handler);

    // Pre-register handle before run()
    let handle = runner.get_operation_handle("callback-op");

    // Start non-blocking run
    let run_future = runner.run(serde_json::json!({}));

    // Drive the run future and the callback failure interaction concurrently.
    let (result, _) = tokio::join!(run_future, async {
        // Wait for the callback operation to reach Submitted status
        handle
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await
            .unwrap();

        // Send callback failure with error details
        let error = TestResultError::new("ValidationError", "Input validation failed");
        handle.send_callback_failure(&error).await.unwrap();
    });

    let result = result.unwrap();

    // Assert execution failed due to callback failure
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Failed,
        "Execution should fail when callback failure is sent"
    );

    // Assert the error details are propagated
    let error = result
        .get_error()
        .expect("Failed result should have error details");
    assert!(
        error
            .error_message
            .as_ref()
            .is_some_and(|msg| msg.contains("Input validation failed")),
        "Error message should contain the callback failure message, got: {:?}",
        error.error_message
    );

    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Multiple Concurrent Callbacks Test
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MultiCallbackResult {
    pub result_a: String,
    pub result_b: String,
}

/// A handler that creates two named callback operations concurrently and waits
/// for both results before returning a combined result.
async fn multi_callback_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MultiCallbackResult, DurableError> {
    let config = CallbackConfig {
        timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    // Create two independent callback operations
    let callback_a = ctx
        .create_callback_named::<CallbackResult>("callback-a", Some(config.clone()))
        .await?;

    let callback_b = ctx
        .create_callback_named::<CallbackResult>("callback-b", Some(config))
        .await?;

    // Wait for both results
    let result_a = callback_a.result().await?;
    let result_b = callback_b.result().await?;

    Ok(MultiCallbackResult {
        result_a: result_a.message,
        result_b: result_b.message,
    })
}

/// Test: multiple concurrent callbacks
///
/// Pre-register handles for "callback-a" and "callback-b", start `run()`,
/// wait for each callback to reach Submitted status, send independent
/// responses to each, and verify the final TestResult contains both
/// callback results with no cross-contamination.
///
/// **Validates: Requirements 5.3**
#[tokio::test]
async fn test_multiple_concurrent_callbacks() {
    // Setup test environment with time skipping
    LocalDurableTestRunner::<serde_json::Value, MultiCallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(multi_callback_handler);

    // Pre-register handles for both callback operations before run()
    let handle_a = runner.get_operation_handle("callback-a");
    let handle_b = runner.get_operation_handle("callback-b");

    // Start non-blocking run
    let run_future = runner.run(serde_json::json!({}));

    // Drive the run future and both callback interactions concurrently.
    let (result, _) = tokio::join!(run_future, async {
        // Wait for callback-a to reach Submitted status and send its response
        handle_a
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await
            .unwrap();
        let response_a = serde_json::json!({"message": "response-for-a"});
        handle_a
            .send_callback_success(&response_a.to_string())
            .await
            .unwrap();

        // Wait for callback-b to reach Submitted status and send its response
        handle_b
            .wait_for_data(WaitingOperationStatus::Submitted)
            .await
            .unwrap();
        let response_b = serde_json::json!({"message": "response-for-b"});
        handle_b
            .send_callback_success(&response_b.to_string())
            .await
            .unwrap();
    });

    let result = result.unwrap();

    // Assert execution succeeded
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should succeed after both callbacks respond"
    );

    // Assert each callback received its own response — no cross-contamination
    let output = result.get_result().unwrap();
    assert_eq!(
        output.result_a, "response-for-a",
        "callback-a should receive response-for-a, not callback-b's response"
    );
    assert_eq!(
        output.result_b, "response-for-b",
        "callback-b should receive response-for-b, not callback-a's response"
    );

    LocalDurableTestRunner::<serde_json::Value, MultiCallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}
