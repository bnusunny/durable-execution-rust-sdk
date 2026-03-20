//! Step Error Determinism Example
//!
//! Demonstrates that a step which fails deterministically produces the same
//! error across replays, ensuring consistent error behavior.

use durable_execution_sdk::{durable_execution, DurableError};

/// Execute a step that always fails deterministically.
///
/// This example demonstrates:
/// - A step that always returns an error
/// - The error is consistent across replays
/// - The handler propagates the step error
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let result: String = ctx
        .step_named(
            "always_fails",
            |_step_ctx| async move { Err::<String, _>("deterministic_failure".into()) },
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
