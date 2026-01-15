//! Simple Callback Example
//!
//! Creating a callback ID for external systems to use.
//! Callbacks allow external systems to signal your workflow.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResult {
    pub status: String,
    pub data: String,
}

/// Create a callback and wait for external system response.
///
/// The callback ID should be sent to an external system. The external
/// system then calls the Lambda durable execution callback API with
/// the callback ID and result payload.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<CallbackResult, DurableError> {
    // Create a callback
    let callback = ctx
        .create_callback::<CallbackResult>(None)
        .await?;

    // In a real scenario, you would send the callback_id to an external system
    // For example: notify_external_system(&callback.callback_id).await?;
    println!("Send this callback_id to external system: {}", callback.callback_id);

    // Wait for the callback result
    // The promise is resolved by calling SendDurableExecutionCallbackSuccess
    // with the callback_id from an external system
    let result = callback.result().await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
