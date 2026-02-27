//! Wait-for-Callback Nested (Child Context) Example
//!
//! This example demonstrates using `wait_for_callback` inside a child context
//! created with `ctx.run_in_child_context_named()`. The callback is created and
//! awaited within the child context's closure, showing that durable callback
//! operations work correctly in nested execution scopes.
//!
//! # Key Concepts
//!
//! - Calling `wait_for_callback` on a child `DurableContext`
//! - Child contexts provide isolated operation namespaces
//! - The callback submitter runs inside the child context scope
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_nested
//! ```

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

/// Response received from the external system via callback inside a child context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NestedCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Demonstrates wait_for_callback called inside a child context.
///
/// The handler creates a child context and calls `wait_for_callback` on the
/// child context. The callback submitter prints the callback ID, simulating
/// notification of an external system. This pattern is useful when you want
/// to isolate callback operations within a sub-workflow.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<NestedCallbackResponse, DurableError> {
    // Run wait_for_callback inside a child context
    let result = ctx
        .run_in_child_context_named(
            "nested_callback",
            |child_ctx| {
                Box::pin(async move {
                    let response: NestedCallbackResponse = child_ctx
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
