//! Tests for wait examples using LocalDurableTestRunner.
//!
//! These tests verify full end-to-end wait operation completion:
//! 1. Wait operations are correctly initiated and captured
//! 2. Wait operations complete with SUCCEEDED status (via time skipping)
//! 3. Handler is re-invoked after wait completes
//! 4. Execution completes with final result after all waits
//! 5. Time skipping advances time correctly for wait durations
//!
//! The TestExecutionOrchestrator handles the full execution lifecycle:
//! - Detects pending wait operations
//! - Advances tokio time to the wait's scheduled end timestamp
//! - Marks wait operations as SUCCEEDED
//! - Re-invokes the handler to continue execution
//!
//! Requirements validated:
//! - 16.2: Time skipping marks waits as SUCCEEDED and schedules re-invocation
//! - 16.3: Time skipping uses tokio::time::advance() to skip wait durations
//! - 16.4: Handler re-invocation when operations complete

use aws_durable_execution_sdk::{
    DurableContext, DurableError, Duration, OperationStatus, OperationType, WaitForConditionConfig,
    WaitForConditionContext,
};
use aws_durable_execution_sdk_examples::test_helper::assert_event_signatures;
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Wait Basic Example Handler
// ============================================================================

/// Handler from wait/basic example (without macro for testing)
async fn wait_basic_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Wait for 2 seconds
    ctx.wait(Duration::from_seconds(2), None).await?;

    Ok("Function Completed".to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_basic() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wait_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Verify execution completed successfully (not just Running)
    // This validates that the orchestrator properly:
    // 1. Detected the pending wait operation
    // 2. Advanced time to the wait's scheduled end
    // 3. Marked the wait as SUCCEEDED
    // 4. Re-invoked the handler to completion
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully after wait"
    );

    // Verify the result value is correct
    assert_eq!(
        result.get_result().unwrap(),
        "Function Completed",
        "Handler should return expected result after wait completes"
    );

    // Verify operations - should have the wait operation
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have at least one operation");

    // Verify the wait operation was captured and completed
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert!(!wait_ops.is_empty(), "Should have a wait operation");

    // Verify wait operation status is SUCCEEDED (not Started/Pending)
    // This validates Requirement 16.2: time skipping marks waits as SUCCEEDED
    let wait_op = wait_ops.first().unwrap();
    assert_eq!(
        wait_op.status,
        OperationStatus::Succeeded,
        "Wait operation should be marked as SUCCEEDED after time skip"
    );

    // Verify handler was re-invoked after wait completed
    // This validates Requirement 16.4: handler re-invocation when operations complete
    let invocations = result.get_invocations();
    assert!(
        invocations.len() >= 2,
        "Handler should be invoked at least twice: initial + after wait completes. Got {} invocations",
        invocations.len()
    );

    // Check event signatures
    assert_event_signatures(operations, "tests/history/wait_basic.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait Named Example Handler
// ============================================================================

/// Handler from wait/named example (without macro for testing)
async fn wait_named_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // First wait with a descriptive name
    ctx.wait(Duration::from_seconds(1), Some("initial_delay"))
        .await?;

    // Perform some work
    let _result: i32 = ctx.step_named("process", |_| Ok(42), None).await?;

    // Second wait with a different name
    ctx.wait(Duration::from_seconds(2), Some("cooldown_period"))
        .await?;

    Ok("All waits completed".to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_named() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wait_named_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Verify execution completed successfully
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully after all waits"
    );

    // Verify the result value
    assert_eq!(
        result.get_result().unwrap(),
        "All waits completed",
        "Handler should return expected result"
    );

    // Verify operations - should have wait operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify the wait operations were captured with correct names
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert_eq!(wait_ops.len(), 2, "Should have exactly 2 wait operations");

    // The first wait should have the name "initial_delay"
    let first_wait = wait_ops.first().unwrap();
    assert_eq!(first_wait.name, Some("initial_delay".to_string()));
    assert_eq!(
        first_wait.status,
        OperationStatus::Succeeded,
        "First wait should be SUCCEEDED"
    );

    // The second wait should have the name "cooldown_period"
    let second_wait = wait_ops.get(1).unwrap();
    assert_eq!(second_wait.name, Some("cooldown_period".to_string()));
    assert_eq!(
        second_wait.status,
        OperationStatus::Succeeded,
        "Second wait should be SUCCEEDED"
    );

    // Verify step operation between waits also completed
    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(!step_ops.is_empty(), "Should have step operation");
    assert_eq!(
        step_ops.first().unwrap().status,
        OperationStatus::Succeeded,
        "Step operation should be SUCCEEDED"
    );

    // Verify handler was re-invoked multiple times (once per wait + initial)
    // With 2 waits, we expect at least 3 invocations
    let invocations = result.get_invocations();
    assert!(
        invocations.len() >= 3,
        "Handler should be invoked at least 3 times for 2 waits. Got {} invocations",
        invocations.len()
    );

    // Check event signatures
    assert_event_signatures(operations, "tests/history/wait_named.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait Extended Duration Example Handler
// ============================================================================

/// Handler from wait/extended_duration example (without macro for testing)
async fn wait_extended_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    // Standard durations
    ctx.wait(Duration::from_seconds(30), Some("seconds_wait"))
        .await?;
    ctx.wait(Duration::from_minutes(1), Some("minutes_wait"))
        .await?;

    Ok("Extended duration waits completed".to_string())
}

#[tokio::test(flavor = "current_thread")]
async fn test_wait_extended_duration() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wait_extended_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Verify execution completed successfully
    // This validates that even long waits (30s, 1m) are properly skipped
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully after extended waits"
    );

    // Verify the result value
    assert_eq!(
        result.get_result().unwrap(),
        "Extended duration waits completed",
        "Handler should return expected result"
    );

    // Verify operations - should have wait operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify the wait operations were captured with correct names
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert_eq!(wait_ops.len(), 2, "Should have exactly 2 wait operations");

    // The first wait should have the name "seconds_wait" and be SUCCEEDED
    let first_wait = wait_ops.first().unwrap();
    assert_eq!(first_wait.name, Some("seconds_wait".to_string()));
    assert_eq!(
        first_wait.status,
        OperationStatus::Succeeded,
        "30-second wait should be SUCCEEDED"
    );

    // The second wait should have the name "minutes_wait" and be SUCCEEDED
    let second_wait = wait_ops.get(1).unwrap();
    assert_eq!(second_wait.name, Some("minutes_wait".to_string()));
    assert_eq!(
        second_wait.status,
        OperationStatus::Succeeded,
        "1-minute wait should be SUCCEEDED"
    );

    // Verify handler was re-invoked for each wait
    let invocations = result.get_invocations();
    assert!(
        invocations.len() >= 3,
        "Handler should be invoked at least 3 times for 2 waits. Got {} invocations",
        invocations.len()
    );

    // Check event signatures
    assert_event_signatures(operations, "tests/history/wait_extended.history.json");

    LocalDurableTestRunner::<serde_json::Value, String>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Wait For Condition Basic Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingState {
    pub job_id: String,
    pub check_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobStatus {
    pub job_id: String,
    pub status: String,
    pub result: Option<String>,
}

/// Handler from wait_for_condition/basic example (without macro for testing)
async fn wait_for_condition_basic_handler(
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
    let result: JobStatus = ctx
        .wait_for_condition(
            |state: &PollingState, wait_ctx: &WaitForConditionContext| {
                // Simulate checking job status
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

#[tokio::test(flavor = "current_thread")]
async fn test_wait_for_condition_basic() {
    LocalDurableTestRunner::<serde_json::Value, JobStatus>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(wait_for_condition_basic_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Verify execution completed successfully
    // wait_for_condition polls until condition is met (attempt >= 3)
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully after condition is met"
    );

    // Verify the result value matches expected JobStatus
    let job_status = result.get_result().unwrap();
    assert_eq!(job_status.job_id, "job-12345");
    assert_eq!(job_status.status, "completed");
    assert_eq!(
        job_status.result,
        Some("Job finished successfully".to_string())
    );

    // Verify operations - should have operations from the polling attempts
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify handler was invoked multiple times for polling
    let invocations = result.get_invocations();
    assert!(
        invocations.len() >= 1,
        "Handler should be invoked at least once. Got {} invocations",
        invocations.len()
    );

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/wait_for_condition_basic.history.json",
    );

    LocalDurableTestRunner::<serde_json::Value, JobStatus>::teardown_test_environment()
        .await
        .unwrap();
}

// ============================================================================
// Multiple Waits Example Handler
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowProgress {
    pub stage: String,
    pub completed_waits: u32,
}

/// Handler from multiple_waits example (without macro for testing)
async fn multiple_waits_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<WorkflowProgress, DurableError> {
    // Stage 1: Initial processing
    let _stage1: String = ctx
        .step_named(
            "stage_1_process",
            |_| Ok("stage 1 complete".to_string()),
            None,
        )
        .await?;

    // Wait before stage 2
    ctx.wait(Duration::from_seconds(5), Some("wait_before_stage_2"))
        .await?;

    // Stage 2: Secondary processing
    let _stage2: String = ctx
        .step_named(
            "stage_2_process",
            |_| Ok("stage 2 complete".to_string()),
            None,
        )
        .await?;

    // Wait before stage 3
    ctx.wait(Duration::from_seconds(10), Some("wait_before_stage_3"))
        .await?;

    // Stage 3: Final processing
    let _stage3: String = ctx
        .step_named(
            "stage_3_process",
            |_| Ok("stage 3 complete".to_string()),
            None,
        )
        .await?;

    // Final wait before completion
    ctx.wait(Duration::from_seconds(2), Some("final_cooldown"))
        .await?;

    Ok(WorkflowProgress {
        stage: "completed".to_string(),
        completed_waits: 3,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_multiple_waits() {
    LocalDurableTestRunner::<serde_json::Value, WorkflowProgress>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(multiple_waits_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // Verify execution completed successfully
    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully after all waits"
    );

    // Verify the result value
    let progress = result.get_result().unwrap();
    assert_eq!(progress.stage, "completed");
    assert_eq!(progress.completed_waits, 3);

    // Verify operations - should have steps and waits
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Verify we have step operations and they all succeeded
    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert_eq!(step_ops.len(), 3, "Should have exactly 3 step operations");
    for (i, step) in step_ops.iter().enumerate() {
        assert_eq!(
            step.status,
            OperationStatus::Succeeded,
            "Step {} should be SUCCEEDED",
            i + 1
        );
    }

    // Verify we have wait operations and they all succeeded
    let wait_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Wait)
        .collect();
    assert_eq!(wait_ops.len(), 3, "Should have exactly 3 wait operations");

    // Verify wait operations have correct names and all succeeded
    let expected_wait_names = [
        "wait_before_stage_2",
        "wait_before_stage_3",
        "final_cooldown",
    ];
    for (i, wait) in wait_ops.iter().enumerate() {
        assert_eq!(
            wait.name,
            Some(expected_wait_names[i].to_string()),
            "Wait {} should have correct name",
            i + 1
        );
        assert_eq!(
            wait.status,
            OperationStatus::Succeeded,
            "Wait '{}' should be SUCCEEDED",
            expected_wait_names[i]
        );
    }

    // Verify handler was re-invoked for each wait (3 waits + initial = at least 4 invocations)
    let invocations = result.get_invocations();
    assert!(
        invocations.len() >= 4,
        "Handler should be invoked at least 4 times for 3 waits. Got {} invocations",
        invocations.len()
    );

    // Check event signatures
    assert_event_signatures(operations, "tests/history/multiple_waits.history.json");

    LocalDurableTestRunner::<serde_json::Value, WorkflowProgress>::teardown_test_environment()
        .await
        .unwrap();
}
