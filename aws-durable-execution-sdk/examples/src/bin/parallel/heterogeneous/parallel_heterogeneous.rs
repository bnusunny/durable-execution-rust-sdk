//! Heterogeneous Parallel Example
//!
//! Running different types of operations using child contexts.

use aws_durable_execution_sdk::{durable_execution, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceResults {
    pub inventory: String,
    pub payment: String,
    pub shipping: String,
}

/// Execute heterogeneous operations using child contexts.
///
/// Each operation runs in its own child context and can perform
/// different types of operations (steps, waits, etc.).
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ServiceResults, DurableError> {
    // Run each service check in its own child context
    let inventory = ctx
        .run_in_child_context_named(
            "check_inventory",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("inventory_available".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    let payment = ctx
        .run_in_child_context_named(
            "process_payment",
            |child_ctx| {
                Box::pin(async move {
                    // Payment includes a delay
                    child_ctx
                        .wait(Duration::from_seconds(1), Some("payment_delay"))
                        .await?;
                    child_ctx
                        .step(|_| Ok("payment_completed".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    let shipping = ctx
        .run_in_child_context_named(
            "calculate_shipping",
            |child_ctx| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok("shipping_calculated".to_string()), None)
                        .await
                })
            },
            None,
        )
        .await?;

    Ok(ServiceResults {
        inventory,
        payment,
        shipping,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
