//! Checkpoint worker manager for managing the checkpoint server thread.
//!
//! This module implements the CheckpointWorkerManager which manages the lifecycle
//! of the checkpoint server thread, matching the Node.js SDK's CheckpointWorkerManager.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use aws_durable_execution_sdk::{
    CheckpointResponse, DurableError, DurableServiceClient, ErrorObject, GetOperationsResponse,
    OperationUpdate,
};

use super::execution_manager::ExecutionManager;
use super::types::{
    ApiType, CheckpointWorkerParams, CompleteInvocationRequest,
    SendCallbackFailureRequest, SendCallbackHeartbeatRequest, SendCallbackSuccessRequest,
    StartDurableExecutionRequest, StartInvocationRequest, WorkerApiRequest, WorkerApiResponse,
    WorkerCommand, WorkerCommandType, WorkerResponse, WorkerResponseType,
};
use crate::error::TestError;

/// Internal state for the checkpoint server.
struct CheckpointServerState {
    execution_manager: ExecutionManager,
    params: CheckpointWorkerParams,
}

/// Message sent to the worker thread.
enum WorkerMessage {
    /// A command to process
    Command(WorkerCommand, oneshot::Sender<WorkerResponse>),
    /// Shutdown signal
    Shutdown,
}


/// Manages the checkpoint server thread lifecycle.
pub struct CheckpointWorkerManager {
    /// Sender for commands to the worker
    command_tx: mpsc::Sender<WorkerMessage>,
    /// Handle to the worker thread (for joining on shutdown)
    worker_handle: Option<JoinHandle<()>>,
    /// Configuration parameters
    params: CheckpointWorkerParams,
}

/// Global singleton instance
static INSTANCE: OnceLock<Mutex<Option<Arc<CheckpointWorkerManager>>>> = OnceLock::new();

impl CheckpointWorkerManager {
    /// Get or create the singleton instance.
    pub fn get_instance(
        params: Option<CheckpointWorkerParams>,
    ) -> Result<Arc<CheckpointWorkerManager>, TestError> {
        let mutex = INSTANCE.get_or_init(|| Mutex::new(None));
        let mut guard = mutex.lock().map_err(|e| {
            TestError::CheckpointServerError(format!("Failed to lock instance: {}", e))
        })?;

        if let Some(ref instance) = *guard {
            return Ok(Arc::clone(instance));
        }

        let params = params.unwrap_or_default();
        let manager = Self::new(params)?;
        let arc = Arc::new(manager);
        *guard = Some(Arc::clone(&arc));
        Ok(arc)
    }

    /// Reset the singleton instance (for testing).
    /// 
    /// Note: This method should be called from a non-async context or at the start
    /// of a test before any async operations. If called from within an async context,
    /// it will skip the graceful shutdown and just clear the instance.
    pub fn reset_instance_for_testing() {
        if let Some(mutex) = INSTANCE.get() {
            if let Ok(mut guard) = mutex.lock() {
                if let Some(instance) = guard.take() {
                    // Try to shutdown gracefully, but don't panic if we can't
                    if let Ok(manager) = Arc::try_unwrap(instance) {
                        // Check if we're in an async context by trying to get the current runtime
                        // If we are, we can't use shutdown_sync, so just drop the manager
                        if tokio::runtime::Handle::try_current().is_err() {
                            // Not in an async context, safe to use shutdown_sync
                            let _ = manager.shutdown_sync();
                        }
                        // If in async context, the manager will be dropped and the thread
                        // will eventually terminate when the channel is closed
                    }
                }
            }
        }
    }

    /// Create a new checkpoint worker manager.
    fn new(params: CheckpointWorkerParams) -> Result<Self, TestError> {
        let (command_tx, command_rx) = mpsc::channel::<WorkerMessage>(100);

        let worker_params = params.clone();
        let worker_handle = thread::spawn(move || {
            Self::run_worker(command_rx, worker_params);
        });

        Ok(Self {
            command_tx,
            worker_handle: Some(worker_handle),
            params,
        })
    }


