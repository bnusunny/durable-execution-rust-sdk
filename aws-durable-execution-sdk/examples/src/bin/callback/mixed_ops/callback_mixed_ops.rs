//! Callback Mixed Operations Example
//!
//! Demonstrates combining `create_callback_named` with other durable operations
//! (steps, waits) in a single workflow. This shows how callbacks integrate
//! naturally with the rest of the SDK's operation types.
//!
//! # Key Concepts
//!
//! - Using `ctx.step_named()` to perform a preparatory step
//! - Using `ctx.wait()` to pause execution for a duration
//! - Using `ctx.create_callback_named()` to create a callback
//! - Combining multiple operation types in a single handler
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example callback_mixed_ops
//! ```

use aws_durable_execution_sdk::{durable_execution, DurableError, Duration};
use serde::{Deserialize, Serialize};

/// Response type for the callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedOpsCallbackResponse {
    pub approval: String,
}

/// Final output combining results from all operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedOpsResult {
    pub prepared_data: String,
    pub callback_approval: String,
}

/// Demonstrates a handler that combines steps, waits, and callbacks.
///
/// The handler performs a preparatory step, waits briefly, then creates
/// a callback and waits for its result. This shows how different operation
/// types compose together in a single durable workflow.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<MixedOpsResult, DurableError> {
    // Step 1: Prepare data
    let prepared_data = ctx
        .step_named(
            "prepare",
            |_| Ok("prepared_payload".to_string()),
            None,
        )
        .await?;

    // Step 2: Wait before creating callback
    ctx.wait(Duration::from_seconds(1), Some("pre_callback_delay"))
        .await?;

    // Step 3: Create callback and wait for result
    let callback = ctx
        .create_callback_named::<MixedOpsCallbackResponse>("mixed_callback", None)
        .await?;

    println!("Callback ID: {}", callback.callback_id);

    let result = callback.result().await?;

    Ok(MixedOpsResult {
        prepared_data,
        callback_approval: result.approval,
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
