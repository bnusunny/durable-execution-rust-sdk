//! Wait-for-Callback in Child Context (Unnamed) Example
//!
//! This example demonstrates using `wait_for_callback` inside a child context
//! created with `ctx.run_in_child_context()` (the unnamed variant). A preparatory
//! step runs before the child context to differentiate this from the named
//! `wfc_nested` example.
//!
//! # Key Concepts
//!
//! - Using `run_in_child_context` (unnamed) to create an isolated scope
//! - Calling `wait_for_callback` on the child `DurableContext`
//! - Performing a step before entering the child context
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_child_context
//! ```

use durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

/// Response received from the external system via callback inside a child context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildContextCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Demonstrates wait_for_callback used within `run_in_child_context` (unnamed).
///
/// The handler first runs a preparatory step, then creates an unnamed child
/// context and calls `wait_for_callback` on the child context. The callback
/// submitter prints the callback ID, simulating notification of an external
/// system.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<ChildContextCallbackResponse, DurableError> {
    // Step before the child context
    let _prepared = ctx
        .step(|_| async move { Ok("prepared".to_string()) }, None)
        .await?;

    // Run wait_for_callback inside an unnamed child context
    let result = ctx
        .run_in_child_context(
            |child_ctx| {
                Box::pin(async move {
                    let response: ChildContextCallbackResponse = child_ctx
                        .wait_for_callback(
                            move |callback_id| async move {
                                println!("Callback ID (from child context): {}", callback_id);
                                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                            },
                            None,
                        )
                        .await?;
                    Ok(response)
                })
            },
            None,
        )
        .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
