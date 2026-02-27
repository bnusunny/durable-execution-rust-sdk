//! Retry Exhaustion Example
//!
//! Demonstrates a step that exhausts all retry attempts and surfaces the final
//! error to the handler.

use aws_durable_execution_sdk::{
    durable_execution, DurableError, Duration, ExponentialBackoff, StepConfig,
};

/// Execute a step that always fails, exhausting retry attempts.
///
/// This example demonstrates:
/// - Configuring a step with `max_attempts: 2`
/// - The step always fails, exhausting all retries
/// - The final error is surfaced to the handler
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retry_strategy: Some(Box::new(ExponentialBackoff::new(
            2,
            Duration::from_seconds(1),
        ))),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "always_failing_step",
            |_step_ctx| Err::<String, _>("step_always_fails".into()),
            Some(config),
        )
        .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
