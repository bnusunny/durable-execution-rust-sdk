//! Tests for step examples using LocalDurableTestRunner.
//!
//! These tests verify that the step examples execute correctly and produce
//! the expected operation history.

use durable_execution_sdk::{DurableContext, DurableError, StepConfig, StepSemantics};
use durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures;
use durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Step Basic Example Handler
// ============================================================================

/// Handler from step/basic example (without macro for testing)
async fn step_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let result = ctx
        .step(|_step_ctx| Ok("step completed".to_string()), None)
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_step_basic() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "step completed");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/step_basic.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Step Named Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessingResult {
    pub step_name: String,
    pub value: i32,
}

/// Handler from step/named example (without macro for testing)
async fn step_named_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<ProcessingResult>, DurableError> {
    let first: ProcessingResult = ctx
        .step_named(
            "fetch_data",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "fetch_data".to_string(),
                    value: 42,
                })
            },
            None,
        )
        .await?;

    let second: ProcessingResult = ctx
        .step_named(
            "process_data",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "process_data".to_string(),
                    value: 100,
                })
            },
            None,
        )
        .await?;

    let third: ProcessingResult = ctx
        .step_named(
            "finalize",
            |_step_ctx| {
                Ok(ProcessingResult {
                    step_name: "finalize".to_string(),
                    value: 200,
                })
            },
            None,
        )
        .await?;

    Ok(vec![first, second, third])
}

#[tokio::test]
async fn test_step_named() {
    LocalDurableTestRunner::<serde_json::Value, Vec<ProcessingResult>>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_named_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let results: &Vec<ProcessingResult> = result.get_result().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].step_name, "fetch_data");
    assert_eq!(results[0].value, 42);
    assert_eq!(results[1].step_name, "process_data");
    assert_eq!(results[1].value, 100);
    assert_eq!(results[2].step_name, "finalize");
    assert_eq!(results[2].value, 200);

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/step_named.history.json");

    LocalDurableTestRunner::<serde_json::Value, Vec<ProcessingResult>>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Step With Config Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub amount: u64,
    pub status: String,
}

/// Handler from step/with_config example (without macro for testing)
async fn step_with_config_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    let config = StepConfig {
        step_semantics: StepSemantics::AtMostOncePerRetry,
        ..Default::default()
    };

    let payment: PaymentResult = ctx
        .step_named(
            "charge_payment",
            |_step_ctx| {
                Ok(PaymentResult {
                    transaction_id: "txn_abc123".to_string(),
                    amount: 9999,
                    status: "completed".to_string(),
                })
            },
            Some(config),
        )
        .await?;

    let _logged: bool = ctx
        .step_named("log_transaction", |_step_ctx| Ok(true), None)
        .await?;

    Ok(payment)
}

#[tokio::test]
async fn test_step_with_config() {
    LocalDurableTestRunner::<serde_json::Value, PaymentResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_with_config_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    let payment = result.get_result().unwrap();
    assert_eq!(payment.transaction_id, "txn_abc123");
    assert_eq!(payment.amount, 9999);
    assert_eq!(payment.status, "completed");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/step_with_config.history.json");

    LocalDurableTestRunner::<serde_json::Value, PaymentResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Step Retry Exponential Backoff Example Handler
// ============================================================================

use durable_execution_sdk::{Duration, ExponentialBackoff, JitterStrategy};

/// Handler from step/retry_exponential_backoff example (without macro for testing)
async fn step_retry_exponential_backoff_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retry_strategy: Some(Box::new(
            ExponentialBackoff::builder()
                .max_attempts(3)
                .base_delay(Duration::from_seconds(1))
                .max_delay(Duration::from_seconds(10))
                .multiplier(2.0)
                .jitter(JitterStrategy::Full)
                .build(),
        )),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "retryable_operation",
            |_step_ctx| Ok("operation_succeeded".to_string()),
            Some(config),
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_step_retry_exponential_backoff() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_retry_exponential_backoff_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "operation_succeeded");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    assert_nodejs_event_signatures(
        &result,
        "tests/history/step_retry_exponential_backoff.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Step Retry With Filter Example Handler
// ============================================================================

use durable_execution_sdk::{ErrorPattern, RetryableErrorFilter};

/// Handler from step/retry_with_filter example (without macro for testing)
async fn step_retry_with_filter_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retryable_error_filter: Some(RetryableErrorFilter {
            patterns: vec![ErrorPattern::Contains("transient".to_string())],
            error_types: vec![],
        }),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "filtered_operation",
            |_step_ctx| Ok("filter_configured".to_string()),
            Some(config),
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_step_retry_with_filter() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_retry_with_filter_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "filter_configured");

    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    assert_nodejs_event_signatures(&result, "tests/history/step_retry_with_filter.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Step Error Determinism Example Handler
// ============================================================================

/// Handler from step/error_determinism example (without macro for testing)
async fn step_error_determinism_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let result: String = ctx
        .step_named(
            "always_fails",
            |_step_ctx| Err::<String, _>("deterministic_failure".into()),
            None,
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_step_error_determinism() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(step_error_determinism_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The step always fails, so the execution should fail
    assert_eq!(result.get_status(), ExecutionStatus::Failed);

    assert_nodejs_event_signatures(&result, "tests/history/step_error_determinism.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}
