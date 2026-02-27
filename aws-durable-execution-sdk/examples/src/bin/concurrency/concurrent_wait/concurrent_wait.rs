//! Concurrent Wait Example
//!
//! Demonstrates multiple `ctx.wait()` calls executed concurrently via `tokio::join!`.
//! Each wait operation runs independently and completes after its specified duration.

use aws_durable_execution_sdk::{durable_execution, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrentWaitResult {
    pub status: String,
}

/// Execute three wait operations concurrently using `tokio::join!`.
///
/// Each wait has a different duration and name. They all execute concurrently,
/// so the total time is determined by the longest wait, not the sum.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ConcurrentWaitResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let (r1, r2, r3) = tokio::join!(
        ctx1.wait(Duration::from_seconds(1), Some("wait_1")),
        ctx2.wait(Duration::from_seconds(2), Some("wait_2")),
        ctx3.wait(Duration::from_seconds(3), Some("wait_3")),
    );

    r1?;
    r2?;
    r3?;

    Ok(ConcurrentWaitResult {
        status: "all_waits_completed".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
