//! Named Wait Example
//!
//! Using named wait operations for better observability.

use aws_durable_execution_sdk::{durable_execution, Duration, DurableError};

/// Execute multiple named wait operations.
///
/// Named waits are easier to identify in logs and debugging.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<String, DurableError> {
    // First wait with a descriptive name
    ctx.wait(Duration::from_seconds(1), Some("initial_delay")).await?;

    // Perform some work
    let _result: i32 = ctx
        .step_named("process", |_| Ok(42), None)
        .await?;

    // Second wait with a different name
    ctx.wait(Duration::from_seconds(2), Some("cooldown_period")).await?;

    Ok("All waits completed".to_string())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
