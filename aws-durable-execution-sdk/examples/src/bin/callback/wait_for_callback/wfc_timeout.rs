//! Wait-for-Callback Timeout Example
//!
//! This example demonstrates using `wait_for_callback` with a short timeout
//! configured via `CallbackConfig { timeout: Duration }`. The callback submitter
//! logs the callback ID (simulating notifying an external system), but since no
//! external system responds, the callback will eventually time out.
//!
//! # Key Concepts
//!
//! - Configuring a callback timeout with `CallbackConfig`
//! - The submitter closure receives the callback ID for external notification
//! - When no response arrives within the timeout, the SDK raises a timeout error
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_timeout
//! ```

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

/// Response received from the external system via callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackResponse {
    pub status: String,
    pub message: String,
}

/// Demonstrates wait_for_callback with a short timeout.
///
/// The handler sets up a callback with a 5-second timeout. The submitter
/// simply prints the callback ID. Since no external system will respond,
/// the callback will time out after 5 seconds.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<CallbackResponse, DurableError> {
    // Configure a short timeout for the callback
    let config = CallbackConfig {
        timeout: Duration::from_seconds(5),
        ..Default::default()
    };

    // wait_for_callback: create callback, notify external system, and wait
    // Since no external system responds, this will time out after 5 seconds
    let response: CallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                // In a real application, you would notify an external system here.
                // The external system would use the callback_id to send a response.
                println!("Callback ID for external system: {}", callback_id);
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            },
            Some(config),
        )
        .await?;

    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
