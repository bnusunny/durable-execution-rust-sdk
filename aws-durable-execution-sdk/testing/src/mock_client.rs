//! Mock implementation of DurableServiceClient for testing.
//!
//! This module provides a mock checkpoint client that can be used for unit testing
//! durable functions without requiring AWS infrastructure.
//!
//! # Examples
//!
//! ```
//! use aws_durable_execution_sdk_testing::MockDurableServiceClient;
//! use aws_durable_execution_sdk::{CheckpointResponse, Operation, OperationType};
//!
//! // Create a mock client with default responses
//! let client = MockDurableServiceClient::new();
//!
//! // Create a mock client with custom checkpoint responses
//! let client = MockDurableServiceClient::new()
//!     .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
//!     .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")));
//!
//! // Create a mock client with multiple default responses
//! let client = MockDurableServiceClient::new()
//!     .with_checkpoint_responses(5);
//! ```

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::{
    CheckpointResponse, DurableError, DurableServiceClient, GetOperationsResponse, Operation,
    OperationUpdate,
};

/// Record of a checkpoint call made to the mock client.
///
/// This struct captures all the parameters passed to a checkpoint call,
/// allowing tests to verify the correct operations were checkpointed.
///
/// # Examples
///
/// ```
/// use aws_durable_execution_sdk_testing::{MockDurableServiceClient, DurableServiceClient};
/// use aws_durable_execution_sdk::{OperationUpdate, OperationType};
///
/// # tokio_test::block_on(async {
/// let client = MockDurableServiceClient::new();
///
/// // Make a checkpoint call
/// client.checkpoint(
///     "arn:aws:lambda:us-east-1:123456789012:function:test",
///     "token-123",
///     vec![OperationUpdate::start("op-1", OperationType::Step)],
/// ).await.unwrap();
///
/// // Verify the call was recorded
/// let calls = client.get_checkpoint_calls();
/// assert_eq!(calls.len(), 1);
/// assert_eq!(calls[0].checkpoint_token, "token-123");
/// assert_eq!(calls[0].operations.len(), 1);
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct CheckpointCall {
    /// The durable execution ARN passed to the checkpoint call
    pub durable_execution_arn: String,
    /// The checkpoint token passed to the checkpoint call
    pub checkpoint_token: String,
    /// The operations passed to the checkpoint call
    pub operations: Vec<OperationUpdate>,
}

impl CheckpointCall {
    /// Creates a new CheckpointCall record.
    pub fn new(
        durable_execution_arn: impl Into<String>,
        checkpoint_token: impl Into<String>,
        operations: Vec<OperationUpdate>,
    ) -> Self {
        Self {
            durable_execution_arn: durable_execution_arn.into(),
            checkpoint_token: checkpoint_token.into(),
            operations,
        }
    }
}

/// Record of a get_operations call made to the mock client.
///
/// This struct captures all the parameters passed to a get_operations call,
/// allowing tests to verify the correct pagination was requested.
#[derive(Debug, Clone)]
pub struct GetOperationsCall {
    /// The durable execution ARN passed to the get_operations call
    pub durable_execution_arn: String,
    /// The next marker passed to the get_operations call
    pub next_marker: String,
}

impl GetOperationsCall {
    /// Creates a new GetOperationsCall record.
    pub fn new(durable_execution_arn: impl Into<String>, next_marker: impl Into<String>) -> Self {
        Self {
            durable_execution_arn: durable_execution_arn.into(),
            next_marker: next_marker.into(),
        }
    }
}

