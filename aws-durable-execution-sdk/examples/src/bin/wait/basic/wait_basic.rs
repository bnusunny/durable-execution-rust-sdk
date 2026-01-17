//! Basic Wait Example
//!
//! Basic usage of `ctx.wait()` to pause execution for a duration.
//! Wait operations suspend the Lambda and resume later.

use aws_durable_execution_sdk::{durable_execution, DurableError, Duration};

/// Pause execution for 2 seconds.
///
/// The wait operation suspends the Lambda execution. The function
/// will be re-invoked after the wait duration, and execution will
/// resume from after the wait.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Wait for 2 seconds
    ctx.wait(Duration::from_seconds(2), None).await?;

    Ok("Function Completed".to_string())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
