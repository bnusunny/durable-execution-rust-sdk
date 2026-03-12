//! Cloud integration tests using CloudDurableTestRunner.
//!
//! These tests deploy example functions to AWS Lambda and verify execution
//! results and event history against the same .history.json files used by
//! local tests, matching the JS SDK's testing pattern.
//!
//! Run with: cargo test --package durable-execution-sdk-examples --test cloud_integration_tests -- --ignored

use durable_execution_sdk_testing::{CloudDurableTestRunner, ExecutionStatus};
use serde_json::json;

/// Helper to get function name from env var or use default with prefix.
fn get_function_name(binary_name: &str) -> String {
    let env_key = binary_name.to_uppercase() + "_FUNCTION";
    std::env::var(&env_key).unwrap_or_else(|_| {
        let prefix =
            std::env::var("FUNCTION_PREFIX").unwrap_or_else(|_| "durable-rust-integ".to_string());
        format!("{}-{}", prefix, binary_name)
    })
}

/// Cloud-specific event signature assertion.
/// Filters InvocationCompleted events (count varies between cloud and local
/// due to timing) and StepStarted events (cloud API doesn't emit StepStarted
/// for steps that complete within a single invocation).
fn assert_cloud_event_signatures<T>(
    result: &durable_execution_sdk_testing::TestResult<T>,
    history_file_path: &str,
) {
    use durable_execution_sdk_examples::test_helper::{
        load_nodejs_history_file, NodeJsEventSignature,
    };

    let skip = |s: &NodeJsEventSignature| {
        s.event_type == "InvocationCompleted"
            || s.event_type == "StepStarted"
            || s.event_type == "ContextStarted"
            || s.event_type == "ContextSucceeded"
            || s.event_type == "ContextFailed"
    };
    let normalize = |mut s: NodeJsEventSignature| -> NodeJsEventSignature {
        if s.event_type.starts_with("Execution") {
            s.name = None;
        }
        s
    };

    let actual: Vec<NodeJsEventSignature> = result
        .get_nodejs_history_events()
        .iter()
        .map(NodeJsEventSignature::from_event)
        .filter(|s| !skip(s))
        .map(normalize)
        .collect();

    let expected: Vec<NodeJsEventSignature> = load_nodejs_history_file(history_file_path)
        .iter()
        .map(NodeJsEventSignature::from_event)
        .filter(|s| !skip(s))
        .map(normalize)
        .collect();

    let mut expected_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut actual_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for sig in &expected {
        *expected_counts.entry(format!("{:?}", sig)).or_insert(0) += 1;
    }
    for sig in &actual {
        *actual_counts.entry(format!("{:?}", sig)).or_insert(0) += 1;
    }

    assert_eq!(
        actual_counts,
        expected_counts,
        "Event signature mismatch.\nActual ({} events): {:#?}\nExpected ({} events): {:#?}",
        actual.len(),
        actual,
        expected.len(),
        expected
    );
}

/// Macro to generate a simple cloud integration test.
/// These tests invoke the function with `{}` payload, assert success,
/// and verify event signatures against the history file.
macro_rules! cloud_test {
    ($name:ident) => {
        #[tokio::test]
        #[ignore]
        async fn $name() {
            let func = get_function_name(stringify!($name).trim_start_matches("cloud_"));
            let mut runner: CloudDurableTestRunner<serde_json::Value> =
                CloudDurableTestRunner::new(&func).await.unwrap();
            let result = runner.run(json!({})).await.unwrap();
            assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
            assert_cloud_event_signatures(
                &result,
                &format!(
                    "tests/history/{}.history.json",
                    stringify!($name).trim_start_matches("cloud_")
                ),
            );
        }
    };
}

/// Macro for cloud tests that are expected to fail at the handler level.
/// These functions return errors that cause the Lambda invocation itself to fail.
macro_rules! cloud_test_error {
    ($name:ident) => {
        #[tokio::test]
        #[ignore]
        async fn $name() {
            let func = get_function_name(stringify!($name).trim_start_matches("cloud_"));
            let mut runner: CloudDurableTestRunner<serde_json::Value> =
                CloudDurableTestRunner::new(&func).await.unwrap();
            let result = runner.run(json!({})).await;
            // These functions are expected to fail — either at invoke level or with Failed status
            match result {
                Ok(r) => assert_eq!(r.get_status(), ExecutionStatus::Failed),
                Err(_) => {} // Expected — handler-level errors cause invoke failure
            }
        }
    };
}

