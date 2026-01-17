//! Checkpoint server module for local testing (Node.js SDK parity).
//!
//! This module implements a checkpoint server that runs in a separate thread,
//! matching the architecture of the Node.js SDK's `CheckpointWorkerManager`.
//! It provides full execution state management for realistic local testing.
//!
//! # Architecture
//!
//! The checkpoint server consists of several components:
//!
//! - **CheckpointWorkerManager**: Manages the lifecycle of the checkpoint server thread
//! - **ExecutionManager**: Manages the state of all executions
//! - **CheckpointManager**: Manages checkpoints for a single execution
//! - **CallbackManager**: Manages callback lifecycle including timeouts and heartbeats
//! - **EventProcessor**: Generates history events for execution tracking
//!
//! # Communication
//!
//! The main thread communicates with the checkpoint server thread via channels:
//! - Requests are sent via `mpsc::Sender<WorkerRequest>`
//! - Responses are received via `mpsc::Receiver<WorkerResponse>`

pub mod callback_manager;
pub mod checkpoint_manager;
pub mod checkpoint_token;
pub mod event_processor;
pub mod execution_manager;
pub mod orchestrator;
pub mod scheduler;
pub mod types;
pub mod worker_manager;

// Re-export main types for convenience
pub use callback_manager::{CallbackManager, CallbackState, CompleteCallbackStatus};
pub use checkpoint_manager::{CheckpointManager, CheckpointOperation, InvocationTimestamps, OperationEvents};
pub use checkpoint_token::{decode_checkpoint_token, encode_checkpoint_token, CheckpointTokenData};
pub use event_processor::EventProcessor;
pub use execution_manager::{ExecutionManager, InvocationResult};
pub use orchestrator::{
    BoxedHandler, InvokeHandlerResult, OperationProcessResult, OperationStorage, ProcessOperationsResult,
    SkipTimeConfig, TestExecutionOrchestrator, TestExecutionResult,
};
pub use scheduler::{
    BoxedAsyncFn, CheckpointUpdateFn, ErrorHandler, QueueScheduler, ScheduledFunction, Scheduler,
    TimerScheduler,
};
pub use types::{
    ApiType, CheckpointWorkerParams, StartDurableExecutionRequest, WorkerApiRequest, WorkerApiResponse, WorkerCommand,
    WorkerCommandType, WorkerResponse, WorkerResponseType,
};
pub use worker_manager::CheckpointWorkerManager;
