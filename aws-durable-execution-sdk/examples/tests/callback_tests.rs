//! Tests for callback examples using LocalDurableTestRunner.
//!
//! These tests verify that the callback examples correctly initiate callback operations.
//! Since callback operations suspend execution (waiting for external signals), these tests verify that:
//! 1. The execution suspends with Running status
//! 2. The callback operation is correctly captured in the operations list
//! 3. The operation has the expected name and type
//!
//! Note: Full end-to-end callback completion testing requires external systems to send
//! callback responses. This is tested in integration tests against deployed Lambda functions.

use aws_durable_execution_sdk::{
    CallbackConfig, DurableContext, DurableError, Duration, OperationType,
};
use aws_durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Callback Simple Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CallbackResult {
    pub status: String,
    pub data: String,
}

/// Handler from callback/simple example (without macro for testing)
async fn callback_simple_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackResult, DurableError> {
    // Create a callback
    let callback = ctx.create_callback::<CallbackResult>(None).await?;

    // In a real scenario, you would send the callback_id to an external system
    println!(
        "Send this callback_id to external system: {}",
        callback.callback_id
    );

    // Wait for the callback result
    let result = callback.result().await?;

    Ok(result)
}

#[tokio::test]
async fn test_callback_simple() {
    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_simple_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Verify the callback operation was captured
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_simple.history.json");

    LocalDurableTestRunner::<serde_json::Value, CallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Concurrent Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckResult {
    pub check_type: String,
    pub passed: bool,
    pub score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationResult {
    pub credit_check: CheckResult,
    pub fraud_check: CheckResult,
    pub approved: bool,
}

/// Handler from callback/concurrent example (without macro for testing)
async fn callback_concurrent_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<VerificationResult, DurableError> {
    let config = CallbackConfig {
        timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    // Create callbacks for different external systems
    let credit_callback = ctx
        .create_callback_named::<CheckResult>("credit_check", Some(config.clone()))
        .await?;

    let fraud_callback = ctx
        .create_callback_named::<CheckResult>("fraud_check", Some(config))
        .await?;

    // Send callback IDs to respective systems
    println!("Credit check callback: {}", credit_callback.callback_id);
    println!("Fraud check callback: {}", fraud_callback.callback_id);

    // Wait for both results
    let credit_result = credit_callback.result().await?;
    let fraud_result = fraud_callback.result().await?;

    // Make decision based on both checks
    let approved = credit_result.passed && fraud_result.passed;

    Ok(VerificationResult {
        credit_check: credit_result,
        fraud_check: fraud_result,
        approved,
    })
}

#[tokio::test]
async fn test_callback_concurrent() {
    LocalDurableTestRunner::<serde_json::Value, VerificationResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_concurrent_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have callback operations started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify callback operations were captured
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    
    // Should have at least one callback (credit_check)
    // The second callback (fraud_check) may or may not be created depending on
    // whether the first callback suspends before the second is created
    assert!(!callback_ops.is_empty(), "Should have callback operations");

    // The first callback should have the name "credit_check"
    let first_callback = callback_ops.first().unwrap();
    assert_eq!(first_callback.name, Some("credit_check".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_concurrent.history.json");

    LocalDurableTestRunner::<serde_json::Value, VerificationResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback With Timeout Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
    pub comments: Option<String>,
}

/// Handler from callback/with_timeout example (without macro for testing)
async fn callback_with_timeout_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ApprovalResponse, DurableError> {
    // Configure callback with custom timeouts
    let config = CallbackConfig {
        timeout: Duration::from_hours(24),          // 24 hours to respond
        heartbeat_timeout: Duration::from_hours(1), // Heartbeat every hour
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<ApprovalResponse>("approval_callback", Some(config))
        .await?;

    // Send callback ID to approval system
    println!("Approval callback ID: {}", callback.callback_id);

    // Wait for approval (suspends until callback is received or timeout)
    let approval = callback.result().await?;

    Ok(approval)
}

#[tokio::test]
async fn test_callback_with_timeout() {
    LocalDurableTestRunner::<serde_json::Value, ApprovalResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_with_timeout_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify the callback operation was captured with the correct name
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");

    // The callback should have the name "approval_callback"
    let callback = callback_ops.first().unwrap();
    assert_eq!(callback.name, Some("approval_callback".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_with_timeout.history.json");

    LocalDurableTestRunner::<serde_json::Value, ApprovalResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait For Callback Example Handler
// ============================================================================

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentRequest {
    pub payment_id: String,
    pub amount: f64,
    pub currency: String,
    pub customer_email: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentConfirmation {
    pub transaction_id: String,
    pub status: String,
    pub confirmed_amount: f64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaymentResult {
    pub payment_id: String,
    pub status: String,
    pub transaction_id: Option<String>,
    pub message: String,
}

/// Handler from callback/wait_for_callback example (without macro for testing)
/// This demonstrates the wait_for_callback pattern which combines callback creation
/// with notification in a single, replay-safe operation.
async fn wait_for_callback_handler(
    event: PaymentRequest,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    // Validate the payment request
    let validated = ctx
        .step_named(
            "validate",
            |_| {
                if event.amount <= 0.0 {
                    return Err("Amount must be positive".into());
                }
                Ok(event.clone())
            },
            None,
        )
        .await?;

    // Clone values BEFORE the closure to avoid borrowing issues
    let payment_id_for_closure = validated.payment_id.clone();
    let amount = validated.amount;
    let currency_for_closure = validated.currency.clone();

    // Use wait_for_callback to:
    // 1. Create a callback
    // 2. Notify the payment processor (replay-safe!)
    // 3. Wait for the confirmation
    let confirmation: PaymentConfirmation = ctx
        .wait_for_callback(
            move |callback_id| {
                let payment_id = payment_id_for_closure;
                let currency = currency_for_closure;

                async move {
                    println!(
                        "Payment processor notified: payment_id={}, callback_id={}, amount={} {}",
                        payment_id, callback_id, amount, currency
                    );

                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                }
            },
            Some(CallbackConfig {
                timeout: Duration::from_hours(1),
                heartbeat_timeout: Duration::from_minutes(10),
                ..Default::default()
            }),
        )
        .await?;

    // Process the confirmation
    Ok(PaymentResult {
        payment_id: validated.payment_id,
        status: confirmation.status,
        transaction_id: Some(confirmation.transaction_id),
        message: format!(
            "Payment confirmed: {} {}",
            confirmation.confirmed_amount, validated.currency
        ),
    })
}

#[tokio::test]
async fn test_wait_for_callback() {
    LocalDurableTestRunner::<PaymentRequest, PaymentResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wait_for_callback_handler);
    let input = PaymentRequest {
        payment_id: "pay-12345".to_string(),
        amount: 100.0,
        currency: "USD".to_string(),
        customer_email: "test@example.com".to_string(),
    };
    let result = runner.run(input).await.unwrap();

    // wait_for_callback suspends execution when waiting for the callback result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the validation step and callback-related operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify we have at least one step operation (validate)
    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have step operations");

    // The first step should be the validation step
    let validate_step = step_ops.first().unwrap();
    assert_eq!(validate_step.name, Some("validate".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wait_for_callback.history.json");

    LocalDurableTestRunner::<PaymentRequest, PaymentResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait For Callback Timeout Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcCallbackResponse {
    pub status: String,
    pub message: String,
}

/// Handler from wait_for_callback/timeout example (without macro for testing)
/// Demonstrates wait_for_callback with a short timeout configured via CallbackConfig.
async fn wfc_timeout_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcCallbackResponse, DurableError> {
    // Configure a short timeout for the callback
    let config = CallbackConfig {
        timeout: Duration::from_seconds(5),
        ..Default::default()
    };

    // wait_for_callback: create callback, notify external system, and wait
    // Since no external system responds, this will time out after 5 seconds
    let response: WfcCallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Callback ID for external system: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        )
        .await?;

    Ok(response)
}

#[tokio::test]
async fn test_wfc_timeout() {
    LocalDurableTestRunner::<serde_json::Value, WfcCallbackResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_timeout_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback suspends execution waiting for a response (or timeout).
    // In the test runner, time doesn't advance, so the execution stays Running.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have callback-related operations from wait_for_callback
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_timeout.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcCallbackResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Failure Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcFailureResult {
    pub status: String,
    pub message: String,
}

/// Handler from wait_for_callback/wfc_failure example (without macro for testing).
///
/// The submitter closure returns an error. The handler catches the
/// `DurableError::UserCode` with `error_type == "SubmitterError"` and returns
/// a result indicating the failure was handled.
async fn wfc_failure_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcFailureResult, DurableError> {
    // wait_for_callback with a submitter that returns an error
    let result: Result<serde_json::Value, DurableError> = ctx
        .wait_for_callback(
            move |_callback_id| async move {
                Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                    "External system unavailable".into(),
                )
            },
            None,
        )
        .await;

    // Catch the submitter error instead of propagating with ?
    match result {
        Ok(_) => Ok(WfcFailureResult {
            status: "completed".to_string(),
            message: "Callback succeeded unexpectedly".to_string(),
        }),
        Err(DurableError::UserCode {
            message,
            error_type,
            ..
        }) if error_type == "SubmitterError" => Ok(WfcFailureResult {
            status: "submitter_failed".to_string(),
            message: format!("Submitter error caught: {}", message),
        }),
        Err(e) => Err(e),
    }
}

#[tokio::test]
async fn test_wfc_failure() {
    LocalDurableTestRunner::<serde_json::Value, WfcFailureResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_failure_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The submitter fails, so the handler catches the error and returns a result.
    // The execution should succeed because the handler handles the error gracefully.
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    // Verify the result indicates the submitter failure was caught
    let output = result.get_result().unwrap();
    assert_eq!(output.status, "submitter_failed");
    assert!(output.message.contains("Submitter error caught"));

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_failure.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcFailureResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Heartbeat Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcHeartbeatCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Handler from wait_for_callback/wfc_heartbeat example (without macro for testing).
///
/// Demonstrates wait_for_callback with heartbeat timeout configured via CallbackConfig.
/// The submitter prints the callback ID. The external system would send heartbeats
/// and eventually complete the callback.
async fn wfc_heartbeat_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcHeartbeatCallbackResponse, DurableError> {
    // Configure callback with heartbeat timeout
    let config = CallbackConfig {
        timeout: Duration::from_hours(1),
        heartbeat_timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    // wait_for_callback: create callback, notify external system, and wait
    let response: WfcHeartbeatCallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Callback ID for external system: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        )
        .await?;

    Ok(response)
}

#[tokio::test]
async fn test_wfc_heartbeat() {
    LocalDurableTestRunner::<serde_json::Value, WfcHeartbeatCallbackResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_heartbeat_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback suspends execution waiting for a response.
    // In the test runner, no external system responds, so execution stays Running.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have callback-related operations from wait_for_callback
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_heartbeat.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcHeartbeatCallbackResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Nested (Child Context) Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcNestedCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Handler from wait_for_callback/wfc_nested example (without macro for testing).
///
/// Demonstrates wait_for_callback called inside a child context created with
/// `run_in_child_context_named`. The callback is created and awaited within
/// the child context's closure.
async fn wfc_nested_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcNestedCallbackResponse, DurableError> {
    // Run wait_for_callback inside a child context
    let result = ctx
        .run_in_child_context_named(
            "nested_callback",
            |child_ctx| {
                Box::pin(async move {
                    let response: WfcNestedCallbackResponse = child_ctx
                        .wait_for_callback(
                            move |callback_id| async move {
                                println!("Callback ID (from child context): {}", callback_id);
                                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                            },
                            None,
                        )
                        .await?;
                    Ok(response)
                })
            },
            None,
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_wfc_nested() {
    LocalDurableTestRunner::<serde_json::Value, WfcNestedCallbackResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_nested_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback inside the child context suspends execution waiting for a response.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have child context and callback-related operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_nested.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcNestedCallbackResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Multiple Invocations Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcMultipleCallbackResponse {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcMultipleCallbackResult {
    pub first: WfcMultipleCallbackResponse,
    pub second: WfcMultipleCallbackResponse,
}

/// Handler from wait_for_callback/wfc_multiple example (without macro for testing).
///
/// Demonstrates multiple sequential wait_for_callback calls in a single handler.
/// The first callback suspends execution, so the second is never reached in tests.
async fn wfc_multiple_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcMultipleCallbackResult, DurableError> {
    // First wait_for_callback
    let first: WfcMultipleCallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("First callback ID: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            None,
        )
        .await?;

    // Second wait_for_callback — only reached after the first completes
    let second: WfcMultipleCallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Second callback ID: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            None,
        )
        .await?;

    Ok(WfcMultipleCallbackResult { first, second })
}

#[tokio::test]
async fn test_wfc_multiple() {
    LocalDurableTestRunner::<serde_json::Value, WfcMultipleCallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_multiple_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The first callback suspends execution waiting for a response.
    // The second callback is never reached.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have callback-related operations from the first wait_for_callback
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_multiple.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcMultipleCallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Child Context (Unnamed) Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WfcChildContextCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Handler from wait_for_callback/wfc_child_context example (without macro for testing).
///
/// Demonstrates wait_for_callback used within `run_in_child_context` (unnamed).
/// A preparatory step runs before the child context, then the callback is
/// created and awaited within the unnamed child context's closure.
async fn wfc_child_context_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcChildContextCallbackResponse, DurableError> {
    // Step before the child context
    let _prepared = ctx.step(|_| Ok("prepared".to_string()), None).await?;

    // Run wait_for_callback inside an unnamed child context
    let result = ctx
        .run_in_child_context(
            |child_ctx| {
                Box::pin(async move {
                    let response: WfcChildContextCallbackResponse = child_ctx
                        .wait_for_callback(
                            move |callback_id| async move {
                                println!("Callback ID (from child context): {}", callback_id);
                                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                            },
                            None,
                        )
                        .await?;
                    Ok(response)
                })
            },
            None,
        )
        .await?;

    Ok(result)
}

#[tokio::test]
async fn test_wfc_child_context() {
    LocalDurableTestRunner::<serde_json::Value, WfcChildContextCallbackResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_child_context_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback inside the child context suspends execution waiting for a response.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the step and child context with callback operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_child_context.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcChildContextCallbackResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait-for-Callback Custom SerDes Example Handler
// ============================================================================

/// Custom payload type for the callback response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WfcCustomSerdesPayload {
    pub data: String,
}

/// A custom `SerDesAny` implementation for `WfcCustomSerdesPayload`.
struct WfcCustomPayloadSerDes;

impl aws_durable_execution_sdk::SerDesAny for WfcCustomPayloadSerDes {
    fn serialize_any(
        &self,
        value: &dyn std::any::Any,
    ) -> Result<String, DurableError> {
        let payload = value
            .downcast_ref::<WfcCustomSerdesPayload>()
            .ok_or_else(|| {
                DurableError::execution("Expected WfcCustomSerdesPayload type")
            })?;
        serde_json::to_string(payload)
            .map_err(|e| DurableError::execution(format!("Serialization error: {}", e)))
    }

    fn deserialize_any(
        &self,
        data: &str,
    ) -> Result<Box<dyn std::any::Any + Send>, DurableError> {
        let payload: WfcCustomSerdesPayload = serde_json::from_str(data)
            .map_err(|e| DurableError::execution(format!("Deserialization error: {}", e)))?;
        Ok(Box::new(payload))
    }
}

/// Handler from wait_for_callback/wfc_custom_serdes example (without macro for testing).
///
/// Demonstrates wait_for_callback with a custom `SerDesAny` implementation
/// passed via `CallbackConfig { serdes: ... }`.
async fn wfc_custom_serdes_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WfcCustomSerdesPayload, DurableError> {
    let config = CallbackConfig {
        serdes: Some(std::sync::Arc::new(WfcCustomPayloadSerDes)),
        ..Default::default()
    };

    let response: WfcCustomSerdesPayload = ctx
        .wait_for_callback(
            move |callback_id| async move {
                println!("Callback ID (custom serdes): {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        )
        .await?;

    Ok(response)
}

#[tokio::test]
async fn test_wfc_custom_serdes() {
    LocalDurableTestRunner::<serde_json::Value, WfcCustomSerdesPayload>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wfc_custom_serdes_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The callback suspends execution waiting for a response.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/wfc_custom_serdes.history.json");

    LocalDurableTestRunner::<serde_json::Value, WfcCustomSerdesPayload>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Heartbeat Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatResponse {
    pub status: String,
    pub data: String,
}

/// Handler from callback/heartbeat example (without macro for testing).
///
/// Demonstrates `create_callback_named` with `CallbackConfig` that has
/// `heartbeat_timeout` set. The external system must send periodic heartbeats
/// to keep the callback alive.
async fn callback_heartbeat_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<HeartbeatResponse, DurableError> {
    let config = CallbackConfig {
        timeout: Duration::from_hours(2),
        heartbeat_timeout: Duration::from_minutes(10),
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<HeartbeatResponse>("heartbeat_callback", Some(config))
        .await?;

    println!("Heartbeat callback ID: {}", callback.callback_id);

    let result = callback.result().await?;

    Ok(result)
}

#[tokio::test]
async fn test_callback_heartbeat() {
    LocalDurableTestRunner::<serde_json::Value, HeartbeatResponse>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_heartbeat_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Verify the callback operation was captured with the correct name
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");

    // The callback should have the name "heartbeat_callback"
    let callback = callback_ops.first().unwrap();
    assert_eq!(callback.name, Some("heartbeat_callback".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_heartbeat.history.json");

    LocalDurableTestRunner::<serde_json::Value, HeartbeatResponse>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Failure Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureCallbackResult {
    pub status: String,
    pub message: String,
}

/// Handler from callback/failure example (without macro for testing).
///
/// Demonstrates `create_callback_named` and handling the case where the callback
/// result is an error. Since no external system responds, the callback suspends
/// execution. The error handling pattern is shown in the match arms.
async fn callback_failure_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<FailureCallbackResult, DurableError> {
    let callback = ctx
        .create_callback_named::<serde_json::Value>("failure_callback", None)
        .await?;

    println!("Failure callback ID: {}", callback.callback_id);

    // Wait for the callback result and handle potential failure
    match callback.result().await {
        Ok(value) => Ok(FailureCallbackResult {
            status: "success".to_string(),
            message: format!("Received: {:?}", value),
        }),
        Err(DurableError::Callback { message, .. }) => Ok(FailureCallbackResult {
            status: "callback_failed".to_string(),
            message: format!("Callback failed: {}", message),
        }),
        Err(e) => Err(e),
    }
}

#[tokio::test]
async fn test_callback_failure() {
    LocalDurableTestRunner::<serde_json::Value, FailureCallbackResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_failure_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result.
    // No external system sends a failure response, so execution stays Running.
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Verify the callback operation was captured with the correct name
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");

    // The callback should have the name "failure_callback"
    let callback = callback_ops.first().unwrap();
    assert_eq!(callback.name, Some("failure_callback".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_failure.history.json");

    LocalDurableTestRunner::<serde_json::Value, FailureCallbackResult>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Custom SerDes Example Handler
// ============================================================================

/// Custom payload type for the callback response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackCustomSerdesPayload {
    pub data: String,
}

/// A custom `SerDesAny` implementation for `CallbackCustomSerdesPayload`.
struct CallbackCustomPayloadSerDes;

impl aws_durable_execution_sdk::SerDesAny for CallbackCustomPayloadSerDes {
    fn serialize_any(
        &self,
        value: &dyn std::any::Any,
    ) -> Result<String, DurableError> {
        let payload = value
            .downcast_ref::<CallbackCustomSerdesPayload>()
            .ok_or_else(|| {
                DurableError::execution("Expected CallbackCustomSerdesPayload type")
            })?;
        serde_json::to_string(payload)
            .map_err(|e| DurableError::execution(format!("Serialization error: {}", e)))
    }

    fn deserialize_any(
        &self,
        data: &str,
    ) -> Result<Box<dyn std::any::Any + Send>, DurableError> {
        let payload: CallbackCustomSerdesPayload = serde_json::from_str(data)
            .map_err(|e| DurableError::execution(format!("Deserialization error: {}", e)))?;
        Ok(Box::new(payload))
    }
}

/// Handler from callback/custom_serdes example (without macro for testing).
///
/// Demonstrates `create_callback_named` with a custom `SerDesAny` implementation
/// passed via `CallbackConfig { serdes: ... }`.
async fn callback_custom_serdes_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackCustomSerdesPayload, DurableError> {
    let config = CallbackConfig {
        serdes: Some(std::sync::Arc::new(CallbackCustomPayloadSerDes)),
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<CallbackCustomSerdesPayload>("custom_serdes_callback", Some(config))
        .await?;

    println!("Callback ID (custom serdes): {}", callback.callback_id);

    let result = callback.result().await?;

    Ok(result)
}

#[tokio::test]
async fn test_callback_custom_serdes() {
    LocalDurableTestRunner::<serde_json::Value, CallbackCustomSerdesPayload>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_custom_serdes_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback operations suspend execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have the callback operation started
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Verify the callback operation was captured with the correct name
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");

    // The callback should have the name "custom_serdes_callback"
    let callback = callback_ops.first().unwrap();
    assert_eq!(callback.name, Some("custom_serdes_callback".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_custom_serdes.history.json");

    LocalDurableTestRunner::<serde_json::Value, CallbackCustomSerdesPayload>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Callback Mixed Ops Example Handler
// ============================================================================

/// Response type for the mixed ops callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedOpsCallbackResponse {
    pub approval: String,
}

/// Final output combining results from all operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedOpsResult {
    pub prepared_data: String,
    pub callback_approval: String,
}

/// Handler from callback/mixed_ops example (without macro for testing).
///
/// Demonstrates combining `create_callback_named` with steps and waits
/// in a single workflow.
async fn callback_mixed_ops_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MixedOpsResult, DurableError> {
    // Step 1: Prepare data
    let prepared_data = ctx
        .step_named(
            "prepare",
            |_| Ok("prepared_payload".to_string()),
            None,
        )
        .await?;

    // Step 2: Wait before creating callback
    ctx.wait(Duration::from_seconds(1), Some("pre_callback_delay"))
        .await?;

    // Step 3: Create callback and wait for result
    let callback = ctx
        .create_callback_named::<MixedOpsCallbackResponse>("mixed_callback", None)
        .await?;

    println!("Callback ID: {}", callback.callback_id);

    let result = callback.result().await?;

    Ok(MixedOpsResult {
        prepared_data,
        callback_approval: result.approval,
    })
}

#[tokio::test]
async fn test_callback_mixed_ops() {
    LocalDurableTestRunner::<serde_json::Value, MixedOpsResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(callback_mixed_ops_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Callback suspends execution when waiting for result
    assert_eq!(result.get_status(), ExecutionStatus::Running);

    // Verify operations - should have step, wait, and callback operations
    let operations = result.get_operations();
    assert!(
        operations.len() >= 3,
        "Should have at least 3 operations (step, wait, callback), got {}",
        operations.len()
    );

    // Verify the step operation
    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have a step operation");
    assert_eq!(step_ops[0].name, Some("prepare".to_string()));

    // Verify the wait operation
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert!(!wait_ops.is_empty(), "Should have a wait operation");

    // Verify the callback operation
    let callback_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Callback)
        .collect();
    assert!(!callback_ops.is_empty(), "Should have a callback operation");
    assert_eq!(callback_ops[0].name, Some("mixed_callback".to_string()));

    // Check event signatures (Node.js-compatible format)
    assert_nodejs_event_signatures(&result, "tests/history/callback_mixed_ops.history.json");

    LocalDurableTestRunner::<serde_json::Value, MixedOpsResult>::teardown_test_environment()
        .await
        .unwrap();
}
