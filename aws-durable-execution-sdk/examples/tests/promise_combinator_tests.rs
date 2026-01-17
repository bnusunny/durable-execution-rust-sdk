//! Tests for promise combinator examples using LocalDurableTestRunner.
//!
//! These tests verify that the promise combinator examples execute correctly and produce
//! the expected operation history. The tests cover:
//!
//! - promise_combinators/all: Wait for all operations to complete
//! - promise_combinators/all_settled: Wait for all operations to settle (success or failure)
//! - promise_combinators/any: Return the first successful operation
//! - promise_combinators/race: Return the first operation to complete (success or failure)
//!
//! These tests use `#[tokio::test(flavor = "current_thread")]` to ensure each test
//! has its own isolated tokio runtime for time control.

use aws_durable_execution_sdk::{
    all, all_settled, any, race, CompletionConfig, DurableContext, DurableError, MapConfig,
    OperationType,
};
use aws_durable_execution_sdk_examples::test_helper::{
    assert_event_signatures, assert_event_signatures_unordered,
};
use aws_durable_execution_sdk_testing::{
    ExecutionStatus, LocalDurableTestRunner, TestEnvironmentConfig,
};
use serde::{Deserialize, Serialize};

// ============================================================================
// Promise All Example Handlers
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AggregatedResult {
    pub results: Vec<i32>,
    pub sum: i32,
}

/// Handler using the `all!` macro - heterogeneous futures.
async fn promise_all_macro_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<AggregatedResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let results = all!(
        ctx,
        async move { ctx1.step(|_| Ok(10), None).await },
        async move { ctx2.step(|_| Ok(20), None).await },
        async move { ctx3.step(|_| Ok(30), None).await },
    )
    .await?;

    let sum: i32 = results.iter().sum();
    Ok(AggregatedResult { results, sum })
}

/// Handler using ctx.all() - homogeneous futures.
async fn promise_all_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<AggregatedResult, DurableError> {
    let values = vec![10, 20, 30];

    let futures: Vec<_> = values
        .into_iter()
        .map(|v| {
            let ctx = ctx.clone();
            async move { ctx.step(move |_| Ok(v), None).await }
        })
        .collect();

    let results = ctx.all(futures).await?;
    let sum: i32 = results.iter().sum();

    Ok(AggregatedResult { results, sum })
}

/// Handler using ctx.map() - alternative approach.
async fn promise_all_map_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<AggregatedResult, DurableError> {
    let values = vec![10, 20, 30];

    let batch_result = ctx
        .map(
            values,
            |child_ctx: DurableContext, value: i32, _index: usize| {
                Box::pin(async move { child_ctx.step(|_| Ok(value), None).await })
            },
            Some(MapConfig {
                max_concurrency: Some(3),
                ..Default::default()
            }),
        )
        .await?;

    let results = batch_result
        .get_results()?
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let sum: i32 = results.iter().sum();

    Ok(AggregatedResult { results, sum })
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_all_macro() {
    LocalDurableTestRunner::<serde_json::Value, AggregatedResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_all_macro_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let aggregated = result.get_result().unwrap();
    assert_eq!(aggregated.results.len(), 3, "Should have 3 results");
    assert!(aggregated.results.contains(&10));
    assert!(aggregated.results.contains(&20));
    assert!(aggregated.results.contains(&30));
    assert_eq!(aggregated.sum, 60, "Sum should be 60");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // The all! macro uses step operations directly (not child contexts)
    // so we should have step operations
    let step_ops: Vec<_> = operations
        .iter()
        .filter(|op| op.operation_type == OperationType::Step)
        .collect();
    assert!(
        !step_ops.is_empty(),
        "Should have step operations for all! macro"
    );

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_all_macro.history.json",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_all() {
    LocalDurableTestRunner::<serde_json::Value, AggregatedResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_all_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let aggregated = result.get_result().unwrap();
    assert_eq!(aggregated.results.len(), 3, "Should have 3 results");
    assert!(aggregated.results.contains(&10));
    assert!(aggregated.results.contains(&20));
    assert!(aggregated.results.contains(&30));
    assert_eq!(aggregated.sum, 60, "Sum should be 60");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(operations, "tests/history/promise_all.history.json");
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_all_with_map() {
    LocalDurableTestRunner::<serde_json::Value, AggregatedResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_all_map_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let aggregated = result.get_result().unwrap();
    assert_eq!(aggregated.results.len(), 3, "Should have 3 results");
    assert!(aggregated.results.contains(&10));
    assert!(aggregated.results.contains(&20));
    assert!(aggregated.results.contains(&30));
    assert_eq!(aggregated.sum, 60, "Sum should be 60");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_all_with_map.history.json",
    );
}

// ============================================================================
// Promise All Settled Example Handlers
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettledResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// Handler using the `all_settled!` macro.
async fn promise_all_settled_macro_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<SettledResult, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let batch = all_settled!(
        ctx,
        async move { ctx1.step(|_| Ok("op1 success".to_string()), None).await },
        async move {
            ctx2.step(|_| Err::<String, _>("op2 failed".into()), None)
                .await
        },
        async move { ctx3.step(|_| Ok("op3 success".to_string()), None).await },
    )
    .await?;

    Ok(SettledResult {
        total: batch.items.len(),
        succeeded: batch.success_count(),
        failed: batch.failure_count(),
    })
}

