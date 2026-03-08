//! Parallel Tolerated Failure Percentage Example
//!
//! Demonstrates `CompletionConfig` with `tolerated_failure_percentage` set.
//! Uses `ctx.map()` with 10 items where 2 fail (20% failure rate).
//! Since the tolerance is 25%, the overall operation succeeds.

use durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToleratedFailurePctResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Handler demonstrating tolerated failure percentage in parallel operations.
///
/// Runs 10 items through `ctx.map()` where items 3 and 7 fail (20% failure rate).
/// Configures `tolerated_failure_percentage: Some(0.25)` so the overall operation
/// succeeds because 20% < 25% tolerance.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ToleratedFailurePctResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(10),
        completion_config: CompletionConfig {
            tolerated_failure_percentage: Some(0.25),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item == 3 || item == 7 {
                                    Err(format!("Item {} failed", item).into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    Ok(ToleratedFailurePctResult {
        total: results.total_count(),
        succeeded: results.succeeded().len(),
        failed: results.failed().len(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
