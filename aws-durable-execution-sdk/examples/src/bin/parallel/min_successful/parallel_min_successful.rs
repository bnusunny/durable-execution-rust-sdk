//! Parallel Min Successful Example
//!
//! Demonstrates `CompletionConfig::with_min_successful(n)` with `ctx.map()` where
//! the parallel operation completes after n successes, regardless of remaining tasks.
//! This is useful for scenarios like quorum-based consensus or redundant service calls
//! where you only need a subset of results to proceed.

use durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinSuccessfulResult {
    pub required: usize,
    pub completed: usize,
    pub results: Vec<String>,
}

/// Handler demonstrating parallel completion after n successes.
///
/// Runs 5 tasks through `ctx.map()` with `CompletionConfig::with_min_successful(2)`.
/// Each task performs a step returning a result string. The operation completes
/// as soon as 2 tasks succeed, without waiting for the remaining 3.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MinSuccessfulResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig::with_min_successful(2),
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![
                "task-a".to_string(),
                "task-b".to_string(),
                "task-c".to_string(),
                "task-d".to_string(),
                "task-e".to_string(),
            ],
            |child_ctx: DurableContext, item: String, _index: usize| {
                Box::pin(
                    async move { child_ctx.step(|_| Ok(format!("{} done", item)), None).await },
                )
            },
            Some(config),
        )
        .await?;

    let succeeded = results.succeeded();
    let successful_results: Vec<String> = succeeded
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(MinSuccessfulResult {
        required: 2,
        completed: successful_results.len(),
        results: successful_results,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
