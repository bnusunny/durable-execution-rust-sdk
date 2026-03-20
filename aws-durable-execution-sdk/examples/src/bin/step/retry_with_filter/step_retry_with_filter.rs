//! Step Retry with Error Filter Example
//!
//! Demonstrates configuring a step with a `RetryableErrorFilter` that retries
//! only specific error patterns using `ErrorPattern::Contains`.

use durable_execution_sdk::{
    durable_execution, DurableError, ErrorPattern, RetryableErrorFilter, StepConfig,
};

/// Execute a step with a retryable error filter.
///
/// This example demonstrates:
/// - Creating a `RetryableErrorFilter` with `ErrorPattern::Contains`
/// - Only errors matching the filter patterns will be retried
/// - Non-matching errors fail immediately
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = StepConfig {
        retryable_error_filter: Some(RetryableErrorFilter {
            patterns: vec![ErrorPattern::Contains("transient".to_string())],
            error_types: vec![],
        }),
        ..Default::default()
    };

    let result: String = ctx
        .step_named(
            "filtered_operation",
            |_step_ctx| async move { Ok("filter_configured".to_string()) },
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
