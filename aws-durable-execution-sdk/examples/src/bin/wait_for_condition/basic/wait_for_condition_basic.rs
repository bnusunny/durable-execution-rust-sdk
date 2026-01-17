//! Wait for Condition Example
//!
//! Polling until a condition is met using `ctx.wait_for_condition()`.

use aws_durable_execution_sdk::{
    durable_execution, DurableError, Duration, WaitForConditionConfig, WaitForConditionContext,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub job_id: String,
    pub check_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatus {
    pub job_id: String,
    pub status: String,
    pub result: Option<String>,
}

/// Poll until a job completes.
///
/// This example demonstrates using `wait_for_condition` to poll
/// an external system until a condition is met.
#[durable_execution]
pub async fn handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<JobStatus, DurableError> {
    // Configure polling behavior with initial state
    let config = WaitForConditionConfig {
        initial_state: PollingState {
            job_id: "job-12345".to_string(),
            check_count: 0,
        },
        interval: Duration::from_seconds(5), // Poll every 5 seconds
        max_attempts: Some(10),              // Max 10 attempts
        timeout: Some(Duration::from_minutes(5)),
    };

    // Poll until job completes
    // The check function receives (&S, &WaitForConditionContext) and returns Result<T, Error>
    let result: JobStatus = ctx
        .wait_for_condition(
            |state: &PollingState, wait_ctx: &WaitForConditionContext| {
                // Simulate checking job status
                // In production, this would call an external API
                let status = if wait_ctx.attempt >= 3 {
                    "completed"
                } else {
                    "running"
                };

                if status == "completed" {
                    // Return Ok(T) when condition is met
                    Ok(JobStatus {
                        job_id: state.job_id.clone(),
                        status: status.to_string(),
                        result: Some("Job finished successfully".to_string()),
                    })
                } else {
                    // Return Err to continue polling
                    Err(format!("Job still running, attempt {}", wait_ctx.attempt).into())
                }
            },
            config,
        )
        .await?;

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
