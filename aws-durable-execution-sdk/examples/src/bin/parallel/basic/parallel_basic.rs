//! Basic Parallel Example
//!
//! Running multiple operations in parallel using `ctx.map()` with indices.
//! For heterogeneous parallel operations, use child contexts.

use aws_durable_execution_sdk::{durable_execution, DurableContext, DurableError, MapConfig};

/// Execute multiple independent operations in parallel.
///
/// This example uses `map` with task indices to simulate parallel branches.
/// Each task runs in its own child context.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<String>, DurableError> {
    let task_ids = vec![1, 2, 3];

    let results = ctx
        .map(
            task_ids,
            |child_ctx: DurableContext, task_id: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("task {} completed", task_id)), None)
                        .await
                })
            },
            Some(MapConfig {
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
