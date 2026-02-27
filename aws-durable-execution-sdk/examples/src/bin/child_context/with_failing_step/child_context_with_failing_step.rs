//! Child Context with Failing Step Example
//!
//! Demonstrates error propagation when a step inside a child context fails,
//! including `ChildConfig::set_error_mapper()` to transform the error before
//! it propagates to the parent.

use aws_durable_execution_sdk::{durable_execution, ChildConfig, DurableError};
use std::sync::Arc;

/// Run a child context where a step fails, with an error mapper.
///
/// The error mapper transforms the child error before it propagates,
/// allowing the parent handler to catch a mapped error and return
/// a meaningful result.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let config = ChildConfig::new().set_error_mapper(Arc::new(|err: DurableError| {
        DurableError::execution(format!("mapped_error: {}", err))
    }));

    let result = ctx
        .run_in_child_context_named(
            "failing_child",
            |child_ctx| {
                Box::pin(async move {
                    // This step will fail
                    let _: String = child_ctx
                        .step(
                            |_| Err::<String, _>("step failed intentionally".into()),
                            None,
                        )
                        .await?;
                    Ok("should not reach here".to_string())
                })
            },
            Some(config),
        )
        .await;

    match result {
        Ok(val) => Ok(val),
        Err(e) => Ok(format!("caught error: {}", e)),
    }
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
