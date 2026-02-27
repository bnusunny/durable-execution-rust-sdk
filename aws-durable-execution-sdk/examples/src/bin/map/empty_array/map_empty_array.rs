//! Map Empty Array Example
//!
//! Demonstrates calling `ctx.map()` with an empty input vector, showing that
//! `BatchResult::empty()` behavior is returned with zero items and an
//! `AllCompleted` completion reason.

use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEmptyResult {
    pub total_items: usize,
    pub message: String,
}

/// Handler demonstrating `ctx.map()` with an empty input vector.
///
/// Calls `ctx.map()` with an empty `Vec<i32>`, which returns a `BatchResult`
/// with no items and `CompletionReason::AllCompleted`.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MapEmptyResult, DurableError> {
    let results = ctx
        .map(
            Vec::<i32>::new(),
            |child_ctx: DurableContext, _item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok("".to_string()), None).await })
            },
            None,
        )
        .await?;

    Ok(MapEmptyResult {
        total_items: results.items.len(),
        message: "Empty map completed successfully".to_string(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
