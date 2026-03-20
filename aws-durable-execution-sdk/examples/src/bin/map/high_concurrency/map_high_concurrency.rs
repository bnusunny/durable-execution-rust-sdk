//! Map High Concurrency Example
//!
//! Demonstrates `MapConfig` with `max_concurrency: Some(10)` processing 15 items
//! concurrently. Each item performs a step that returns a processed string.

use durable_execution_sdk::{durable_execution, DurableContext, DurableError, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HighConcurrencyResult {
    pub total: usize,
    pub results: Vec<String>,
}

/// Handler demonstrating `ctx.map()` with high concurrency.
///
/// Maps 15 items with `max_concurrency: Some(10)`, meaning up to 10 items
/// are processed concurrently. Each item performs a step returning a
/// `"processed_{item}"` string.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<HighConcurrencyResult, DurableError> {
    let items: Vec<i32> = (1..=15).collect();

    let config = MapConfig {
        max_concurrency: Some(10),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| async move { Ok(format!("processed_{}", item)) }, None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let result_strings: Vec<String> = results
        .succeeded()
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(HighConcurrencyResult {
        total: results.total_count(),
        results: result_strings,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