    /// Run the worker thread.
    fn run_worker(mut command_rx: mpsc::Receiver<WorkerMessage>, params: CheckpointWorkerParams) {
        // Create a tokio runtime for the worker thread
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for worker");

        rt.block_on(async {
            let mut state = CheckpointServerState {
                execution_manager: ExecutionManager::new(),
                params,
            };

            while let Some(message) = command_rx.recv().await {
                match message {
                    WorkerMessage::Command(command, response_tx) => {
                        let response = Self::process_command(&mut state, command).await;
                        let _ = response_tx.send(response);
                    }
                    WorkerMessage::Shutdown => {
                        break;
                    }
                }
            }
        });
    }

    /// Process a command and return a response.
    async fn process_command(
        state: &mut CheckpointServerState,
        command: WorkerCommand,
    ) -> WorkerResponse {
        // Simulate checkpoint delay if configured
        if let Some(delay_ms) = state.params.checkpoint_delay {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        }

        let api_response = match command.data.api_type {
            ApiType::StartDurableExecution => {
                Self::handle_start_execution(state, &command.data)
            }
            ApiType::StartInvocation => {
                Self::handle_start_invocation(state, &command.data)
            }
            ApiType::CompleteInvocation => {
                Self::handle_complete_invocation(state, &command.data)
            }
            ApiType::CheckpointDurableExecutionState => {
                Self::handle_checkpoint(state, &command.data)
            }
            ApiType::GetDurableExecutionState => {
                Self::handle_get_state(state, &command.data)
            }
            ApiType::SendDurableExecutionCallbackSuccess => {
                Self::handle_callback_success(state, &command.data)
            }
            ApiType::SendDurableExecutionCallbackFailure => {
                Self::handle_callback_failure(state, &command.data)
            }
            ApiType::SendDurableExecutionCallbackHeartbeat => {
                Self::handle_callback_heartbeat(state, &command.data)
            }
            ApiType::UpdateCheckpointData => {
                Self::handle_update_checkpoint_data(state, &command.data)
            }
            _ => WorkerApiResponse::error(
                command.data.api_type,
                command.data.request_id.clone(),
                format!("Unsupported API type: {:?}", command.data.api_type),
            ),
        };

        WorkerResponse {
            response_type: WorkerResponseType::ApiResponse,
            data: api_response,
        }
    }

