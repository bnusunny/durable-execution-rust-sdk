//! Step with Configuration Example
//!
//! Using `StepConfig` to customize step behavior including
//! execution semantics and serialization.

use aws_durable_execution_sdk::{durable_execution, DurableError, StepConfig, StepSemantics};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub amount: u64,
    pub status: String,
}

/// Execute steps with custom configuration.
///
/// This example demonstrates:
/// - AtMostOncePerRetry semantics for non-idempotent operations
/// - AtLeastOncePerRetry semantics for idempotent operations
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<PaymentResult, DurableError> {
    // Use AtMostOncePerRetry for payment processing
    // This ensures the payment is charged at most once per retry attempt
    let config = StepConfig {
        step_semantics: StepSemantics::AtMostOncePerRetry,
        ..Default::default()
    };

    let payment: PaymentResult = ctx
        .step_named(
            "charge_payment",
            |_step_ctx| {
                // Simulate payment processing
                Ok(PaymentResult {
                    transaction_id: "txn_abc123".to_string(),
                    amount: 9999,
                    status: "completed".to_string(),
                })
            },
            Some(config),
        )
        .await?;

    // Use default AtLeastOncePerRetry for idempotent operations
    let _logged: bool = ctx
        .step_named(
            "log_transaction",
            |_step_ctx| {
                // Logging is idempotent - safe to retry
                Ok(true)
            },
            None,
        )
        .await?;

    Ok(payment)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
