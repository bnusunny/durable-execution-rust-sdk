//! Concurrent Callbacks Example
//!
//! Creating multiple callbacks and waiting for them concurrently.

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_type: String,
    pub passed: bool,
    pub score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub credit_check: CheckResult,
    pub fraud_check: CheckResult,
    pub approved: bool,
}

/// Create multiple callbacks for concurrent external checks.
///
/// This pattern is useful when you need to wait for multiple
/// external systems to respond before proceeding.
#[durable_execution]
pub async fn handler(
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

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
