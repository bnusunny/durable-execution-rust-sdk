//! Race with Timeout Example
//!
//! Demonstrates `race!` macro used as a timeout pattern where two step
//! operations race against each other. The first to complete wins.

use durable_execution_sdk::{durable_execution, race, DurableError};

/// Use `race!` as a timeout pattern.
///
/// Two branches race: one performs the actual operation, the other
/// acts as a timeout sentinel. Whichever step completes first wins.
/// This pattern is useful when you want to bound the time spent
/// waiting for an operation.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let result = race!(
        ctx,
        async move {
            ctx1.step(|_| Ok("operation completed".to_string()), None)
                .await
        },
        async move { ctx2.step(|_| Ok("timed out".to_string()), None).await },
    )
    .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
