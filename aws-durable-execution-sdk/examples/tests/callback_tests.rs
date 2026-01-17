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
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/callback_simple.history.json");

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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/callback_concurrent.history.json");

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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/callback_with_timeout.history.json");

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

    // Check event signatures
    assert_event_signatures(operations, "tests/history/wait_for_callback.history.json");

    LocalDurableTestRunner::<PaymentRequest, PaymentResult>::teardown_test_environment()
        .await
        .unwrap();
}