/// Macro for cloud tests that only verify success status (no event signature check).
/// Used for tests where cloud and local event models diverge (e.g., retry semantics).
macro_rules! cloud_test_status_only {
    ($name:ident) => {
        #[tokio::test]
        #[ignore]
        async fn $name() {
            let func = get_function_name(stringify!($name).trim_start_matches("cloud_"));
            let mut runner: CloudDurableTestRunner<serde_json::Value> =
                CloudDurableTestRunner::new(&func).await.unwrap();
            let result = runner.run(json!({})).await.unwrap();
            assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
        }
    };
}

// ============================================================================
// Hello World & Basic Operations
// ============================================================================
cloud_test!(cloud_hello_world);
cloud_test!(cloud_step_basic);
cloud_test!(cloud_step_named);
cloud_test!(cloud_step_with_config);
cloud_test!(cloud_step_retry_exponential_backoff);
cloud_test!(cloud_step_retry_with_filter);
cloud_test_error!(cloud_step_error_determinism);

// ============================================================================
// Wait Operations
// ============================================================================
cloud_test!(cloud_wait_basic);
cloud_test!(cloud_wait_named);
cloud_test!(cloud_wait_extended);
cloud_test!(cloud_multiple_waits);
cloud_test_status_only!(cloud_wait_for_condition_basic);

// ============================================================================
// Map Operations
// ============================================================================
cloud_test!(cloud_map_basic);
cloud_test!(cloud_map_empty_array);
cloud_test!(cloud_map_error_preservation);
cloud_test!(cloud_map_failure_tolerance);
cloud_test!(cloud_map_high_concurrency);
cloud_test!(cloud_map_large_scale);
cloud_test!(cloud_map_min_successful);

// ============================================================================
// Parallel Operations
// ============================================================================
cloud_test!(cloud_parallel_basic);
cloud_test!(cloud_parallel_empty_array);
cloud_test!(cloud_parallel_error_preservation);
cloud_test!(cloud_parallel_failure_threshold_exceeded);
cloud_test!(cloud_parallel_first_successful);
cloud_test!(cloud_parallel_heterogeneous);
cloud_test!(cloud_parallel_min_successful);
cloud_test!(cloud_parallel_tolerated_failure_count);
cloud_test!(cloud_parallel_tolerated_failure_pct);
cloud_test_error!(cloud_parallel_with_wait);

// ============================================================================
// Promise Combinators
// ============================================================================
cloud_test!(cloud_promise_all);
cloud_test_status_only!(cloud_promise_all_settled);
cloud_test_status_only!(cloud_promise_any);
cloud_test_status_only!(cloud_promise_race);
cloud_test!(cloud_race_with_timeout);
cloud_test!(cloud_all_with_wait);
cloud_test!(cloud_replay_behavior);

// ============================================================================
// Child Context Operations
// ============================================================================
cloud_test!(cloud_child_context_basic);
cloud_test!(cloud_child_context_nested);
cloud_test_error!(cloud_child_context_large_data);
cloud_test_error!(cloud_child_context_checkpoint_size_limit);

// ============================================================================
// Concurrency
// ============================================================================
cloud_test!(cloud_concurrent_operations);
cloud_test!(cloud_concurrent_wait);

// ============================================================================
// Advanced
// ============================================================================
cloud_test!(cloud_checkpointing_batched);
cloud_test!(cloud_checkpointing_eager);
cloud_test!(cloud_checkpointing_optimistic);
cloud_test!(cloud_custom_serialization);

/// large_payload requires a specific input with a `data` field.
#[tokio::test]
#[ignore]
async fn cloud_large_payload() {
    let func = get_function_name("large_payload");
    let mut runner: CloudDurableTestRunner<serde_json::Value> =
        CloudDurableTestRunner::new(&func).await.unwrap();
    let result = runner
        .run(json!({"data": "x".repeat(1000)}))
        .await
        .unwrap();
    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
}

// ============================================================================
// Error Handling (expected to fail)
// ============================================================================
cloud_test_error!(cloud_handler_error);
cloud_test_error!(cloud_retry_exhaustion);
cloud_test!(cloud_child_context_with_failing_step);
cloud_test!(cloud_context_validation_parent_in_child);
cloud_test!(cloud_context_validation_parent_in_step);
cloud_test!(cloud_context_validation_parent_in_wait_condition);
