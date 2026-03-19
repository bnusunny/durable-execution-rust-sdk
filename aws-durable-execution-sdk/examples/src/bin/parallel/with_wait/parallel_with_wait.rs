//! Parallel With Wait Example
//!
//! Demonstrates parallel branches that include `ctx.wait()` calls within each branch.
//! Each branch waits for a different duration before performing a step operation.

use durable_execution_sdk::{
    durable_execution, CompletionReason, DurableContext, DurableError, Duration, MapConfig,
};

/// Handler demonstrating parallel tasks with wait calls in each branch.
///
/// Uses `ctx.map()` with 3 items where each branch does a `ctx.wait()` followed
/// by a `ctx.step()`. Each branch waits for a different duration (1s, 2s, 3s)
/// and then returns a result string.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<String>, DurableError> {
    let results = ctx
        .map(
            vec![1, 2, 3],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    // Wait for item seconds
                    child_ctx
                        .wait(
                            Duration::from_seconds(item as u64),
                            Some(&format!("wait_{}", item)),
                        )
                        .await?;
                    // Then do a step that returns a result
                    child_ctx
                        .step(|_| async move { Ok(format!("branch {} done", item)) }, None)
                        .await
                })
            },
            Some(MapConfig {
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    // If the map operation is suspended (waiting for waits to complete),
    // propagate the suspension so the orchestrator can re-invoke
    if results.completion_reason == CompletionReason::Suspended {
        return Err(DurableError::suspend());
    }

    results
        .get_results()
        .map(|refs| refs.into_iter().cloned().collect())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
