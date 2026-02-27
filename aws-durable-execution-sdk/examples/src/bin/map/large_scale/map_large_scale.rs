//! Map Large Scale Example
//!
//! Demonstrates `ctx.map()` processing 50 items with `MapConfig` configured
//! with `max_concurrency: Some(10)` for controlled batch-like processing.

use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LargeScaleResult {
    pub total: usize,
    pub results: Vec<i32>,
}

/// Handler demonstrating `ctx.map()` processing a large number of items.
///
/// Maps 50 items (integers 1..=50) with `max_concurrency: Some(10)`.
/// Each item performs a step that doubles the input value.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<LargeScaleResult, DurableError> {
    let items: Vec<i32> = (1..=50).collect();

    let config = MapConfig {
        max_concurrency: Some(10),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx.step(|_| Ok(item * 2), None).await
                })
            },
            Some(config),
        )
        .await?;

    let result_values: Vec<i32> = results
        .succeeded()
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(LargeScaleResult {
        total: results.total_count(),
        results: result_values,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
