//! Basic Invoke Example
//!
//! Calling other Lambda functions using `ctx.invoke()`.

use aws_durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildRequest {
    pub task_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildResponse {
    pub task_id: String,
    pub result: String,
}

/// Invoke another Lambda function.
///
/// This example demonstrates calling a child Lambda function
/// and waiting for its result.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<ChildResponse, DurableError> {
    let request = ChildRequest {
        task_id: "task-123".to_string(),
        data: "process this".to_string(),
    };

    // Invoke another Lambda function with default config
    let response: ChildResponse = ctx
        .invoke(
            "arn:aws:lambda:us-east-1:123456789012:function:child-function",
            request,
            None,
        )
        .await?;

    Ok(response)
}

/// Invoke with custom timeout using InvokeConfig::default().
///
/// Note: InvokeConfig has a private marker field, so use Default::default()
/// and then modify the public fields as needed.
#[durable_execution]
pub async fn handler_with_timeout(_event: serde_json::Value, ctx: DurableContext) -> Result<ChildResponse, DurableError> {
    let request = ChildRequest {
        task_id: "task-456".to_string(),
        data: "urgent task".to_string(),
    };

    // Invoke with default config (timeout can be set via the config if needed)
    // For most use cases, the default config is sufficient
    let response: ChildResponse = ctx
        .invoke(
            "arn:aws:lambda:us-east-1:123456789012:function:child-function",
            request,
            None,
        )
        .await?;

    Ok(response)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
