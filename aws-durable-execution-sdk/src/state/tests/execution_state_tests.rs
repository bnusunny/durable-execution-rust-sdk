//! Tests for ExecutionState.

use std::sync::Arc;

use crate::client::{GetOperationsResponse, MockDurableServiceClient, SharedDurableServiceClient};
use crate::lambda::InitialExecutionState;
use crate::operation::{Operation, OperationStatus, OperationType, OperationUpdate};
use crate::state::{CheckpointBatcherConfig, ExecutionState};

fn create_mock_client() -> SharedDurableServiceClient {
    Arc::new(MockDurableServiceClient::new())
}

fn create_test_operation(id: &str, status: OperationStatus) -> Operation {
    let mut op = Operation::new(id, OperationType::Step);
    op.status = status;
    op
}

#[tokio::test]
async fn test_execution_state_new_empty() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert_eq!(state.durable_execution_arn(), "arn:test");
    assert_eq!(state.checkpoint_token().await, "token-123");
    assert!(state.is_new());
    assert!(!state.is_replay());
    assert_eq!(state.operation_count().await, 0);
    assert!(!state.has_more_operations().await);
}

#[tokio::test]
async fn test_execution_state_new_with_operations() {
    let client = create_mock_client();
    let ops = vec![
        create_test_operation("op-1", OperationStatus::Succeeded),
        create_test_operation("op-2", OperationStatus::Succeeded),
    ];
    let initial_state = InitialExecutionState::with_operations(ops);
    
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        initial_state,
        client,
    );

    assert!(state.is_replay());
    assert!(!state.is_new());
    assert_eq!(state.operation_count().await, 2);
}

#[tokio::test]
async fn test_get_checkpoint_result_exists() {
    let client = create_mock_client();
    let mut op = create_test_operation("op-1", OperationStatus::Succeeded);
    op.result = Some(r#"{"value": 42}"#.to_string());
    let initial_state = InitialExecutionState::with_operations(vec![op]);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    let result = state.get_checkpoint_result("op-1").await;
    assert!(result.is_existent());
    assert!(result.is_succeeded());
    assert_eq!(result.result(), Some(r#"{"value": 42}"#));
}

#[tokio::test]
async fn test_get_checkpoint_result_not_exists() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let result = state.get_checkpoint_result("non-existent").await;
    assert!(!result.is_existent());
}

#[tokio::test]
async fn test_track_replay_transitions_to_new() {
    let client = create_mock_client();
    let ops = vec![
        create_test_operation("op-1", OperationStatus::Succeeded),
        create_test_operation("op-2", OperationStatus::Succeeded),
    ];
    let initial_state = InitialExecutionState::with_operations(ops);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    assert!(state.is_replay());
    state.track_replay("op-1").await;
    assert!(state.is_replay());
    state.track_replay("op-2").await;
    assert!(state.is_new());
}

#[tokio::test]
async fn test_track_replay_with_pagination() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_get_operations_response(Ok(GetOperationsResponse {
                operations: vec![create_test_operation("op-3", OperationStatus::Succeeded)],
                next_marker: None,
            }))
    );
    
    let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
    let mut initial_state = InitialExecutionState::with_operations(ops);
    initial_state.next_marker = Some("marker-1".to_string());
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    state.track_replay("op-1").await;
    assert!(state.is_replay());
}

#[tokio::test]
async fn test_set_checkpoint_token() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert_eq!(state.checkpoint_token().await, "token-123");
    state.set_checkpoint_token("token-456").await;
    assert_eq!(state.checkpoint_token().await, "token-456");
}

#[tokio::test]
async fn test_add_operation() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert!(!state.has_operation("op-1").await);
    
    let op = create_test_operation("op-1", OperationStatus::Succeeded);
    state.add_operation(op).await;
    
    assert!(state.has_operation("op-1").await);
    assert_eq!(state.operation_count().await, 1);
}

