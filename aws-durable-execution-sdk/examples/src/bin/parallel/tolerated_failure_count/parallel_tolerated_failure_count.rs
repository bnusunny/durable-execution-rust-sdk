//! Parallel Tolerated Failure Count Example
//!
//! Demonstrates `CompletionConfig` with `tolerated_failure_count` set, where
//! failures up to the threshold do not cause overall failure. Uses `ctx.map()`
//! with 6 items where 2 will fail (items 3 and 6). Since the tolerance is 2,
//! the overall operation succeeds.

use aws_durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToleratedFailureResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Handler demonstrating tolerated failure count in parallel operations.
///
/// Runs 6 items through `ctx.map()` where items 3 and 6 fail. Configures
/// `tolerated_failure_count: Some(2)` so the overall operation succeeds
/// because exactly 2 items fail, which is within the tolerance threshold.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ToleratedFailureResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(6),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(2),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| {
                                if item == 3 || item == 6 {
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

    Ok(ToleratedFailureResult {
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
