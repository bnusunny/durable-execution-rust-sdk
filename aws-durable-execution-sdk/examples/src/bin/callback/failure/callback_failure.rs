//! Callback Failure Example
//!
//! Demonstrates creating a named callback with `create_callback_named` and handling
//! the case where the callback result is an error. The handler uses pattern matching
//! on the callback result to distinguish between success and failure responses,
//! returning a structured result in both cases.

use durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCallbackResult {
    pub status: String,
    pub message: String,
}

/// Create a named callback and handle potential failure responses.
///
/// This example demonstrates:
/// - Using `create_callback_named` to create a callback with a specific name
/// - Waiting for the callback result with `callback.result().await`
/// - Pattern matching on the result to handle both success and failure cases
/// - Converting callback errors into structured results instead of propagating them
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<FailureCallbackResult, DurableError> {
    let callback = ctx
        .create_callback_named::<serde_json::Value>("failure_callback", None)
        .await?;

    println!("Failure callback ID: {}", callback.callback_id);

    // Wait for the callback result and handle potential failure
    match callback.result().await {
        Ok(value) => Ok(FailureCallbackResult {
            status: "success".to_string(),
            message: format!("Received: {:?}", value),
        }),
        Err(DurableError::Callback { message, .. }) => Ok(FailureCallbackResult {
            status: "callback_failed".to_string(),
            message: format!("Callback failed: {}", message),
        }),
        Err(e) => Err(e),
    }
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