#[tokio::test]
async fn test_update_operation() {
    let client = create_mock_client();
    let ops = vec![create_test_operation("op-1", OperationStatus::Started)];
    let initial_state = InitialExecutionState::with_operations(ops);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    let op = state.get_operation("op-1").await.unwrap();
    assert_eq!(op.status, OperationStatus::Started);

    state.update_operation("op-1", |op| {
        op.status = OperationStatus::Succeeded;
        op.result = Some("done".to_string());
    }).await;

    let op = state.get_operation("op-1").await.unwrap();
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert_eq!(op.result, Some("done".to_string()));
}

#[tokio::test]
async fn test_load_more_operations() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_get_operations_response(Ok(GetOperationsResponse {
                operations: vec![
                    create_test_operation("op-2", OperationStatus::Succeeded),
                    create_test_operation("op-3", OperationStatus::Succeeded),
                ],
                next_marker: None,
            }))
    );
    
    let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
    let mut initial_state = InitialExecutionState::with_operations(ops);
    initial_state.next_marker = Some("marker-1".to_string());
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    assert_eq!(state.operation_count().await, 1);
    assert!(state.has_more_operations().await);

    let loaded = state.load_more_operations().await.unwrap();
    assert!(loaded);
    
    assert_eq!(state.operation_count().await, 3);
    assert!(!state.has_more_operations().await);
}

#[tokio::test]
async fn test_load_more_operations_no_more() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let loaded = state.load_more_operations().await.unwrap();
    assert!(!loaded);
}

#[tokio::test]
async fn test_load_all_operations() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_get_operations_response(Ok(GetOperationsResponse {
                operations: vec![create_test_operation("op-2", OperationStatus::Succeeded)],
                next_marker: Some("marker-2".to_string()),
            }))
            .with_get_operations_response(Ok(GetOperationsResponse {
                operations: vec![create_test_operation("op-3", OperationStatus::Succeeded)],
                next_marker: None,
            }))
    );
    
    let ops = vec![create_test_operation("op-1", OperationStatus::Succeeded)];
    let mut initial_state = InitialExecutionState::with_operations(ops);
    initial_state.next_marker = Some("marker-1".to_string());
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);

    assert_eq!(state.operation_count().await, 1);
    state.load_all_operations().await.unwrap();
    
    assert_eq!(state.operation_count().await, 3);
    assert!(!state.has_more_operations().await);
}

#[tokio::test]
async fn test_mark_parent_done() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert!(!state.is_parent_done("parent-1").await);
    state.mark_parent_done("parent-1").await;
    assert!(state.is_parent_done("parent-1").await);
    assert!(!state.is_parent_done("parent-2").await);
}

#[tokio::test]
async fn test_is_orphaned() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert!(!state.is_orphaned(None).await);
    assert!(!state.is_orphaned(Some("parent-1")).await);

    state.mark_parent_done("parent-1").await;

    assert!(state.is_orphaned(Some("parent-1")).await);
    assert!(!state.is_orphaned(Some("parent-2")).await);
}

#[tokio::test]
async fn test_debug_impl() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let debug_str = format!("{:?}", state);
    assert!(debug_str.contains("ExecutionState"));
    assert!(debug_str.contains("arn:test"));
}

#[tokio::test]
async fn test_create_checkpoint_direct() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.create_checkpoint(update, true).await;
    
    assert!(result.is_ok());
    assert_eq!(state.checkpoint_token().await, "new-token");
    assert!(state.has_operation("op-1").await);
}

