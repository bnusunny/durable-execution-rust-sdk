//! Map with Concurrency Limit Example
//!
//! Processing items with controlled concurrency using `MapConfig`.

use aws_durable_execution_sdk::{durable_execution, DurableError, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedItem {
    pub id: String,
    pub result: String,
}

/// Process items with a maximum concurrency limit.
///
/// This is useful when you need to limit concurrent operations
/// to avoid overwhelming downstream services.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<Vec<ProcessedItem>, DurableError> {
    let items: Vec<String> = vec![
        "item-1".to_string(),
        "item-2".to_string(),
        "item-3".to_string(),
        "item-4".to_string(),
        "item-5".to_string(),
        "item-6".to_string(),
        "item-7".to_string(),
        "item-8".to_string(),
        "item-9".to_string(),
        "item-10".to_string(),
    ];

    // Configure map with concurrency limit
    let config = MapConfig {
        max_concurrency: Some(3), // Process at most 3 items concurrently
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: String, index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step_named(
                            &format!("process_{}", index),
                            |_| {
                                Ok(ProcessedItem {
                                    id: item.clone(),
                                    result: format!("processed_{}", item),
                                })
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
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
