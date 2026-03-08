//! Step Retry with Exponential Backoff Example
//!
//! Demonstrates configuring a step with `ExponentialBackoff` retry strategy
//! including custom `max_attempts`, `base_delay`, `max_delay`, `multiplier`,
//! and `JitterStrategy`.

use durable_execution_sdk::{
    durable_execution, DurableError, Duration, ExponentialBackoff, JitterStrategy, StepConfig,
};

/// Execute a step with exponential backoff retry configuration.
///
/// This example demonstrates:
/// - Building an `ExponentialBackoff` strategy via the builder pattern
/// - Configuring max_attempts, base_delay, max_delay, multiplier, and jitter
/// - Passing the retry strategy to `StepConfig`
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retry_strategy: Some(Box::new(
            ExponentialBackoff::builder()
                .max_attempts(3)
                .base_delay(Duration::from_seconds(1))
                .max_delay(Duration::from_seconds(10))
                .multiplier(2.0)
                .jitter(JitterStrategy::Full)
                .build(),
        )),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "retryable_operation",
            |_step_ctx| Ok("operation_succeeded".to_string()),
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
