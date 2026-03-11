//! Cloud integration tests using CloudDurableTestRunner.
//!
//! These tests deploy example functions to AWS Lambda and verify execution
//! results and event history against the same .history.json files used by
//! local tests, matching the JS SDK's testing pattern.
//!
//! Run with: cargo test --package durable-execution-sdk-examples --test cloud_integration_tests -- --ignored

use durable_execution_sdk_examples::test_helper::assert_nodejs_event_signatures_unordered;
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

// ============================================================================
// Hello World
// ============================================================================

#[tokio::test]
#[ignore]
async fn cloud_hello_world() {
    let func = get_function_name("hello_world");
    let mut runner: CloudDurableTestRunner<String> =
        CloudDurableTestRunner::new(&func).await.unwrap();

    let result = runner.run(json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "Hello World!");

    assert_nodejs_event_signatures_unordered(&result, "tests/history/hello_world.history.json");
}

// ============================================================================
// Step Basic
// ============================================================================

#[tokio::test]
#[ignore]
async fn cloud_step_basic() {
    let func = get_function_name("step_basic");
    let mut runner: CloudDurableTestRunner<String> =
        CloudDurableTestRunner::new(&func).await.unwrap();

    let result = runner.run(json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "step completed");

    assert_nodejs_event_signatures_unordered(&result, "tests/history/step_basic.history.json");
}

// ============================================================================
// Step Named
// ============================================================================

#[tokio::test]
#[ignore]
async fn cloud_step_named() {
    let func = get_function_name("step_named");
    let mut runner: CloudDurableTestRunner<serde_json::Value> =
        CloudDurableTestRunner::new(&func).await.unwrap();

    let result = runner.run(json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    assert_nodejs_event_signatures_unordered(&result, "tests/history/step_named.history.json");
}

// ============================================================================
// Wait Basic
// ============================================================================

#[tokio::test]
#[ignore]
async fn cloud_wait_basic() {
    let func = get_function_name("wait_basic");
    let mut runner: CloudDurableTestRunner<String> =
        CloudDurableTestRunner::new(&func).await.unwrap();

    let result = runner.run(json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
    assert_eq!(result.get_result().unwrap(), "waited successfully");

    assert_nodejs_event_signatures_unordered(&result, "tests/history/wait_basic.history.json");
}

// ============================================================================
// Map Basic
// ============================================================================

#[tokio::test]
#[ignore]
async fn cloud_map_basic() {
    let func = get_function_name("map_basic");
    let mut runner: CloudDurableTestRunner<serde_json::Value> =
        CloudDurableTestRunner::new(&func).await.unwrap();

    let result = runner.run(json!({})).await.unwrap();

    assert_eq!(result.get_status(), ExecutionStatus::Succeeded);

    assert_nodejs_event_signatures_unordered(&result, "tests/history/map_basic.history.json");
}
