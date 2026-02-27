//! Concurrent Operations Example
//!
//! Demonstrates multiple `ctx.step()` calls executed concurrently via `tokio::join!`.
//! Each step runs independently and the results are collected together.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrentResult {
    pub step_1: String,
    pub step_2: String,
    pub step_3: String,
}

/// Execute three steps concurrently using `tokio::join!`.
///
/// Each step is independent and can execute in any order. The `tokio::join!`
/// macro waits for all three to complete before continuing.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ConcurrentResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let (r1, r2, r3) = tokio::join!(
        ctx1.step(|_| Ok("step_1".to_string()), None),
        ctx2.step(|_| Ok("step_2".to_string()), None),
        ctx3.step(|_| Ok("step_3".to_string()), None),
    );

    Ok(ConcurrentResult {
        step_1: r1?,
        step_2: r2?,
        step_3: r3?,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
