//! Parallel Min Successful with Callback Example
//!
//! Demonstrates `CompletionConfig::with_min_successful(n)` combined with callback
//! operations in parallel branches using `ctx.map()`. Each branch creates a callback
//! via `create_callback_named` and waits for its result. The map operation completes
//! and returns a `BatchResult` even though individual callbacks are still pending.

use aws_durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinSuccessfulCallbackResult {
    pub required: usize,
    pub results: Vec<String>,
}

/// Handler demonstrating parallel min_successful with callbacks in each branch.
///
/// Runs 3 services through `ctx.map()` with `CompletionConfig::with_min_successful(1)`.
/// Each branch creates a named callback and waits for its result. Since no external
/// system responds, all callbacks suspend and the execution stays Running.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MinSuccessfulCallbackResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(3),
        completion_config: CompletionConfig::with_min_successful(1),
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![
                "service-a".to_string(),
                "service-b".to_string(),
                "service-c".to_string(),
            ],
            |child_ctx: DurableContext, item: String, _index: usize| {
                Box::pin(async move {
                    let callback = child_ctx
                        .create_callback_named::<String>(
                            &format!("{}_callback", item),
                            None,
                        )
                        .await?;
                    println!("Callback for {}: {}", item, callback.callback_id);
                    callback.result().await
                })
            },
            Some(config),
        )
        .await?;

    let succeeded = results.succeeded();
    let successful_results: Vec<String> = succeeded
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(MinSuccessfulCallbackResult {
        required: 1,
        results: successful_results,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
