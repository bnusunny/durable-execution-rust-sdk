//! Callback Heartbeat Example
//!
//! Demonstrates creating a named callback with `CallbackConfig::heartbeat_timeout`
//! configured. The heartbeat timeout ensures the external system must send periodic
//! heartbeat signals to keep the callback alive, preventing stale callbacks from
//! blocking execution indefinitely.

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, DurableError, Duration};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub status: String,
    pub data: String,
}

/// Create a named callback with heartbeat timeout configuration.
///
/// This example demonstrates:
/// - Using `create_callback_named` to create a callback with a specific name
/// - Configuring `heartbeat_timeout` via `CallbackConfig`
/// - Waiting for the callback result
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<HeartbeatResponse, DurableError> {
    let config = CallbackConfig {
        timeout: Duration::from_hours(2),
        heartbeat_timeout: Duration::from_minutes(10),
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<HeartbeatResponse>("heartbeat_callback", Some(config))
        .await?;

    // In a real scenario, send the callback_id to an external system
    // that will periodically send heartbeats and eventually complete the callback
    println!("Heartbeat callback ID: {}", callback.callback_id);

    // Wait for the callback result (suspends until callback is received or timeout)
    let result = callback.result().await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