#[tokio::test]
async fn test_create_checkpoint_updates_local_cache_on_succeed() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "token-1".to_string(),
            }))
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "token-2".to_string(),
            }))
    );
    
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let update = OperationUpdate::start("op-1", OperationType::Step);
    state.create_checkpoint(update, true).await.unwrap();
    
    let op = state.get_operation("op-1").await.unwrap();
    assert_eq!(op.status, OperationStatus::Started);

    let update = OperationUpdate::succeed("op-1", OperationType::Step, Some(r#"{"result": "ok"}"#.to_string()));
    state.create_checkpoint(update, true).await.unwrap();
    
    let op = state.get_operation("op-1").await.unwrap();
    assert_eq!(op.status, OperationStatus::Succeeded);
    assert_eq!(op.result, Some(r#"{"result": "ok"}"#.to_string()));
}

#[tokio::test]
async fn test_create_checkpoint_rejects_orphaned_child() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    state.mark_parent_done("parent-1").await;

    let update = OperationUpdate::start("child-1", OperationType::Step)
        .with_parent_id("parent-1");
    let result = state.create_checkpoint(update, true).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::DurableError::OrphanedChild { operation_id, .. } => {
            assert_eq!(operation_id, "child-1");
        }
        _ => panic!("Expected OrphanedChild error"),
    }
}

#[tokio::test]
async fn test_with_batcher_creates_state_and_batcher() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let (state, mut batcher) = ExecutionState::with_batcher(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        CheckpointBatcherConfig {
            max_batch_time_ms: 10,
            ..Default::default()
        },
        100,
    );

    assert!(state.has_checkpoint_sender());
    assert_eq!(state.durable_execution_arn(), "arn:test");
    
    let batcher_handle = tokio::spawn(async move {
        batcher.run().await;
    });

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.create_checkpoint(update, true).await;
    
    drop(state);
    batcher_handle.await.unwrap();
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_checkpoint_sync_convenience_method() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.checkpoint_sync(update).await;
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_checkpoint_async_convenience_method() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.checkpoint_async(update).await;
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_shared_checkpoint_token() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    let shared_token = state.shared_checkpoint_token();
    assert_eq!(*shared_token.read().await, "token-123");
    
    {
        let mut guard = shared_token.write().await;
        *guard = "modified-token".to_string();
    }
    
    assert_eq!(state.checkpoint_token().await, "modified-token");
}

// Tests for ReplayChildren support (Requirements 10.5, 10.6)

#[tokio::test]
async fn test_load_child_operations_returns_children() {
    let client = create_mock_client();
    
    let mut parent_op = Operation::new("parent-ctx", OperationType::Context);
    parent_op.status = OperationStatus::Succeeded;
    
    let mut child1 = create_test_operation("child-1", OperationStatus::Succeeded);
    child1.parent_id = Some("parent-ctx".to_string());
    
    let mut child2 = create_test_operation("child-2", OperationStatus::Succeeded);
    child2.parent_id = Some("parent-ctx".to_string());
    
    let mut other_child = create_test_operation("other-child", OperationStatus::Succeeded);
    other_child.parent_id = Some("other-parent".to_string());
    
    let initial_state = InitialExecutionState::with_operations(vec![
        parent_op, child1, child2, other_child
    ]);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let children = state.load_child_operations("parent-ctx").await.unwrap();
    
    assert_eq!(children.len(), 2);
    let child_ids: Vec<&str> = children.iter().map(|c| c.operation_id.as_str()).collect();
    assert!(child_ids.contains(&"child-1"));
    assert!(child_ids.contains(&"child-2"));
}

#[tokio::test]
async fn test_load_child_operations_no_children() {
    let client = create_mock_client();
    
    let parent_op = Operation::new("parent-ctx", OperationType::Context);
    let initial_state = InitialExecutionState::with_operations(vec![parent_op]);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let children = state.load_child_operations("parent-ctx").await.unwrap();
    
    assert!(children.is_empty());
}

#[tokio::test]
async fn test_get_child_operations_returns_cached_children() {
    let client = create_mock_client();
    
    let mut parent_op = Operation::new("parent-ctx", OperationType::Context);
    parent_op.status = OperationStatus::Succeeded;
    
    let mut child1 = create_test_operation("child-1", OperationStatus::Succeeded);
    child1.parent_id = Some("parent-ctx".to_string());
    
    let mut child2 = create_test_operation("child-2", OperationStatus::Succeeded);
    child2.parent_id = Some("parent-ctx".to_string());
    
    let initial_state = InitialExecutionState::with_operations(vec![
        parent_op, child1, child2
    ]);
    
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    let children = state.get_child_operations("parent-ctx").await;
    
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_get_child_operations_empty_for_nonexistent_parent() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );
    
    let children = state.get_child_operations("nonexistent-parent").await;
    assert!(children.is_empty());
}