/// Handler using ctx.map() with all_completed.
async fn promise_all_settled_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<SettledResult, DurableError> {
    let operations: Vec<(String, bool)> = vec![
        ("op1".to_string(), true),  // will succeed
        ("op2".to_string(), false), // will fail
        ("op3".to_string(), true),  // will succeed
    ];

    let config = MapConfig {
        completion_config: CompletionConfig::all_completed(),
        ..Default::default()
    };

    let batch_result = ctx
        .map(
            operations,
            |child_ctx: DurableContext, (name, should_succeed): (String, bool), _index: usize| {
                Box::pin(async move {
                    if should_succeed {
                        child_ctx
                            .step(|_| Ok(format!("{} success", name)), None)
                            .await
                    } else {
                        child_ctx
                            .step(
                                |_| Err::<String, _>(format!("{} failed", name).into()),
                                None,
                            )
                            .await
                    }
                })
            },
            Some(config),
        )
        .await?;

    Ok(SettledResult {
        total: batch_result.items.len(),
        succeeded: batch_result.succeeded().len(),
        failed: batch_result.failed().len(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_all_settled_macro() {
    LocalDurableTestRunner::<serde_json::Value, SettledResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_all_settled_macro_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let settled = result.get_result().unwrap();
    assert_eq!(settled.total, 3, "Should have 3 total operations");
    assert_eq!(settled.succeeded, 2, "Should have 2 succeeded");
    assert_eq!(settled.failed, 1, "Should have 1 failed");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_all_settled_macro.history.json",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_all_settled() {
    LocalDurableTestRunner::<serde_json::Value, SettledResult>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_all_settled_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let settled = result.get_result().unwrap();
    assert_eq!(settled.total, 3, "Should have 3 total operations");
    assert_eq!(settled.succeeded, 2, "Should have 2 succeeded");
    assert_eq!(settled.failed, 1, "Should have 1 failed");

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (unordered because map can process in any order)
    assert_event_signatures_unordered(
        operations,
        "tests/history/promise_all_settled.history.json",
    );
}

// ============================================================================
// Promise Any Example Handlers
// ============================================================================

/// Handler using the `any!` macro.
async fn promise_any_macro_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let result = any!(
        ctx,
        async move {
            ctx1.step(|_| Err::<String, _>("primary unavailable".into()), None)
                .await
        },
        async move {
            ctx2.step(|_| Ok("data from secondary".to_string()), None)
                .await
        },
        async move {
            ctx3.step(|_| Ok("data from fallback".to_string()), None)
                .await
        },
    )
    .await?;

    Ok(result)
}

/// Handler using ctx.map() with first_successful.
async fn promise_any_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let operations: Vec<(String, bool)> = vec![
        ("op1".to_string(), false), // will fail
        ("op2".to_string(), true),  // will succeed
        ("op3".to_string(), true),  // will succeed
    ];

    let config = MapConfig {
        completion_config: CompletionConfig::first_successful(),
        max_concurrency: Some(3),
        ..Default::default()
    };

    let batch_result = ctx
        .map(
            operations,
            |child_ctx: DurableContext, (name, should_succeed): (String, bool), _index: usize| {
                Box::pin(async move {
                    if should_succeed {
                        child_ctx
                            .step(|_| Ok(format!("{} succeeded", name)), None)
                            .await
                    } else {
                        child_ctx
                            .step(
                                |_| Err::<String, _>(format!("{} failed", name).into()),
                                None,
                            )
                            .await
                    }
                })
            },
            Some(config),
        )
        .await?;

    let results = batch_result.get_results()?;
    Ok(results.first().cloned().cloned().unwrap_or_default())
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_any_macro() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_any_macro_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let any_result = result.get_result().unwrap();
    // Should get the first successful result (secondary or fallback)
    assert!(
        any_result == "data from secondary" || any_result == "data from fallback",
        "Should get a successful result, got: {}",
        any_result
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_any_macro.history.json",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_any() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_any_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let any_result = result.get_result().unwrap();
    // Should get the first successful result (op2 or op3)
    assert!(
        any_result == "op2 succeeded" || any_result == "op3 succeeded",
        "Should get a successful result, got: {}",
        any_result
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (unordered because concurrency can affect order)
    assert_event_signatures_unordered(
        operations,
        "tests/history/promise_any.history.json",
    );
}

// ============================================================================
// Promise Race Example Handlers
// ============================================================================

/// Handler using the `race!` macro.
async fn promise_race_macro_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let result = race!(
        ctx,
        async move {
            ctx1.step(|_| Ok("fast result".to_string()), None).await
        },
        async move {
            ctx2.step(|_| Ok("slow result".to_string()), None).await
        },
    )
    .await?;

    Ok(result)
}

/// Handler using ctx.map() with first_successful.
async fn promise_race_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let operations: Vec<String> =
        vec!["fast".to_string(), "medium".to_string(), "slow".to_string()];

    let config = MapConfig {
        completion_config: CompletionConfig::first_successful(),
        max_concurrency: Some(3),
        ..Default::default()
    };

    let batch_result = ctx
        .map(
            operations,
            |child_ctx: DurableContext, name: String, _index: usize| {
                Box::pin(async move {
                    child_ctx
                        .step(|_| Ok(format!("{} result", name)), None)
                        .await
                })
            },
            Some(config),
        )
        .await?;

    let results = batch_result.get_results()?;
    Ok(results.first().cloned().cloned().unwrap_or_default())
}

/// Handler demonstrating timeout pattern with race!
async fn promise_race_timeout_handler(
    _event: serde_json::Value,
    ctx: DurableContext,
) -> Result<String, DurableError> {
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();

    let result = race!(
        ctx,
        async move {
            ctx1.step(|_| Ok("operation completed".to_string()), None)
                .await
        },
        async move {
            ctx2.step(|_| Err::<String, _>("operation timed out".into()), None)
                .await
        },
    )
    .await;

    match result {
        Ok(data) => Ok(data),
        Err(e) => Err(e),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_race_macro() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_race_macro_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let race_result = result.get_result().unwrap();
    // Should get one of the results (whichever completes first)
    assert!(
        race_result == "fast result" || race_result == "slow result",
        "Should get a result, got: {}",
        race_result
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_race_macro.history.json",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_race() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_race_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    assert_eq!(
        result.get_status(),
        ExecutionStatus::Succeeded,
        "Execution should complete successfully"
    );

    let race_result = result.get_result().unwrap();
    // Should get one of the results
    assert!(
        race_result == "fast result"
            || race_result == "medium result"
            || race_result == "slow result",
        "Should get a result, got: {}",
        race_result
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures (unordered because concurrency can affect order)
    assert_event_signatures_unordered(
        operations,
        "tests/history/promise_race.history.json",
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_promise_race_timeout_pattern() {
    LocalDurableTestRunner::<serde_json::Value, String>::setup_test_environment(
        TestEnvironmentConfig {
            skip_time: true,
            checkpoint_delay: None,
        },
    )
    .await
    .unwrap();

    let mut runner = LocalDurableTestRunner::new(promise_race_timeout_handler);
    let result = runner.run(serde_json::json!({})).await.unwrap();

    // The result depends on which operation completes first
    // In this test, both operations complete immediately, so either could win
    let status = result.get_status();
    assert!(
        status == ExecutionStatus::Succeeded || status == ExecutionStatus::Failed,
        "Execution should complete (either success or failure)"
    );

    // Verify operations
    let operations = result.get_operations();
    assert!(!operations.is_empty(), "Should have operations");

    // Check event signatures
    assert_event_signatures(
        operations,
        "tests/history/promise_race_timeout.history.json",
    );
}
