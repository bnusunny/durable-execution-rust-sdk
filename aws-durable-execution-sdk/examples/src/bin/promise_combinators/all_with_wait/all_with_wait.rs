//! All with Wait Example
//!
//! Demonstrates `all!` macro combined with `ctx.wait()` calls,
//! showing how wait operations compose with promise combinators.
//! Waits are performed before the combinator since `all!` branches
//! cannot contain suspend-producing operations like `ctx.wait()`.

use durable_execution_sdk::{all, durable_execution, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllWithWaitResult {
    pub results: Vec<String>,
}

/// Use `ctx.wait()` followed by `all!` macro with 3 step futures.
///
/// Wait operations are performed before the combinator to demonstrate
/// how waits and combinators compose in a workflow. The `all!` macro
/// then collects results from multiple concurrent step operations.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<AllWithWaitResult, DurableError> {
    // Perform waits before the all! combinator
    ctx.wait(Duration::from_seconds(1), Some("wait_1")).await?;
    ctx.wait(Duration::from_seconds(2), Some("wait_2")).await?;

    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let results = all!(
        ctx,
        async move { ctx1.step(|_| Ok("fast".to_string()), None).await },
        async move { ctx2.step(|_| Ok("after_wait_1".to_string()), None).await },
        async move { ctx3.step(|_| Ok("after_wait_2".to_string()), None).await },
    )
    .await?;

    Ok(AllWithWaitResult { results })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
