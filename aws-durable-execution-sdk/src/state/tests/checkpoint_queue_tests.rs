//! Tests for checkpoint queue and sender.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::client::{CheckpointResponse, MockDurableServiceClient};
use crate::error::DurableError;
use crate::operation::{OperationType, OperationUpdate};
use crate::state::{
    CheckpointBatcher, CheckpointBatcherConfig, CheckpointRequest, create_checkpoint_queue,
};

fn create_test_update(id: &str) -> OperationUpdate {
    OperationUpdate::start(id, OperationType::Step)
}

#[test]
fn test_checkpoint_request_sync() {
    let update = create_test_update("op-1");
    let (request, _rx) = CheckpointRequest::sync(update);
    
    assert!(request.is_sync());
    assert_eq!(request.operation.operation_id, "op-1");
}

#[test]
fn test_checkpoint_request_async() {
    let update = create_test_update("op-1");
    let request = CheckpointRequest::async_request(update);
    
    assert!(!request.is_sync());
    assert_eq!(request.operation.operation_id, "op-1");
}

#[test]
fn test_checkpoint_request_estimated_size() {
    let update = create_test_update("op-1");
    let request = CheckpointRequest::async_request(update);
    
    let size = request.estimated_size();
    assert!(size > 0);
    assert!(size >= 100);
}

#[test]
fn test_checkpoint_request_estimated_size_with_result() {
    let mut update = create_test_update("op-1");
    update.result = Some("a".repeat(1000));
    let request = CheckpointRequest::async_request(update);
    
    let size = request.estimated_size();
    assert!(size >= 1100);
}

#[test]
fn test_create_checkpoint_queue() {
    let (sender, _rx) = create_checkpoint_queue(100);
    drop(sender);
}

#[tokio::test]
async fn test_checkpoint_sender_sync() {
    let (sender, mut rx) = create_checkpoint_queue(10);
    
    let handle = tokio::spawn(async move {
        if let Some(request) = rx.recv().await {
            assert!(request.is_sync());
            if let Some(completion) = request.completion {
                let _ = completion.send(Ok(()));
            }
        }
    });

    let update = create_test_update("op-1");
    let result = sender.checkpoint_sync(update).await;
    assert!(result.is_ok());
    
    handle.await.unwrap();
}

#[tokio::test]
async fn test_checkpoint_sender_async() {
    let (sender, mut rx) = create_checkpoint_queue(10);
    
    let update = create_test_update("op-1");
    let result = sender.checkpoint_async(update).await;
    assert!(result.is_ok());
    
    let request = rx.recv().await.unwrap();
    assert!(!request.is_sync());
    assert_eq!(request.operation.operation_id, "op-1");
}

#[tokio::test]
async fn test_checkpoint_sender_queue_closed() {
    let (sender, rx) = create_checkpoint_queue(10);
    drop(rx);
    
    let update = create_test_update("op-1");
    let result = sender.checkpoint_async(update).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_checkpoint_batcher_processes_batch() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let (sender, rx) = create_checkpoint_queue(10);
    let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
    
    let config = CheckpointBatcherConfig {
        max_batch_time_ms: 10,
        ..Default::default()
    };
    
    let mut batcher = CheckpointBatcher::new(
        config,
        rx,
        client,
        "arn:test".to_string(),
        checkpoint_token.clone(),
    );
    
    let update = create_test_update("op-1");
    let (request, completion_rx) = CheckpointRequest::sync(update);
    sender.tx.send(request).await.unwrap();
    
    drop(sender);
    batcher.run().await;
    
    let result = completion_rx.await.unwrap();
    assert!(result.is_ok());
    
    let token = checkpoint_token.read().await;
    assert_eq!(*token, "new-token");
}

#[tokio::test]
async fn test_checkpoint_batcher_handles_error() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Test error")))
    );
    
    let (sender, rx) = create_checkpoint_queue(10);
    let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
    
    let config = CheckpointBatcherConfig {
        max_batch_time_ms: 10,
        ..Default::default()
    };
    
    let mut batcher = CheckpointBatcher::new(
        config,
        rx,
        client,
        "arn:test".to_string(),
        checkpoint_token.clone(),
    );
    
    let update = create_test_update("op-1");
    let (request, completion_rx) = CheckpointRequest::sync(update);
    sender.tx.send(request).await.unwrap();
    
    drop(sender);
    batcher.run().await;
    
    let result = completion_rx.await.unwrap();
    assert!(result.is_err());
    
    let token = checkpoint_token.read().await;
    assert_eq!(*token, "initial-token");
}

#[tokio::test]
async fn test_checkpoint_batcher_batches_multiple_requests() {
    let client = Arc::new(
        MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse {
                checkpoint_token: "new-token".to_string(),
            }))
    );
    
    let (sender, rx) = create_checkpoint_queue(10);
    let checkpoint_token = Arc::new(RwLock::new("initial-token".to_string()));
    
    let config = CheckpointBatcherConfig {
        max_batch_time_ms: 50,
        max_batch_operations: 3,
        ..Default::default()
    };
    
    let mut batcher = CheckpointBatcher::new(
        config,
        rx,
        client,
        "arn:test".to_string(),
        checkpoint_token.clone(),
    );
    
    for i in 0..3 {
        let update = create_test_update(&format!("op-{}", i));
        sender.tx.send(CheckpointRequest::async_request(update)).await.unwrap();
    }
    
    drop(sender);
    batcher.run().await;
    
    let token = checkpoint_token.read().await;
    assert_eq!(*token, "new-token");
}