/// Mock implementation of DurableServiceClient for testing.
///
/// This mock client allows you to:
/// - Configure responses for checkpoint and get_operations calls
/// - Record all calls made for verification in tests
/// - Simulate error conditions
///
/// # Thread Safety
///
/// The mock client uses internal mutexes to allow safe concurrent access
/// from multiple tasks.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```
/// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
///
/// let client = MockDurableServiceClient::new();
/// ```
///
/// ## With Custom Responses
///
/// ```
/// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
/// use aws_durable_execution_sdk::{CheckpointResponse, DurableError};
///
/// let client = MockDurableServiceClient::new()
///     .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
///     .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Temporary error")));
/// ```
///
/// ## Verifying Calls
///
/// ```
/// use aws_durable_execution_sdk_testing::{MockDurableServiceClient, DurableServiceClient};
/// use aws_durable_execution_sdk::{OperationUpdate, OperationType};
///
/// # tokio_test::block_on(async {
/// let client = MockDurableServiceClient::new();
///
/// client.checkpoint("arn:test", "token", vec![]).await.unwrap();
///
/// let calls = client.get_checkpoint_calls();
/// assert_eq!(calls.len(), 1);
/// # });
/// ```
pub struct MockDurableServiceClient {
    /// Queue of checkpoint responses to return
    checkpoint_responses: Mutex<VecDeque<Result<CheckpointResponse, DurableError>>>,
    /// Queue of get_operations responses to return
    get_operations_responses: Mutex<VecDeque<Result<GetOperationsResponse, DurableError>>>,
    /// Record of all checkpoint calls made
    checkpoint_calls: Mutex<Vec<CheckpointCall>>,
    /// Record of all get_operations calls made
    get_operations_calls: Mutex<Vec<GetOperationsCall>>,
}

impl MockDurableServiceClient {
    /// Creates a new mock client with no pre-configured responses.
    ///
    /// When no responses are configured, the client will return default
    /// successful responses.
    pub fn new() -> Self {
        Self {
            checkpoint_responses: Mutex::new(VecDeque::new()),
            get_operations_responses: Mutex::new(VecDeque::new()),
            checkpoint_calls: Mutex::new(Vec::new()),
            get_operations_calls: Mutex::new(Vec::new()),
        }
    }

    /// Adds a checkpoint response to be returned.
    ///
    /// Responses are returned in the order they were added (FIFO).
    /// Once all configured responses are consumed, the client returns
    /// default successful responses.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
    /// use aws_durable_execution_sdk::CheckpointResponse;
    ///
    /// let client = MockDurableServiceClient::new()
    ///     .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")));
    /// ```
    pub fn with_checkpoint_response(
        self,
        response: Result<CheckpointResponse, DurableError>,
    ) -> Self {
        self.checkpoint_responses
            .lock()
            .unwrap()
            .push_back(response);
        self
    }

    /// Adds multiple default checkpoint responses.
    ///
    /// Each response will have a unique token in the format "token-{index}".
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
    ///
    /// // Add 5 default checkpoint responses
    /// let client = MockDurableServiceClient::new()
    ///     .with_checkpoint_responses(5);
    /// ```
    pub fn with_checkpoint_responses(self, count: usize) -> Self {
        let mut responses = self.checkpoint_responses.lock().unwrap();
        for i in 0..count {
            responses.push_back(Ok(CheckpointResponse::new(format!("token-{}", i))));
        }
        drop(responses);
        self
    }

    /// Adds a checkpoint response that includes operations in the new execution state.
    ///
    /// This is useful for testing scenarios where the checkpoint response
    /// includes service-generated values like callback IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
    /// use aws_durable_execution_sdk::{Operation, OperationType};
    ///
    /// let op = Operation::new("op-1", OperationType::Callback);
    /// let client = MockDurableServiceClient::new()
    ///     .with_checkpoint_response_with_operations("token-1", vec![op]);
    /// ```
    pub fn with_checkpoint_response_with_operations(
        self,
        token: impl Into<String>,
        operations: Vec<Operation>,
    ) -> Self {
        use aws_durable_execution_sdk::client::NewExecutionState;

        let response = CheckpointResponse {
            checkpoint_token: token.into(),
            new_execution_state: Some(NewExecutionState {
                operations,
                next_marker: None,
            }),
        };
        self.checkpoint_responses
            .lock()
            .unwrap()
            .push_back(Ok(response));
        self
    }

