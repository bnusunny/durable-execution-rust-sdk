//! Child Context Checkpoint Size Limit Example
//!
//! Demonstrates behavior when a child context result approaches the
//! checkpoint size limit, using `ChildConfig::set_summary_generator()`
//! as mitigation to keep checkpoints compact.

use durable_execution_sdk::{durable_execution, ChildConfig, DurableError};
use std::sync::Arc;

/// Run a child context with a moderately large result and summary generator.
///
/// Produces a ~300KB result that exceeds the 256KB threshold, triggering
/// the summary generator to store a compact representation instead.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = ChildConfig::new().set_summary_generator(Arc::new(|serialized: &str| {
        format!(
            "checkpoint_summary: original_size={}, truncated=true",
            serialized.len()
        )
    }));

    let result = ctx
        .run_in_child_context_named(
            "checkpoint_limit_child",
            |child_ctx| {
                Box::pin(async move {
                    // Produce a moderately large result (~300KB)
                    let data: String = child_ctx
                        .step(
                            |_| {
                                let payload = "y".repeat(300_000);
                                Ok(payload)
                            },
                            None,
                        )
                        .await?;
                    Ok(data)
                })
            },
            Some(config),
        )
        .await?;

    Ok(format!("result_length={}", result.len()))
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
