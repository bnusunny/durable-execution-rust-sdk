//! Callback with Timeout Example
//!
//! Configuring callback timeouts and heartbeat intervals.

use aws_durable_execution_sdk::{durable_execution, CallbackConfig, Duration, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResponse {
    pub approved: bool,
    pub approver: String,
    pub comments: Option<String>,
}

/// Create a callback with custom timeout configuration.
///
/// This example demonstrates:
/// - Setting a callback timeout
/// - Setting a heartbeat timeout for long-running operations
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<ApprovalResponse, DurableError> {
    // Configure callback with custom timeouts
    let config = CallbackConfig {
        timeout: Duration::from_hours(24),           // 24 hours to respond
        heartbeat_timeout: Duration::from_hours(1),  // Heartbeat every hour
        ..Default::default()
    };

    let callback = ctx
        .create_callback_named::<ApprovalResponse>("approval_callback", Some(config))
        .await?;

    // Send callback ID to approval system
    println!("Approval callback ID: {}", callback.callback_id);

    // Wait for approval (suspends until callback is received or timeout)
    let approval = callback.result().await?;

    Ok(approval)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
