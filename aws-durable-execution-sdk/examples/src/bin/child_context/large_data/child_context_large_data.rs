//! Child Context Large Data Example
//!
//! Demonstrates a child context that produces a large result (> 256KB),
//! exercising `ChildConfig::set_summary_generator()` to store a compact
//! summary instead of the full payload.

use durable_execution_sdk::{durable_execution, ChildConfig, DurableError};
use std::sync::Arc;

/// Run a child context producing a large result with a summary generator.
///
/// When the serialized child result exceeds 256KB, the summary generator
/// is invoked and its return value is stored instead of the full result.
/// This keeps checkpoint sizes manageable for large payloads.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = ChildConfig::new().set_summary_generator(Arc::new(|serialized: &str| {
        format!("summary: {} bytes", serialized.len())
    }));

    let result = ctx
        .run_in_child_context_named(
            "large_data_child",
            |child_ctx| {
                Box::pin(async move {
                    // Produce a large string > 256KB (262144 bytes)
                    let large_data: String = child_ctx
                        .step(
                            |_| async move {
                                let data = "x".repeat(300_000);
                                Ok(data)
                            },
                            None,
                        )
                        .await?;
                    Ok(large_data)
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
