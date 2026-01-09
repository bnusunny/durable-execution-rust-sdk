//! Tests for checkpoint batch ordering.

use crate::error::ErrorObject;
use crate::operation::{OperationAction, OperationType, OperationUpdate};
use crate::state::{CheckpointBatcher, CheckpointRequest};

fn create_start_update(id: &str, op_type: OperationType) -> OperationUpdate {
    OperationUpdate::start(id, op_type)
}

fn create_succeed_update(id: &str, op_type: OperationType) -> OperationUpdate {
    OperationUpdate::succeed(id, op_type, Some("result".to_string()))
}

fn create_fail_update(id: &str, op_type: OperationType) -> OperationUpdate {
    OperationUpdate::fail(id, op_type, ErrorObject::new("Error", "message"))
}

fn create_request(update: OperationUpdate) -> CheckpointRequest {
    CheckpointRequest::async_request(update)
}

#[test]
fn test_execution_completion_is_last() {
    let batch = vec![
        create_request(create_succeed_update("exec-1", OperationType::Execution)),
        create_request(create_start_update("step-1", OperationType::Step)),
        create_request(create_succeed_update("step-2", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 3);
    assert_eq!(sorted[2].operation.operation_id, "exec-1");
    assert_eq!(sorted[2].operation.action, OperationAction::Succeed);
    assert_eq!(sorted[2].operation.operation_type, OperationType::Execution);
}

#[test]
fn test_execution_fail_is_last() {
    let batch = vec![
        create_request(create_fail_update("exec-1", OperationType::Execution)),
        create_request(create_start_update("step-1", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[1].operation.operation_id, "exec-1");
    assert_eq!(sorted[1].operation.action, OperationAction::Fail);
}

#[test]
fn test_child_after_parent_context_start() {
    let mut child_update = create_start_update("child-1", OperationType::Step);
    child_update.parent_id = Some("parent-ctx".to_string());

    let batch = vec![
        create_request(child_update),
        create_request(create_start_update("parent-ctx", OperationType::Context)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].operation.operation_id, "parent-ctx");
    assert_eq!(sorted[0].operation.action, OperationAction::Start);
    assert_eq!(sorted[1].operation.operation_id, "child-1");
}

#[test]
fn test_start_and_completion_same_operation() {
    let batch = vec![
        create_request(create_succeed_update("step-1", OperationType::Step)),
        create_request(create_start_update("step-1", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].operation.operation_id, "step-1");
    assert_eq!(sorted[0].operation.action, OperationAction::Start);
    assert_eq!(sorted[1].operation.operation_id, "step-1");
    assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
}

#[test]
fn test_context_start_and_completion_same_batch() {
    let batch = vec![
        create_request(create_succeed_update("ctx-1", OperationType::Context)),
        create_request(create_start_update("ctx-1", OperationType::Context)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].operation.operation_id, "ctx-1");
    assert_eq!(sorted[0].operation.action, OperationAction::Start);
    assert_eq!(sorted[1].operation.operation_id, "ctx-1");
    assert_eq!(sorted[1].operation.action, OperationAction::Succeed);
}

#[test]
fn test_step_start_and_fail_same_batch() {
    let batch = vec![
        create_request(create_fail_update("step-1", OperationType::Step)),
        create_request(create_start_update("step-1", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 2);
    assert_eq!(sorted[0].operation.operation_id, "step-1");
    assert_eq!(sorted[0].operation.action, OperationAction::Start);
    assert_eq!(sorted[1].operation.operation_id, "step-1");
    assert_eq!(sorted[1].operation.action, OperationAction::Fail);
}

#[test]
fn test_complex_ordering_scenario() {
    let mut child1 = create_start_update("child-1", OperationType::Step);
    child1.parent_id = Some("parent-ctx".to_string());

    let mut child2 = create_succeed_update("child-2", OperationType::Step);
    child2.parent_id = Some("parent-ctx".to_string());

    let batch = vec![
        create_request(create_succeed_update("exec-1", OperationType::Execution)),
        create_request(child1),
        create_request(create_succeed_update("parent-ctx", OperationType::Context)),
        create_request(create_start_update("parent-ctx", OperationType::Context)),
        create_request(child2),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 5);

    let parent_start_pos = sorted.iter().position(|r| 
        r.operation.operation_id == "parent-ctx" && r.operation.action == OperationAction::Start
    ).unwrap();
    let parent_succeed_pos = sorted.iter().position(|r| 
        r.operation.operation_id == "parent-ctx" && r.operation.action == OperationAction::Succeed
    ).unwrap();
    let child1_pos = sorted.iter().position(|r| r.operation.operation_id == "child-1").unwrap();
    let child2_pos = sorted.iter().position(|r| r.operation.operation_id == "child-2").unwrap();
    let exec_pos = sorted.iter().position(|r| r.operation.operation_id == "exec-1").unwrap();

    assert!(parent_start_pos < parent_succeed_pos, "Parent START should be before parent SUCCEED");
    assert!(parent_start_pos < child1_pos, "Parent START should be before child-1");
    assert!(parent_start_pos < child2_pos, "Parent START should be before child-2");
    assert_eq!(exec_pos, 4, "EXECUTION completion should be last");
}

#[test]
fn test_empty_batch() {
    let batch: Vec<CheckpointRequest> = vec![];
    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
    assert!(sorted.is_empty());
}

#[test]
fn test_single_item_batch() {
    let batch = vec![create_request(create_start_update("step-1", OperationType::Step))];
    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);
    assert_eq!(sorted.len(), 1);
    assert_eq!(sorted[0].operation.operation_id, "step-1");
}

#[test]
fn test_preserves_original_order() {
    let batch = vec![
        create_request(create_start_update("step-1", OperationType::Step)),
        create_request(create_start_update("step-2", OperationType::Step)),
        create_request(create_start_update("step-3", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 3);
    assert_eq!(sorted[0].operation.operation_id, "step-1");
    assert_eq!(sorted[1].operation.operation_id, "step-2");
    assert_eq!(sorted[2].operation.operation_id, "step-3");
}

#[test]
fn test_multiple_children_same_parent() {
    let mut child1 = create_start_update("child-1", OperationType::Step);
    child1.parent_id = Some("parent-ctx".to_string());

    let mut child2 = create_start_update("child-2", OperationType::Step);
    child2.parent_id = Some("parent-ctx".to_string());

    let mut child3 = create_start_update("child-3", OperationType::Step);
    child3.parent_id = Some("parent-ctx".to_string());

    let batch = vec![
        create_request(child1),
        create_request(child2),
        create_request(create_start_update("parent-ctx", OperationType::Context)),
        create_request(child3),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted[0].operation.operation_id, "parent-ctx");
    
    for i in 1..4 {
        assert!(sorted[i].operation.parent_id.as_deref() == Some("parent-ctx"));
    }
}

#[test]
fn test_nested_contexts() {
    let mut parent_ctx = create_start_update("parent-ctx", OperationType::Context);
    parent_ctx.parent_id = Some("grandparent-ctx".to_string());

    let mut child = create_start_update("child-1", OperationType::Step);
    child.parent_id = Some("parent-ctx".to_string());

    let batch = vec![
        create_request(child),
        create_request(parent_ctx),
        create_request(create_start_update("grandparent-ctx", OperationType::Context)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    let grandparent_pos = sorted.iter().position(|r| r.operation.operation_id == "grandparent-ctx").unwrap();
    let parent_pos = sorted.iter().position(|r| r.operation.operation_id == "parent-ctx").unwrap();
    let child_pos = sorted.iter().position(|r| r.operation.operation_id == "child-1").unwrap();

    assert!(grandparent_pos < parent_pos, "Grandparent should be before parent");
    assert!(parent_pos < child_pos, "Parent should be before child");
}

#[test]
fn test_execution_start_not_affected() {
    let batch = vec![
        create_request(create_start_update("step-1", OperationType::Step)),
        create_request(create_start_update("exec-1", OperationType::Execution)),
        create_request(create_start_update("step-2", OperationType::Step)),
    ];

    let sorted = CheckpointBatcher::sort_checkpoint_batch(batch);

    assert_eq!(sorted.len(), 3);
    assert_eq!(sorted[0].operation.operation_id, "step-1");
    assert_eq!(sorted[1].operation.operation_id, "exec-1");
    assert_eq!(sorted[2].operation.operation_id, "step-2");
}
