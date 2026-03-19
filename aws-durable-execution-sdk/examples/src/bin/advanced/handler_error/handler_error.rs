//! Handler Error Example
//!
//! Demonstrates a top-level handler that returns a `DurableError` and verifies
//! the error is properly serialized in the execution output.

use durable_execution_sdk::{durable_execution, DurableError};

/// A handler that returns a DurableError at the top level.
///
/// This example demonstrates:
/// - Returning `DurableError::execution()` from the handler
/// - The error is properly serialized as `ExecutionStatus::Failed`
/// - The error message is preserved in the execution output
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Perform a step before failing
    let _setup: String = ctx
        .step_named(
            "setup_step",
            |_step_ctx| async move { Ok("setup_done".to_string()) },
            None,
        )
        .await?;

    // Return a top-level handler error
    Err(DurableError::execution("handler_level_failure"))
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