#[tokio::test]
async fn test_has_replay_children_true() {
    use crate::operation::ContextDetails;
    
    let client = create_mock_client();
    
    let mut ctx_op = Operation::new("ctx-with-replay", OperationType::Context);
    ctx_op.status = OperationStatus::Succeeded;
    ctx_op.context_details = Some(ContextDetails {
        result: None,
        replay_children: Some(true),
        error: None,
    });
    
    let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    assert!(state.has_replay_children("ctx-with-replay").await);
}

#[tokio::test]
async fn test_has_replay_children_false_when_not_set() {
    use crate::operation::ContextDetails;
    
    let client = create_mock_client();
    
    let mut ctx_op = Operation::new("ctx-no-replay", OperationType::Context);
    ctx_op.status = OperationStatus::Succeeded;
    ctx_op.context_details = Some(ContextDetails {
        result: None,
        replay_children: None,
        error: None,
    });
    
    let initial_state = InitialExecutionState::with_operations(vec![ctx_op]);
    let state = ExecutionState::new("arn:test", "token-123", initial_state, client);
    
    assert!(!state.has_replay_children("ctx-no-replay").await);
}

// Tests for CheckpointingMode (Requirements 24.1, 24.2, 24.3, 24.4)

#[tokio::test]
async fn test_checkpointing_mode_default_is_batched() {
    let client = create_mock_client();
    let state = ExecutionState::new(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
    );

    assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Batched);
    assert!(state.is_batched_checkpointing());
    assert!(!state.is_eager_checkpointing());
    assert!(!state.is_optimistic_checkpointing());
}

#[tokio::test]
async fn test_checkpointing_mode_eager() {
    let client = create_mock_client();
    let state = ExecutionState::with_checkpointing_mode(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        crate::config::CheckpointingMode::Eager,
    );

    assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Eager);
    assert!(state.is_eager_checkpointing());
    assert!(!state.is_batched_checkpointing());
    assert!(!state.is_optimistic_checkpointing());
    assert!(state.prioritizes_durability());
    assert!(!state.prioritizes_performance());
    assert!(!state.should_use_async_checkpoint());
}

#[tokio::test]
async fn test_checkpointing_mode_optimistic() {
    let client = create_mock_client();
    let state = ExecutionState::with_checkpointing_mode(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        crate::config::CheckpointingMode::Optimistic,
    );

    assert_eq!(state.checkpointing_mode(), crate::config::CheckpointingMode::Optimistic);
    assert!(state.is_optimistic_checkpointing());
    assert!(!state.is_eager_checkpointing());
    assert!(!state.is_batched_checkpointing());
    assert!(state.prioritizes_performance());
    assert!(!state.prioritizes_durability());
    assert!(state.should_use_async_checkpoint());
}

#[tokio::test]
async fn test_checkpointing_mode_batched_helpers() {
    let client = create_mock_client();
    let state = ExecutionState::with_checkpointing_mode(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        crate::config::CheckpointingMode::Batched,
    );

    assert!(!state.prioritizes_durability());
    assert!(!state.prioritizes_performance());
    assert!(state.should_use_async_checkpoint());
}

#[tokio::test]
async fn test_checkpoint_optimal_eager_mode() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let state = ExecutionState::with_checkpointing_mode(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        crate::config::CheckpointingMode::Eager,
    );

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.checkpoint_optimal(update, false).await;
    
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_eager_mode_bypasses_batcher() {
    use crate::client::CheckpointResponse;
    
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let (state, mut batcher) = ExecutionState::with_batcher_and_mode(
        "arn:test",
        "token-123",
        InitialExecutionState::new(),
        client,
        CheckpointBatcherConfig {
            max_batch_time_ms: 10,
            ..Default::default()
        },
        100,
        crate::config::CheckpointingMode::Eager,
    );

    let batcher_handle = tokio::spawn(async move {
        batcher.run().await;
    });

    let update = OperationUpdate::start("op-1", OperationType::Step);
    let result = state.create_checkpoint(update, false).await;
    
    drop(state);
    batcher_handle.await.unwrap();
    
    assert!(result.is_ok());
}
