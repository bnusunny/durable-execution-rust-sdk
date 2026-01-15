//! Map with Failure Tolerance Example
//!
//! Processing items with configurable failure tolerance using `CompletionConfig`.

use aws_durable_execution_sdk::{durable_execution, CompletionConfig, DurableError, MapConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResult {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
}

/// Process items with failure tolerance.
///
/// This example allows up to 10% of items to fail without
/// failing the entire batch operation.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<BatchResult, DurableError> {
    let items: Vec<i32> = (1..=20).collect();
    let total = items.len();

    // Configure map with failure tolerance
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            // Allow up to 10% failures
            tolerated_failure_percentage: Some(0.1),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            items,
            |child_ctx, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| {
                            // Simulate some items failing
                            if item % 7 == 0 {
                                Err("Simulated failure".into())
                            } else {
                                Ok(item * 2)
                            }
                        }, None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    Ok(BatchResult {
        total,
        successful: results.succeeded().len(),
        failed: results.failed().len(),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
