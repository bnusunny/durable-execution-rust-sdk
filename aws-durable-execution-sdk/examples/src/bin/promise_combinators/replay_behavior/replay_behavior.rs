//! Replay Behavior Example
//!
//! Demonstrates that combinator results are deterministic across replays
//! by using multiple combinators in sequence.

use durable_execution_sdk::{all, durable_execution, race, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayBehaviorResult {
    pub all_results: Vec<String>,
    pub race_result: String,
    pub combined: String,
}

/// Use multiple combinators in sequence to demonstrate replay determinism.
///
/// First, `all!` collects results from 3 steps. Then `race!` picks the
/// first of 2 steps. The final result combines both, and replaying the
/// handler against the recorded history produces the same output.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ReplayBehaviorResult, DurableError> {
    // First: all! to get multiple results
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let all_results = all!(
        ctx,
        async move { ctx1.step(|_| Ok("result_a".to_string()), None).await },
        async move { ctx2.step(|_| Ok("result_b".to_string()), None).await },
        async move { ctx3.step(|_| Ok("result_c".to_string()), None).await },
    )
    .await?;

    // Second: race! to get the first result
    let ctx4 = ctx.clone();
    let ctx5 = ctx.clone();

    let race_result = race!(
        ctx,
        async move { ctx4.step(|_| Ok("fast_path".to_string()), None).await },
        async move { ctx5.step(|_| Ok("slow_path".to_string()), None).await },
    )
    .await?;

    // Combine and return
    let combined = format!("all=[{}], race={}", all_results.join(","), race_result);

    Ok(ReplayBehaviorResult {
        all_results,
        race_result,
        combined,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
