//! Step Error Handling Example
//!
//! Demonstrating error handling in durable steps.

use aws_durable_execution_sdk::{durable_execution, DurableError, TerminationReason};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessingInput {
    pub value: i32,
    pub should_fail: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingResult {
    pub success: bool,
    pub message: String,
}

/// Handle errors in durable steps.
///
/// This example demonstrates:
/// - Returning errors from steps
/// - Using DurableError types
/// - Error propagation in workflows
#[durable_execution]
pub async fn handler(event: ProcessingInput, ctx: DurableContext) -> Result<ProcessingResult, DurableError> {
    // Step that may fail based on input
    let result = ctx
        .step_named("process_value", |_| {
            if event.should_fail {
                Err("Processing failed due to invalid input".into())
            } else {
                Ok(event.value * 2)
            }
        }, None)
        .await;

    match result {
        Ok(processed_value) => {
            Ok(ProcessingResult {
                success: true,
                message: format!("Processed value: {}", processed_value),
            })
        }
        Err(_) => {
            // Handle the error gracefully
            // In production, you might want to log, retry, or compensate
            Err(DurableError::Execution {
                message: "Step processing failed".to_string(),
                termination_reason: TerminationReason::ExecutionError,
            })
        }
    }
}

/// Example of validation error handling.
#[durable_execution]
pub async fn validation_handler(event: ProcessingInput, ctx: DurableContext) -> Result<ProcessingResult, DurableError> {
    // Validation step
    let is_valid: bool = ctx
        .step_named("validate_input", |_| {
            Ok(event.value > 0 && event.value < 1000)
        }, None)
        .await?;

    if !is_valid {
        return Err(DurableError::validation("Input value must be between 1 and 999"));
    }

    // Process valid input
    let result: i32 = ctx
        .step_named("process", |_| Ok(event.value * 2), None)
        .await?;

    Ok(ProcessingResult {
        success: true,
        message: format!("Result: {}", result),
    })
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
