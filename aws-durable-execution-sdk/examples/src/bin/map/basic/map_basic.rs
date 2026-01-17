//! Basic Map Example
//!
//! Processing arrays with concurrent operations using `ctx.map()`.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Process an array of items with map.
///
/// Each item is processed in its own child context with
/// independent checkpointing.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<i32>, DurableError> {
    let items = vec![1, 2, 3, 4, 5];

    let results = ctx
        .map(
            items,
            |child_ctx, item: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok(item * 2), None).await })
            },
            None,
        )
        .await?;

    // Get all successful results
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
