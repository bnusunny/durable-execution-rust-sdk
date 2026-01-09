//! Property-based tests for orphan prevention.

use std::sync::Arc;

use proptest::prelude::*;

use crate::client::{CheckpointResponse, MockDurableServiceClient};
use crate::error::DurableError;
use crate::lambda::InitialExecutionState;
use crate::operation::{OperationType, OperationUpdate};
use crate::state::ExecutionState;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_orphaned_child_checkpoint_fails(
        parent_id in "[a-z]{5,10}",
        child_id in "[a-z]{5,10}",
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
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

            state.mark_parent_done(&parent_id).await;

            let update = OperationUpdate::start(&child_id, OperationType::Step)
                .with_parent_id(&parent_id);
            let checkpoint_result = state.create_checkpoint(update, true).await;
            
            match checkpoint_result {
                Err(DurableError::OrphanedChild { operation_id, .. }) => {
                    if operation_id != child_id {
                        return Err(TestCaseError::fail(format!(
                            "Expected operation_id '{}' in OrphanedChild error, got '{}'",
                            child_id, operation_id
                        )));
                    }
                }
                Ok(_) => {
                    return Err(TestCaseError::fail(
                        "Expected OrphanedChild error, but checkpoint succeeded"
                    ));
                }
                Err(other) => {
                    return Err(TestCaseError::fail(format!(
                        "Expected OrphanedChild error, got {:?}",
                        other
                    )));
                }
            }
            
            Ok(())
        });
        result?;
    }

    #[test]
    fn prop_non_orphaned_child_checkpoint_succeeds(
        parent_id in "[a-z]{5,10}",
        child_id in "[a-z]{5,10}",
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
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

            let update = OperationUpdate::start(&child_id, OperationType::Step)
                .with_parent_id(&parent_id);
            let checkpoint_result = state.create_checkpoint(update, true).await;
            
            if let Err(e) = checkpoint_result {
                return Err(TestCaseError::fail(format!(
                    "Expected checkpoint to succeed for non-orphaned child, got error: {:?}",
                    e
                )));
            }
            
            Ok(())
        });
        result?;
    }

    #[test]
    fn prop_root_operation_never_orphaned(
        operation_id in "[a-z]{5,10}",
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
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

            let update = OperationUpdate::start(&operation_id, OperationType::Step);
            let checkpoint_result = state.create_checkpoint(update, true).await;
            
            if let Err(e) = checkpoint_result {
                return Err(TestCaseError::fail(format!(
                    "Expected checkpoint to succeed for root operation, got error: {:?}",
                    e
                )));
            }
            
            Ok(())
        });
        result?;
    }

    #[test]
    fn prop_marking_parent_done_affects_future_checkpoints(
        parent_id in "[a-z]{5,10}",
        child_id_before in "[a-z]{5,10}",
        child_id_after in "[a-z]{5,10}",
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
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

            let update_before = OperationUpdate::start(&child_id_before, OperationType::Step)
                .with_parent_id(&parent_id);
            let result_before = state.create_checkpoint(update_before, true).await;
            
            if let Err(e) = result_before {
                return Err(TestCaseError::fail(format!(
                    "Expected first checkpoint to succeed, got error: {:?}",
                    e
                )));
            }

            state.mark_parent_done(&parent_id).await;

            let update_after = OperationUpdate::start(&child_id_after, OperationType::Step)
                .with_parent_id(&parent_id);
            let result_after = state.create_checkpoint(update_after, true).await;
            
            match result_after {
                Err(DurableError::OrphanedChild { .. }) => {
                    // Expected
                }
                Ok(_) => {
                    return Err(TestCaseError::fail(
                        "Expected OrphanedChild error after marking parent done"
                    ));
                }
                Err(other) => {
                    return Err(TestCaseError::fail(format!(
                        "Expected OrphanedChild error, got {:?}",
                        other
                    )));
                }
            }
            
            Ok(())
        });
        result?;
    }
}
