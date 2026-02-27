//! Wait-for-Callback Heartbeat Example
//!
//! This example demonstrates using `wait_for_callback` with a `CallbackConfig`
//! that has `heartbeat_timeout` set. The heartbeat timeout tells the SDK how
//! long to wait between heartbeat signals before considering the callback stale.
//!
//! In the `wait_for_callback` pattern, the submitter runs inside a child context
//! and notifies an external system of the callback ID. The external system is
//! responsible for sending heartbeats (via `send_heartbeat()`) and eventually
//! completing the callback. This example demonstrates configuring the
//! `heartbeat_timeout` field.
//!
//! # Key Concepts
//!
//! - Configuring `heartbeat_timeout` on `CallbackConfig`
//! - The submitter logs the callback ID for an external system
//! - Heartbeats are sent by the external system, not the submitter
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_heartbeat
//! ```

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

/// Response received from the external system via callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatCallbackResponse {
    pub status: String,
    pub data: String,
}

/// Demonstrates wait_for_callback with heartbeat timeout configured.
///
/// The handler sets up a callback with both a timeout and a heartbeat timeout.
/// The submitter prints the callback ID, simulating notifying an external system.
/// The external system would then periodically send heartbeats and eventually
/// complete the callback with a response.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<HeartbeatCallbackResponse, DurableError> {
    // Configure callback with heartbeat timeout
    let config = CallbackConfig {
        timeout: Duration::from_hours(1),
        heartbeat_timeout: Duration::from_minutes(5),
        ..Default::default()
    };

    // wait_for_callback: create callback, notify external system, and wait
    let response: HeartbeatCallbackResponse = ctx
        .wait_for_callback(
            move |callback_id| async move {
                // In a real application, you would notify an external system here.
                // The external system would use the callback_id to:
                // 1. Periodically send heartbeats via send_heartbeat()
                // 2. Eventually complete the callback with a response
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