    /// Adds a get_operations response to be returned.
    ///
    /// Responses are returned in the order they were added (FIFO).
    /// Once all configured responses are consumed, the client returns
    /// default empty responses.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_durable_execution_sdk_testing::MockDurableServiceClient;
    /// use aws_durable_execution_sdk::{GetOperationsResponse, Operation, OperationType};
    ///
    /// let client = MockDurableServiceClient::new()
    ///     .with_get_operations_response(Ok(GetOperationsResponse {
    ///         operations: vec![Operation::new("op-1", OperationType::Step)],
    ///         next_marker: None,
    ///     }));
    /// ```
    pub fn with_get_operations_response(
        self,
        response: Result<GetOperationsResponse, DurableError>,
    ) -> Self {
        self.get_operations_responses
            .lock()
            .unwrap()
            .push_back(response);
        self
    }

    /// Gets all checkpoint calls made to this mock client.
    ///
    /// Returns a clone of the recorded calls, allowing verification
    /// without consuming the records.
    ///
    /// # Examples
    ///
    /// ```
    /// use aws_durable_execution_sdk_testing::{MockDurableServiceClient, DurableServiceClient};
    ///
    /// # tokio_test::block_on(async {
    /// let client = MockDurableServiceClient::new();
    /// client.checkpoint("arn:test", "token", vec![]).await.unwrap();
    ///
    /// let calls = client.get_checkpoint_calls();
    /// assert_eq!(calls.len(), 1);
    /// assert_eq!(calls[0].durable_execution_arn, "arn:test");
    /// # });
    /// ```
    pub fn get_checkpoint_calls(&self) -> Vec<CheckpointCall> {
        self.checkpoint_calls.lock().unwrap().clone()
    }

    /// Gets all get_operations calls made to this mock client.
    ///
    /// Returns a clone of the recorded calls, allowing verification
    /// without consuming the records.
    pub fn get_get_operations_calls(&self) -> Vec<GetOperationsCall> {
        self.get_operations_calls.lock().unwrap().clone()
    }

    /// Clears all recorded checkpoint calls.
    ///
    /// This is useful when reusing a mock client across multiple test cases.
    pub fn clear_checkpoint_calls(&self) {
        self.checkpoint_calls.lock().unwrap().clear();
    }

    /// Clears all recorded get_operations calls.
    pub fn clear_get_operations_calls(&self) {
        self.get_operations_calls.lock().unwrap().clear();
    }

    /// Clears all recorded calls (both checkpoint and get_operations).
    pub fn clear_all_calls(&self) {
        self.clear_checkpoint_calls();
        self.clear_get_operations_calls();
    }

    /// Returns the number of checkpoint calls made.
    pub fn checkpoint_call_count(&self) -> usize {
        self.checkpoint_calls.lock().unwrap().len()
    }

    /// Returns the number of get_operations calls made.
    pub fn get_operations_call_count(&self) -> usize {
        self.get_operations_calls.lock().unwrap().len()
    }
}

impl Default for MockDurableServiceClient {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MockDurableServiceClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockDurableServiceClient")
            .field(
                "checkpoint_responses_remaining",
                &self.checkpoint_responses.lock().unwrap().len(),
            )
            .field(
                "get_operations_responses_remaining",
                &self.get_operations_responses.lock().unwrap().len(),
            )
            .field(
                "checkpoint_calls_count",
                &self.checkpoint_calls.lock().unwrap().len(),
            )
            .field(
                "get_operations_calls_count",
                &self.get_operations_calls.lock().unwrap().len(),
            )
            .finish()
    }
}

#[async_trait]
impl DurableServiceClient for MockDurableServiceClient {
    async fn checkpoint(
        &self,
        durable_execution_arn: &str,
        checkpoint_token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        // Record the call
        self.checkpoint_calls
            .lock()
            .unwrap()
            .push(CheckpointCall::new(
                durable_execution_arn,
                checkpoint_token,
                operations,
            ));

        // Return the next configured response, or a default response
        let mut responses = self.checkpoint_responses.lock().unwrap();
        if let Some(response) = responses.pop_front() {
            response
        } else {
            // Default response when no responses are configured
            Ok(CheckpointResponse::new("mock-token"))
        }
    }

