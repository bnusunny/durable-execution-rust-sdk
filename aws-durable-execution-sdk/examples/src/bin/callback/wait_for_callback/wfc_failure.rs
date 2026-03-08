//! Wait-for-Callback Failure Example
//!
//! This example demonstrates using `wait_for_callback` where the callback
//! submitter closure returns an error. The handler catches the resulting
//! `DurableError::UserCode` with `error_type == "SubmitterError"` and returns
//! a meaningful result instead of propagating the error.
//!
//! # Key Concepts
//!
//! - Submitter closures can fail by returning `Err(...)`
//! - Submitter failures surface as `DurableError::UserCode` with `error_type = "SubmitterError"`
//! - The handler can match on the error and produce a graceful result
//!
//! # Running this example
//!
//! ```bash
//! cargo build --example wfc_failure
//! ```

use durable_execution_sdk::{durable_execution, DurableError};
use serde::{Deserialize, Serialize};

/// Result indicating whether the callback succeeded or the submitter failed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureResult {
    pub status: String,
    pub message: String,
}

/// Demonstrates wait_for_callback where the submitter returns an error.
///
/// The handler calls `wait_for_callback` with a submitter that always fails.
/// Instead of propagating the error with `?`, the handler catches it and
/// returns a result indicating the failure was handled gracefully.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<FailureResult, DurableError> {
    // wait_for_callback with a submitter that returns an error
    let result: Result<serde_json::Value, DurableError> = ctx
        .wait_for_callback(
            move |_callback_id| async move {
                // Simulate an external system being unavailable
                Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                    "External system unavailable".into(),
                )
            },
            None,
        )
        .await;

    // Catch the submitter error instead of propagating with ?
    match result {
        Ok(_) => Ok(FailureResult {
            status: "completed".to_string(),
            message: "Callback succeeded unexpectedly".to_string(),
        }),
        Err(DurableError::UserCode {
            message,
            error_type,
            ..
        }) if error_type == "SubmitterError" => Ok(FailureResult {
            status: "submitter_failed".to_string(),
            message: format!("Submitter error caught: {}", message),
        }),
        Err(e) => Err(e),
    }
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    lambda_runtime::tracing::init_default_subscriber();
    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
