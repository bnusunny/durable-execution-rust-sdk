//! Map with Minimum Successful Example
//!
//! Processing items with a quorum requirement using `CompletionConfig`.

use aws_durable_execution_sdk::{durable_execution, CompletionConfig, DurableError, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuorumResult {
    pub required: usize,
    pub achieved: usize,
    pub results: Vec<String>,
}

/// Process items with a minimum successful requirement (quorum).
///
/// This pattern is useful when you need a majority of results
/// but don't need all items to succeed.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<QuorumResult, DurableError> {
    let items: Vec<String> = vec![
        "node-1".to_string(),
        "node-2".to_string(),
        "node-3".to_string(),
        "node-4".to_string(),
        "node-5".to_string(),
    ];
    let total = items.len();
    let min_required = (total + 1) / 2; // Majority quorum

    // Configure map with minimum successful requirement
    let config = MapConfig {
        completion_config: CompletionConfig::with_min_successful(min_required),
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("response_from_{}", item)), None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let successful_results: Vec<String> = results
        .items
        .iter()
        .filter_map(|item| item.result.clone())
        .collect();

    Ok(QuorumResult {
        required: min_required,
        achieved: successful_results.len(),
        results: successful_results,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