    /// Handle StartDurableExecution request.
    fn handle_start_execution(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<StartDurableExecutionRequest, _> =
            serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                let result = state.execution_manager.start_execution_from_request(req);
                match serde_json::to_string(&result) {
                    Ok(payload) => WorkerApiResponse::success(
                        request.api_type,
                        request.request_id.clone(),
                        payload,
                    ),
                    Err(e) => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Failed to serialize response: {}", e),
                    ),
                }
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle StartInvocation request.
    fn handle_start_invocation(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<StartInvocationRequest, _> = serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => match state.execution_manager.start_invocation(req) {
                Ok(result) => match serde_json::to_string(&result) {
                    Ok(payload) => WorkerApiResponse::success(
                        request.api_type,
                        request.request_id.clone(),
                        payload,
                    ),
                    Err(e) => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Failed to serialize response: {}", e),
                    ),
                },
                Err(e) => WorkerApiResponse::error(
                    request.api_type,
                    request.request_id.clone(),
                    format!("{}", e),
                ),
            },
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle CompleteInvocation request.
    fn handle_complete_invocation(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<CompleteInvocationRequest, _> = serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => match state.execution_manager.complete_invocation(req) {
                Ok(result) => match serde_json::to_string(&result) {
                    Ok(payload) => WorkerApiResponse::success(
                        request.api_type,
                        request.request_id.clone(),
                        payload,
                    ),
                    Err(e) => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Failed to serialize response: {}", e),
                    ),
                },
                Err(e) => WorkerApiResponse::error(
                    request.api_type,
                    request.request_id.clone(),
                    format!("{}", e),
                ),
            },
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle CheckpointDurableExecutionState request.
    fn handle_checkpoint(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<CheckpointDurableExecutionStateRequest, _> =
            serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Get checkpoint manager by token
                match state
                    .execution_manager
                    .get_checkpoints_by_token_mut(&req.checkpoint_token)
                {
                    Ok(Some(checkpoint_manager)) => {
                        match checkpoint_manager.process_checkpoint(req.operations) {
                            Ok(dirty_ops) => {
                                // Build response matching SDK's CheckpointResponse structure
                                let response = serde_json::json!({
                                    "CheckpointToken": req.checkpoint_token,
                                    "NewExecutionState": {
                                        "Operations": dirty_ops
                                    }
                                });
                                match serde_json::to_string(&response) {
                                    Ok(payload) => WorkerApiResponse::success(
                                        request.api_type,
                                        request.request_id.clone(),
                                        payload,
                                    ),
                                    Err(e) => WorkerApiResponse::error(
                                        request.api_type,
                                        request.request_id.clone(),
                                        format!("Failed to serialize response: {}", e),
                                    ),
                                }
                            }
                            Err(e) => WorkerApiResponse::error(
                                request.api_type,
                                request.request_id.clone(),
                                format!("Checkpoint processing failed: {}", e),
                            ),
                        }
                    }
                    Ok(None) => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        "Execution not found for checkpoint token".to_string(),
                    ),
                    Err(e) => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Invalid checkpoint token: {}", e),
                    ),
                }
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle GetDurableExecutionState request.
    fn handle_get_state(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<GetDurableExecutionStateRequest, _> =
            serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Extract execution ID from ARN (simplified - just use the ARN as ID for testing)
                let execution_id = &req.durable_execution_arn;
                match state.execution_manager.get_checkpoints_by_execution(execution_id) {
                    Some(checkpoint_manager) => {
                        let operations = checkpoint_manager.get_state();
                        let response = GetOperationsResponse {
                            operations,
                            next_marker: None,
                        };
                        match serde_json::to_string(&response) {
                            Ok(payload) => WorkerApiResponse::success(
                                request.api_type,
                                request.request_id.clone(),
                                payload,
                            ),
                            Err(e) => WorkerApiResponse::error(
                                request.api_type,
                                request.request_id.clone(),
                                format!("Failed to serialize response: {}", e),
                            ),
                        }
                    }
                    None => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Execution not found: {}", execution_id),
                    ),
                }
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle SendDurableExecutionCallbackSuccess request.
    fn handle_callback_success(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<SendCallbackSuccessRequest, _> = serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Collect execution IDs first to avoid borrow conflict
                let execution_ids: Vec<String> = state
                    .execution_manager
                    .get_execution_ids()
                    .into_iter()
                    .cloned()
                    .collect();

                // Find the execution containing this callback
                for execution_id in execution_ids {
                    if let Some(checkpoint_manager) = state
                        .execution_manager
                        .get_checkpoints_by_execution_mut(&execution_id)
                    {
                        if checkpoint_manager
                            .callback_manager()
                            .get_callback_state(&req.callback_id)
                            .is_some()
                        {
                            match checkpoint_manager
                                .callback_manager_mut()
                                .send_success(&req.callback_id, &req.result)
                            {
                                Ok(()) => {
                                    return WorkerApiResponse::success(
                                        request.api_type,
                                        request.request_id.clone(),
                                        "{}".to_string(),
                                    );
                                }
                                Err(e) => {
                                    return WorkerApiResponse::error(
                                        request.api_type,
                                        request.request_id.clone(),
                                        format!("{}", e),
                                    );
                                }
                            }
                        }
                    }
                }
                WorkerApiResponse::error(
                    request.api_type,
                    request.request_id.clone(),
                    format!("Callback not found: {}", req.callback_id),
                )
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle SendDurableExecutionCallbackFailure request.
    fn handle_callback_failure(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<SendCallbackFailureRequest, _> = serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Collect execution IDs first to avoid borrow conflict
                let execution_ids: Vec<String> = state
                    .execution_manager
                    .get_execution_ids()
                    .into_iter()
                    .cloned()
                    .collect();

                // Find the execution containing this callback
                for execution_id in execution_ids {
                    if let Some(checkpoint_manager) = state
                        .execution_manager
                        .get_checkpoints_by_execution_mut(&execution_id)
                    {
                        if checkpoint_manager
                            .callback_manager()
                            .get_callback_state(&req.callback_id)
                            .is_some()
                        {
                            match checkpoint_manager
                                .callback_manager_mut()
                                .send_failure(&req.callback_id, &req.error)
                            {
                                Ok(()) => {
                                    return WorkerApiResponse::success(
                                        request.api_type,
                                        request.request_id.clone(),
                                        "{}".to_string(),
                                    );
                                }
                                Err(e) => {
                                    return WorkerApiResponse::error(
                                        request.api_type,
                                        request.request_id.clone(),
                                        format!("{}", e),
                                    );
                                }
                            }
                        }
                    }
                }
                WorkerApiResponse::error(
                    request.api_type,
                    request.request_id.clone(),
                    format!("Callback not found: {}", req.callback_id),
                )
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle SendDurableExecutionCallbackHeartbeat request.
    fn handle_callback_heartbeat(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<SendCallbackHeartbeatRequest, _> =
            serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Collect execution IDs first to avoid borrow conflict
                let execution_ids: Vec<String> = state
                    .execution_manager
                    .get_execution_ids()
                    .into_iter()
                    .cloned()
                    .collect();

                // Find the execution containing this callback
                for execution_id in execution_ids {
                    if let Some(checkpoint_manager) = state
                        .execution_manager
                        .get_checkpoints_by_execution_mut(&execution_id)
                    {
                        if checkpoint_manager
                            .callback_manager()
                            .get_callback_state(&req.callback_id)
                            .is_some()
                        {
                            match checkpoint_manager
                                .callback_manager_mut()
                                .send_heartbeat(&req.callback_id)
                            {
                                Ok(()) => {
                                    return WorkerApiResponse::success(
                                        request.api_type,
                                        request.request_id.clone(),
                                        "{}".to_string(),
                                    );
                                }
                                Err(e) => {
                                    return WorkerApiResponse::error(
                                        request.api_type,
                                        request.request_id.clone(),
                                        format!("{}", e),
                                    );
                                }
                            }
                        }
                    }
                }
                WorkerApiResponse::error(
                    request.api_type,
                    request.request_id.clone(),
                    format!("Callback not found: {}", req.callback_id),
                )
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Handle UpdateCheckpointData request.
    /// 
    /// This method updates the state of a specific operation in the checkpoint server.
    /// It's used by the orchestrator to mark wait operations as SUCCEEDED after time
    /// has been advanced in time-skipping mode.
    fn handle_update_checkpoint_data(
        state: &mut CheckpointServerState,
        request: &WorkerApiRequest,
    ) -> WorkerApiResponse {
        let parsed: Result<super::types::UpdateCheckpointDataRequest, _> =
            serde_json::from_str(&request.payload);

        match parsed {
            Ok(req) => {
                // Get checkpoint manager by execution ID
                match state
                    .execution_manager
                    .get_checkpoints_by_execution_mut(&req.execution_id)
                {
                    Some(checkpoint_manager) => {
                        // Update the operation data
                        checkpoint_manager.update_operation_data(
                            &req.operation_id,
                            req.operation_data,
                        );
                        WorkerApiResponse::success(
                            request.api_type,
                            request.request_id.clone(),
                            "{}".to_string(),
                        )
                    }
                    None => WorkerApiResponse::error(
                        request.api_type,
                        request.request_id.clone(),
                        format!("Execution not found: {}", req.execution_id),
                    ),
                }
            }
            Err(e) => WorkerApiResponse::error(
                request.api_type,
                request.request_id.clone(),
                format!("Failed to parse request: {}", e),
            ),
        }
    }

    /// Send an API request to the checkpoint server and wait for response.
    pub async fn send_api_request(
        &self,
        api_type: ApiType,
        payload: String,
    ) -> Result<WorkerApiResponse, TestError> {
        let request_id = Uuid::new_v4().to_string();
        let command = WorkerCommand {
            command_type: WorkerCommandType::ApiRequest,
            data: WorkerApiRequest {
                api_type,
                request_id: request_id.clone(),
                payload,
            },
        };

        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx
            .send(WorkerMessage::Command(command, response_tx))
            .await
            .map_err(|e| {
                TestError::CheckpointCommunicationError(format!("Failed to send command: {}", e))
            })?;

        let response = response_rx.await.map_err(|e| {
            TestError::CheckpointCommunicationError(format!("Failed to receive response: {}", e))
        })?;

        Ok(response.data)
    }

    /// Gracefully shut down the checkpoint server.
    pub async fn shutdown(mut self) -> Result<(), TestError> {
        // Send shutdown signal
        let _ = self.command_tx.send(WorkerMessage::Shutdown).await;

        // Wait for worker thread to finish
        if let Some(handle) = self.worker_handle.take() {
            handle.join().map_err(|_| {
                TestError::CheckpointServerError("Worker thread panicked".to_string())
            })?;
        }

        Ok(())
    }

    /// Synchronous shutdown (for use in Drop or non-async contexts).
    pub fn shutdown_sync(mut self) -> Result<(), TestError> {
        // Create a runtime to send the shutdown signal
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            let _ = rt.block_on(self.command_tx.send(WorkerMessage::Shutdown));
        }

        // Wait for worker thread to finish
        if let Some(handle) = self.worker_handle.take() {
            handle.join().map_err(|_| {
                TestError::CheckpointServerError("Worker thread panicked".to_string())
            })?;
        }

        Ok(())
    }

    /// Get the configuration parameters.
    pub fn params(&self) -> &CheckpointWorkerParams {
        &self.params
    }
}

// Additional types needed for checkpoint requests
use super::types::{CheckpointDurableExecutionStateRequest, GetDurableExecutionStateRequest};

// Implement DurableServiceClient trait for CheckpointWorkerManager
use async_trait::async_trait;

#[async_trait]
impl DurableServiceClient for CheckpointWorkerManager {
    async fn checkpoint(
        &self,
        durable_execution_arn: &str,
        checkpoint_token: &str,
        operations: Vec<OperationUpdate>,
    ) -> Result<CheckpointResponse, DurableError> {
        let request = CheckpointDurableExecutionStateRequest {
            durable_execution_arn: durable_execution_arn.to_string(),
            checkpoint_token: checkpoint_token.to_string(),
            operations,
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| DurableError::validation(format!("Failed to serialize request: {}", e)))?;

        let response = self
            .send_api_request(ApiType::CheckpointDurableExecutionState, payload)
            .await
            .map_err(|e| DurableError::checkpoint_retriable(format!("Communication error: {}", e)))?;

        if let Some(error) = response.error {
            return Err(DurableError::checkpoint_retriable(error));
        }

        let payload = response
            .payload
            .ok_or_else(|| DurableError::checkpoint_retriable("Empty response payload"))?;

        serde_json::from_str(&payload)
            .map_err(|e| DurableError::validation(format!("Failed to parse response: {}", e)))
    }

    async fn get_operations(
        &self,
        durable_execution_arn: &str,
        _next_marker: &str,
    ) -> Result<GetOperationsResponse, DurableError> {
        let request = GetDurableExecutionStateRequest {
            durable_execution_arn: durable_execution_arn.to_string(),
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| DurableError::validation(format!("Failed to serialize request: {}", e)))?;

        let response = self
            .send_api_request(ApiType::GetDurableExecutionState, payload)
            .await
            .map_err(|e| DurableError::checkpoint_retriable(format!("Communication error: {}", e)))?;

        if let Some(error) = response.error {
            return Err(DurableError::checkpoint_retriable(error));
        }

        let payload = response
            .payload
            .ok_or_else(|| DurableError::checkpoint_retriable("Empty response payload"))?;

        serde_json::from_str(&payload)
            .map_err(|e| DurableError::validation(format!("Failed to parse response: {}", e)))
    }
}

// Callback methods (not part of DurableServiceClient trait)
impl CheckpointWorkerManager {
    /// Send a callback success response.
    pub async fn send_callback_success(
        &self,
        callback_id: &str,
        result: &str,
    ) -> Result<(), DurableError> {
        let request = SendCallbackSuccessRequest {
            callback_id: callback_id.to_string(),
            result: result.to_string(),
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| DurableError::validation(format!("Failed to serialize request: {}", e)))?;

        let response = self
            .send_api_request(ApiType::SendDurableExecutionCallbackSuccess, payload)
            .await
            .map_err(|e| DurableError::checkpoint_retriable(format!("Communication error: {}", e)))?;

        if let Some(error) = response.error {
            return Err(DurableError::execution(error));
        }

        Ok(())
    }

    /// Send a callback failure response.
    pub async fn send_callback_failure(
        &self,
        callback_id: &str,
        error: &ErrorObject,
    ) -> Result<(), DurableError> {
        let request = SendCallbackFailureRequest {
            callback_id: callback_id.to_string(),
            error: error.clone(),
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| DurableError::validation(format!("Failed to serialize request: {}", e)))?;

        let response = self
            .send_api_request(ApiType::SendDurableExecutionCallbackFailure, payload)
            .await
            .map_err(|e| DurableError::checkpoint_retriable(format!("Communication error: {}", e)))?;

        if let Some(error) = response.error {
            return Err(DurableError::execution(error));
        }

        Ok(())
    }

    /// Send a callback heartbeat.
    pub async fn send_callback_heartbeat(&self, callback_id: &str) -> Result<(), DurableError> {
        let request = SendCallbackHeartbeatRequest {
            callback_id: callback_id.to_string(),
        };

        let payload = serde_json::to_string(&request)
            .map_err(|e| DurableError::validation(format!("Failed to serialize request: {}", e)))?;

        let response = self
            .send_api_request(ApiType::SendDurableExecutionCallbackHeartbeat, payload)
            .await
            .map_err(|e| DurableError::checkpoint_retriable(format!("Communication error: {}", e)))?;

        if let Some(error) = response.error {
            return Err(DurableError::execution(error));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint_server::InvocationResult;

    #[tokio::test]
    async fn test_get_instance() {
        CheckpointWorkerManager::reset_instance_for_testing();

        let instance = CheckpointWorkerManager::get_instance(None).unwrap();
        assert!(instance.params().checkpoint_delay.is_none());

        // Getting instance again should return the same one
        let instance2 = CheckpointWorkerManager::get_instance(None).unwrap();
        assert!(Arc::ptr_eq(&instance, &instance2));

        CheckpointWorkerManager::reset_instance_for_testing();
    }

    #[tokio::test]
    async fn test_start_execution() {
        CheckpointWorkerManager::reset_instance_for_testing();

        let manager = CheckpointWorkerManager::get_instance(None).unwrap();

        let request = StartDurableExecutionRequest {
            invocation_id: "inv-1".to_string(),
            payload: Some(r#"{"test": true}"#.to_string()),
        };

        let payload = serde_json::to_string(&request).unwrap();
        let response = manager
            .send_api_request(ApiType::StartDurableExecution, payload)
            .await
            .unwrap();

        assert!(!response.is_error());
        assert!(response.payload.is_some());

        let result: InvocationResult = serde_json::from_str(&response.payload.unwrap()).unwrap();
        assert!(!result.execution_id.is_empty());
        assert_eq!(result.invocation_id, "inv-1");

        CheckpointWorkerManager::reset_instance_for_testing();
    }

    #[tokio::test]
    async fn test_checkpoint_workflow() {
        CheckpointWorkerManager::reset_instance_for_testing();

        let manager = CheckpointWorkerManager::get_instance(None).unwrap();

        // Start execution
        let start_request = StartDurableExecutionRequest {
            invocation_id: "inv-1".to_string(),
            payload: Some("{}".to_string()),
        };
        let payload = serde_json::to_string(&start_request).unwrap();
        let response = manager
            .send_api_request(ApiType::StartDurableExecution, payload)
            .await
            .unwrap();

        let result: InvocationResult = serde_json::from_str(&response.payload.unwrap()).unwrap();

        // Send checkpoint using OperationUpdate::start helper
        let checkpoint_request = CheckpointDurableExecutionStateRequest {
            durable_execution_arn: result.execution_id.clone(),
            checkpoint_token: result.checkpoint_token.clone(),
            operations: vec![
                OperationUpdate::start("op-1", aws_durable_execution_sdk::OperationType::Step)
                    .with_name("test-step")
            ],
        };

        let payload = serde_json::to_string(&checkpoint_request).unwrap();
        let response = manager
            .send_api_request(ApiType::CheckpointDurableExecutionState, payload)
            .await
            .unwrap();

        assert!(!response.is_error());

        CheckpointWorkerManager::reset_instance_for_testing();
    }
}
