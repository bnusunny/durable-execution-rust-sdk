//! Parallel Error Preservation Example
//!
//! Demonstrates that individual task errors are preserved and accessible via
//! `BatchItem::get_error()` when some parallel branches fail. Uses `CompletionConfig`
//! with `tolerated_failure_count` so the overall operation succeeds despite individual failures.

use durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPreservationResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_messages: Vec<String>,
}

/// Handler demonstrating that individual task errors are preserved in batch results.
///
/// Runs 5 items through `ctx.map()` where even-numbered items fail. Configures
/// `tolerated_failure_count` to allow up to 3 failures so the overall operation
/// succeeds. After completion, inspects `results.failed()` to collect error messages.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ErrorPreservationResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(5),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(3),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| async move {
                                if item % 2 == 0 {
                                    Err(format!("Item {} failed: even numbers not allowed", item)
                                        .into())
                                } else {
                                    Ok(format!("item {} ok", item))
                                }
                            },
                            None,
                        )
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let failed = results.failed();
    let succeeded = results.succeeded();

    let error_messages: Vec<String> = failed
        .iter()
        .map(|item| {
            item.get_error()
                .map(|e| e.error_message.clone())
                .unwrap_or_else(|| "unknown error".to_string())
        })
        .collect();

    Ok(ErrorPreservationResult {
        total: results.total_count(),
        succeeded: succeeded.len(),
        failed: failed.len(),
        error_messages,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