    async fn get_operations(
        &self,
        durable_execution_arn: &str,
        next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        // Record the call
        self.get_operations_calls
            .lock()
            .unwrap()
            .push(GetOperationsCall::new(durable_execution_arn, next_marker));

        // Return the next configured response, or a default response
        let mut responses = self.get_operations_responses.lock().unwrap();
        if let Some(response) = responses.pop_front() {
            response
        } else {
            // Default response when no responses are configured
            Ok(GetOperationsResponse {
                operations: Vec::new(),
                next_marker: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OperationType;

    #[tokio::test]
    async fn test_mock_client_default_checkpoint_response() {
        let client = MockDurableServiceClient::new();
        let result = client
            .checkpoint("arn:test", "token-123", vec![])
            .await
            .unwrap();
        assert_eq!(result.checkpoint_token, "mock-token");
    }

    #[tokio::test]
    async fn test_mock_client_custom_checkpoint_response() {
        let client = MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("custom-token")));

        let result = client
            .checkpoint("arn:test", "token-123", vec![])
            .await
            .unwrap();
        assert_eq!(result.checkpoint_token, "custom-token");
    }

    #[tokio::test]
    async fn test_mock_client_checkpoint_response_order() {
        let client = MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-1")))
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-2")))
            .with_checkpoint_response(Ok(CheckpointResponse::new("token-3")));

        let r1 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        let r2 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        let r3 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();

        assert_eq!(r1.checkpoint_token, "token-1");
        assert_eq!(r2.checkpoint_token, "token-2");
        assert_eq!(r3.checkpoint_token, "token-3");
    }

    #[tokio::test]
    async fn test_mock_client_checkpoint_error_response() {
        let client = MockDurableServiceClient::new()
            .with_checkpoint_response(Err(DurableError::checkpoint_retriable("Test error")));

        let result = client.checkpoint("arn:test", "token-123", vec![]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().is_retriable());
    }

    #[tokio::test]
    async fn test_mock_client_records_checkpoint_calls() {
        let client = MockDurableServiceClient::new();

        client
            .checkpoint(
                "arn:aws:lambda:us-east-1:123456789012:function:test",
                "token-123",
                vec![OperationUpdate::start("op-1", OperationType::Step)],
            )
            .await
            .unwrap();

        let calls = client.get_checkpoint_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].durable_execution_arn,
            "arn:aws:lambda:us-east-1:123456789012:function:test"
        );
        assert_eq!(calls[0].checkpoint_token, "token-123");
        assert_eq!(calls[0].operations.len(), 1);
        assert_eq!(calls[0].operations[0].operation_id, "op-1");
    }

