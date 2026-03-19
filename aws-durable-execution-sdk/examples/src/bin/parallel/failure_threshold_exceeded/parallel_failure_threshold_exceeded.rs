//! Parallel Failure Threshold Exceeded Example
//!
//! Demonstrates the behavior when failures exceed the configured tolerance,
//! causing the parallel operation to fail. Uses `ctx.map()` with 5 items
//! where 3 fail, but only 1 failure is tolerated.

use durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailureThresholdResult {
    pub threshold_exceeded: bool,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_message: Option<String>,
}

/// Handler demonstrating failure threshold exceeded in parallel operations.
///
/// Runs 5 items through `ctx.map()` where items 1, 3, and 5 fail (3 failures).
/// Configures `tolerated_failure_count: Some(1)` so the operation completes
/// early when the second failure is detected (exceeding the 1-failure tolerance).
/// The handler catches the threshold exceeded condition and returns a summary.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<FailureThresholdResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(1),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| async move {
                                if item % 2 == 1 {
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

    let threshold_exceeded = results.is_failure();
    let error_message = if threshold_exceeded {
        results.get_results().err().map(|e| e.to_string())
    } else {
        None
    };

    Ok(FailureThresholdResult {
        threshold_exceeded,
        total: results.total_count(),
        succeeded: results.succeeded().len(),
        failed: results.failed().len(),
        error_message,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
