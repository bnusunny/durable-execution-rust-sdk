//! Map Error Preservation Example
//!
//! Demonstrates that individual item errors are preserved and accessible via
//! `BatchItem::get_error()` when some items fail during mapping. Uses
//! `CompletionConfig` with `tolerated_failure_count` so the overall operation
//! succeeds despite individual failures.

use durable_execution_sdk::{
    durable_execution, CompletionConfig, DurableContext, DurableError, MapConfig,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapErrorPreservationResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub error_messages: Vec<String>,
}

/// Handler demonstrating that individual item errors are preserved in map results.
///
/// Maps 8 items where items 2, 5, and 8 fail. Configures `tolerated_failure_count: Some(3)`
/// so the overall operation succeeds. After completion, inspects `results.failed()` to
/// collect error messages from each failed item.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MapErrorPreservationResult, DurableError> {
    let config = MapConfig {
        max_concurrency: Some(8),
        completion_config: CompletionConfig {
            tolerated_failure_count: Some(3),
            ..Default::default()
        },
        ..Default::default()
    };

    let results = ctx
        .map(
            vec![1, 2, 3, 4, 5, 6, 7, 8],
            |child_ctx: DurableContext, item: i32, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(
                            |_| async move {
                                if item == 2 || item == 5 || item == 8 {
                                    Err(format!("Item {} processing failed", item).into())
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

    Ok(MapErrorPreservationResult {
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
