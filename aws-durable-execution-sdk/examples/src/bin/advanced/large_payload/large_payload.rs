//! Large Payload Example
//!
//! Demonstrates a handler that processes a large input payload and produces
//! a large output, exercising `ctx.complete_execution_if_large()`.

use durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeInput {
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LargeOutput {
    pub processed: String,
    pub size: usize,
}

/// Process a large payload and handle oversized results.
///
/// This example demonstrates:
/// - Processing a large input payload
/// - Using `ctx.complete_execution_if_large()` to handle oversized results
/// - If the result exceeds Lambda's response size limit, it is checkpointed
#[durable_execution]
pub async fn handler(event: LargeInput, ctx: DurableContext) -> Result<LargeOutput, DurableError> {
    let processed: String = ctx
        .step_named(
            "process_large_data",
            |_step_ctx| {
                // Simulate processing the large input
                Ok(format!("processed_{}_bytes", event.data.len()))
            },
            None,
        )
        .await?;

    let result = LargeOutput {
        size: processed.len(),
        processed,
    };

    // Check if the result is too large for Lambda response
    if ctx.complete_execution_if_large(&result).await? {
        // Result was checkpointed; return a placeholder
        return Ok(LargeOutput::default());
    }

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
