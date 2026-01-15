//! Hello World Example
//!
//! A simple hello world example with no durable operations.
//! This demonstrates the basic structure of a durable function.

use aws_durable_execution_sdk::{durable_execution, DurableError};

/// Simple durable function that returns a greeting.
///
/// This example shows the minimal structure of a durable function.
/// No durable operations are performed - it simply returns a value.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, _ctx: DurableContext) -> Result<String, DurableError> {
    Ok("Hello World!".to_string())
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