    #[tokio::test]
    async fn test_mock_client_records_multiple_checkpoint_calls() {
        let client = MockDurableServiceClient::new();

        client
            .checkpoint("arn:test-1", "token-1", vec![])
            .await
            .unwrap();
        client
            .checkpoint("arn:test-2", "token-2", vec![])
            .await
            .unwrap();
        client
            .checkpoint("arn:test-3", "token-3", vec![])
            .await
            .unwrap();

        let calls = client.get_checkpoint_calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].checkpoint_token, "token-1");
        assert_eq!(calls[1].checkpoint_token, "token-2");
        assert_eq!(calls[2].checkpoint_token, "token-3");
    }

    #[tokio::test]
    async fn test_mock_client_clear_checkpoint_calls() {
        let client = MockDurableServiceClient::new();

        client
            .checkpoint("arn:test", "token", vec![])
            .await
            .unwrap();
        assert_eq!(client.checkpoint_call_count(), 1);

        client.clear_checkpoint_calls();
        assert_eq!(client.checkpoint_call_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_client_default_get_operations_response() {
        let client = MockDurableServiceClient::new();
        let result = client
            .get_operations("arn:test", "marker-123")
            .await
            .unwrap();
        assert!(result.operations.is_empty());
        assert!(result.next_marker.is_none());
    }

    #[tokio::test]
    async fn test_mock_client_custom_get_operations_response() {
        let client = MockDurableServiceClient::new().with_get_operations_response(Ok(
            GetOperationsResponse {
                operations: vec![Operation::new("op-1", OperationType::Step)],
                next_marker: Some("next-marker".to_string()),
            },
        ));

        let result = client
            .get_operations("arn:test", "marker-123")
            .await
            .unwrap();
        assert_eq!(result.operations.len(), 1);
        assert_eq!(result.operations[0].operation_id, "op-1");
        assert_eq!(result.next_marker, Some("next-marker".to_string()));
    }

    #[tokio::test]
    async fn test_mock_client_records_get_operations_calls() {
        let client = MockDurableServiceClient::new();

        client
            .get_operations("arn:test", "marker-123")
            .await
            .unwrap();

        let calls = client.get_get_operations_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].durable_execution_arn, "arn:test");
        assert_eq!(calls[0].next_marker, "marker-123");
    }

    #[tokio::test]
    async fn test_mock_client_with_checkpoint_responses() {
        let client = MockDurableServiceClient::new().with_checkpoint_responses(3);

        let r1 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        let r2 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        let r3 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();

        assert_eq!(r1.checkpoint_token, "token-0");
        assert_eq!(r2.checkpoint_token, "token-1");
        assert_eq!(r3.checkpoint_token, "token-2");
    }

    #[tokio::test]
    async fn test_mock_client_with_checkpoint_response_with_operations() {
        let mut op = Operation::new("callback-1", OperationType::Callback);
        op.callback_details = Some(aws_durable_execution_sdk::CallbackDetails {
            callback_id: Some("cb-123".to_string()),
            result: None,
            error: None,
        });

        let client = MockDurableServiceClient::new()
            .with_checkpoint_response_with_operations("token-1", vec![op]);

        let result = client.checkpoint("arn:test", "t", vec![]).await.unwrap();

        assert_eq!(result.checkpoint_token, "token-1");
        let state = result.new_execution_state.unwrap();
        assert_eq!(state.operations.len(), 1);
        assert_eq!(state.operations[0].operation_id, "callback-1");
        assert_eq!(
            state.operations[0]
                .callback_details
                .as_ref()
                .unwrap()
                .callback_id,
            Some("cb-123".to_string())
        );
    }

    #[tokio::test]
    async fn test_mock_client_falls_back_to_default_after_configured_responses() {
        let client = MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("configured-token")));

        // First call returns configured response
        let r1 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        assert_eq!(r1.checkpoint_token, "configured-token");

        // Second call returns default response
        let r2 = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
        assert_eq!(r2.checkpoint_token, "mock-token");
    }

    #[test]
    fn test_mock_client_debug() {
        let client = MockDurableServiceClient::new()
            .with_checkpoint_response(Ok(CheckpointResponse::new("token")));

        let debug_str = format!("{:?}", client);
        assert!(debug_str.contains("MockDurableServiceClient"));
        assert!(debug_str.contains("checkpoint_responses_remaining"));
    }

    #[test]
    fn test_mock_client_default() {
        let client = MockDurableServiceClient::default();
        assert_eq!(client.checkpoint_call_count(), 0);
    }
}

