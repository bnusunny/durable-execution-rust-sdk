//! Property-based tests for checkpoint batching.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use proptest::prelude::*;
use tokio::sync::RwLock;

use crate::client::{CheckpointResponse, DurableServiceClient, GetOperationsResponse};
use crate::error::DurableError;
use crate::operation::{OperationType, OperationUpdate};
use crate::state::{
    CheckpointBatcher, CheckpointBatcherConfig, CheckpointRequest, create_checkpoint_queue,
};

/// Mock client that counts the number of checkpoint calls
struct CountingMockClient {
    checkpoint_count: Arc<AtomicUsize>,
}

impl CountingMockClient {
    fn new() -> (Self, Arc<AtomicUsize>) {
        let count = Arc::new(AtomicUsize::new(0));
        (Self { checkpoint_count: count.clone() }, count)
    }
}

#[async_trait]
impl DurableServiceClient for CountingMockClient {
    async fn checkpoint(
        &self,
        _durable_execution_arn: &str,
        _checkpoint_token: &str,
        _operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        self.checkpoint_count.fetch_add(1, Ordering::SeqCst);
        Ok(CheckpointResponse {
            checkpoint_token: format!("token-{}", self.checkpoint_count.load(Ordering::SeqCst)),
        })
    }

    async fn get_operations(
        &self,
        _durable_execution_arn: &str,
        _next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        Ok(GetOperationsResponse {
            operations: vec![],
            next_marker: None,
        })
    }
}

fn create_test_update_with_size(id: &str, result_size: usize) -> OperationUpdate {
    let mut update = OperationUpdate::start(id, OperationType::Step);
    if result_size > 0 {
        update.result = Some("x".repeat(result_size));
    }
    update
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_checkpoint_batching_respects_operation_count_limit(
        num_requests in 1usize..20,
        max_ops_per_batch in 1usize..10,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
            let (client, call_count) = CountingMockClient::new();
            let client = Arc::new(client);
            
            let (sender, rx) = create_checkpoint_queue(100);
            let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
            
            let config = CheckpointBatcherConfig {
                max_batch_time_ms: 10,
                max_batch_operations: max_ops_per_batch,
                max_batch_size_bytes: usize::MAX,
            };
            
            let mut batcher = CheckpointBatcher::new(
                config,
                rx,
                client,
                "arn:test".to_string(),
                checkpoint_token.clone(),
            );
            
            for i in 0..num_requests {
                let update = create_test_update_with_size(&format!("op-{}", i), 0);
                sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
            }
            
            drop(sender);
            batcher.run().await;
            
            let expected_max_calls = (num_requests + max_ops_per_batch - 1) / max_ops_per_batch;
            let actual_calls = call_count.load(Ordering::SeqCst);
            
            if actual_calls > expected_max_calls {
                return Err(TestCaseError::fail(format!(
                    "Expected at most {} API calls for {} requests with batch size {}, got {}",
                    expected_max_calls, num_requests, max_ops_per_batch, actual_calls
                )));
            }
            
            if actual_calls < 1 {
                return Err(TestCaseError::fail(format!(
                    "Expected at least 1 API call for {} requests, got {}",
                    num_requests, actual_calls
                )));
            }
            
            Ok(())
        });
        result?;
    }

    #[test]
    fn prop_checkpoint_batching_respects_size_limit(
        num_requests in 1usize..10,
        result_size in 100usize..500,
        max_batch_size in 500usize..2000,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
            let (client, call_count) = CountingMockClient::new();
            let client = Arc::new(client);
            
            let (sender, rx) = create_checkpoint_queue(100);
            let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
            
            let config = CheckpointBatcherConfig {
                max_batch_time_ms: 10,
                max_batch_operations: usize::MAX,
                max_batch_size_bytes: max_batch_size,
            };
            
            let mut batcher = CheckpointBatcher::new(
                config,
                rx,
                client,
                "arn:test".to_string(),
                checkpoint_token.clone(),
            );
            
            for i in 0..num_requests {
                let update = create_test_update_with_size(&format!("op-{}", i), result_size);
                sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
            }
            
            drop(sender);
            batcher.run().await;
            
            let estimated_request_size = 100 + result_size;
            let total_size = num_requests * estimated_request_size;
            let expected_max_calls = (total_size + max_batch_size - 1) / max_batch_size;
            let actual_calls = call_count.load(Ordering::SeqCst);
            
            if actual_calls > expected_max_calls.max(1) * 2 {
                return Err(TestCaseError::fail(format!(
                    "Expected at most ~{} API calls for {} requests of size {}, got {}",
                    expected_max_calls, num_requests, estimated_request_size, actual_calls
                )));
            }
            
            if actual_calls < 1 {
                return Err(TestCaseError::fail(format!(
                    "Expected at least 1 API call, got {}",
                    actual_calls
                )));
            }
            
            Ok(())
        });
        result?;
    }

    #[test]
    fn prop_all_requests_are_processed(
        num_requests in 1usize..20,
    ) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result: Result<(), TestCaseError> = rt.block_on(async {
            let (client, _call_count) = CountingMockClient::new();
            let client = Arc::new(client);
            
            let (sender, rx) = create_checkpoint_queue(100);
            let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
            
            let config = CheckpointBatcherConfig {
                max_batch_time_ms: 10,
                max_batch_operations: 5,
                ..Default::default()
            };
            
            let mut batcher = CheckpointBatcher::new(
                config,
                rx,
                client,
                "arn:test".to_string(),
                checkpoint_token.clone(),
            );
            
            let mut receivers = Vec::new();
            
            for i in 0..num_requests {
                let update = create_test_update_with_size(&format!("op-{}", i), 0);
                let (request, rx) = CheckpointRequest::sync(update);
                sender.tx.send(request).await.unwrap();
                receivers.push(rx);
            }
            
            drop(sender);
            batcher.run().await;
            
            let mut success_count = 0;
            for rx in receivers {
                if let Ok(result) = rx.await {
                    if result.is_ok() {
                        success_count += 1;
                    }
                }
            }
            
            if success_count != num_requests {
                return Err(TestCaseError::fail(format!(
                    "Expected all {} requests to succeed, got {}",
                    num_requests, success_count
                )));
            }
            
            Ok(())
        });
        result?;
    }
}