/// Property-based tests for MockDurableServiceClient
///
/// These tests verify the correctness properties defined in the design document.
#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::OperationType;
    use proptest::prelude::*;

    /// Strategy to generate a sequence of checkpoint tokens
    fn token_sequence_strategy() -> impl Strategy<Value = Vec<String>> {
        prop::collection::vec("[a-zA-Z0-9_-]{1,20}", 1..=10)
    }

    /// Strategy to generate a checkpoint call with ARN, token, and operations
    fn checkpoint_call_strategy() -> impl Strategy<Value = (String, String, Vec<OperationUpdate>)> {
        (
            "[a-zA-Z0-9:/_-]{10,50}", // ARN-like string
            "[a-zA-Z0-9_-]{1,20}",    // Token
            prop::collection::vec(
                (
                    "[a-zA-Z0-9_-]{1,20}",
                    prop_oneof![
                        Just(OperationType::Step),
                        Just(OperationType::Wait),
                        Just(OperationType::Callback),
                        Just(OperationType::Invoke),
                        Just(OperationType::Context),
                    ],
                )
                    .prop_map(|(id, op_type)| OperationUpdate::start(id, op_type)),
                0..=5,
            ),
        )
    }

    proptest! {
        /// **Feature: rust-testing-utilities, Property 11: Mock Client Response Order**
        ///
        /// *For any* sequence of N configured checkpoint responses, the mock client
        /// SHALL return them in the order they were configured for the first N checkpoint calls.
        ///
        /// **Validates: Requirements 9.2**
        #[test]
        fn prop_mock_client_response_order(tokens in token_sequence_strategy()) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                // Configure the mock client with the sequence of tokens
                let mut client = MockDurableServiceClient::new();
                for token in &tokens {
                    client = client.with_checkpoint_response(Ok(CheckpointResponse::new(token.clone())));
                }

                // Make checkpoint calls and verify the order
                let mut received_tokens = Vec::new();
                for _ in 0..tokens.len() {
                    let response = client.checkpoint("arn:test", "t", vec![]).await.unwrap();
                    received_tokens.push(response.checkpoint_token);
                }

                // Verify the tokens are returned in the exact order they were configured
                prop_assert_eq!(received_tokens, tokens);
                Ok(())
            })?;
        }

        /// **Feature: rust-testing-utilities, Property 12: Mock Client Call Recording**
        ///
        /// *For any* sequence of checkpoint calls made to the mock client,
        /// `get_checkpoint_calls()` SHALL return all calls in the order they were made.
        ///
        /// **Validates: Requirements 9.3**
        #[test]
        fn prop_mock_client_call_recording(
            calls in prop::collection::vec(checkpoint_call_strategy(), 1..=10)
        ) {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let client = MockDurableServiceClient::new();

                // Make all the checkpoint calls
                for (arn, token, ops) in &calls {
                    let _ = client.checkpoint(arn, token, ops.clone()).await;
                }

                // Get the recorded calls
                let recorded_calls = client.get_checkpoint_calls();

                // Verify the number of recorded calls matches
                prop_assert_eq!(recorded_calls.len(), calls.len());

                // Verify each call was recorded in order with correct data
                for (i, ((expected_arn, expected_token, expected_ops), recorded)) in
                    calls.iter().zip(recorded_calls.iter()).enumerate()
                {
                    prop_assert_eq!(
                        &recorded.durable_execution_arn,
                        expected_arn,
                        "Call {} ARN mismatch",
                        i
                    );
                    prop_assert_eq!(
                        &recorded.checkpoint_token,
                        expected_token,
                        "Call {} token mismatch",
                        i
                    );
                    prop_assert_eq!(
                        recorded.operations.len(),
                        expected_ops.len(),
                        "Call {} operations count mismatch",
                        i
                    );

                    // Verify each operation was recorded correctly
                    for (j, (expected_op, recorded_op)) in
                        expected_ops.iter().zip(recorded.operations.iter()).enumerate()
                    {
                        prop_assert_eq!(
                            &recorded_op.operation_id,
                            &expected_op.operation_id,
                            "Call {} operation {} ID mismatch",
                            i,
                            j
                        );
                        prop_assert_eq!(
                            recorded_op.operation_type,
                            expected_op.operation_type,
                            "Call {} operation {} type mismatch",
                            i,
                            j
                        );
                    }
                }

                Ok(())
            })?;
        }
    }
}
