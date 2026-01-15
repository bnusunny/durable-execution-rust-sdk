//! Execution state management for the AWS Durable Execution SDK.
//!
//! This module provides the core state management types for durable executions,
//! including checkpoint tracking, replay logic, and operation state management.
//!
//! ## Module Structure
//!
//! - `checkpoint_result` - Result types for checkpoint queries
//! - `replay_status` - Replay state tracking
//! - `batcher` - Checkpoint batching for efficient API calls
//! - `execution_state` - Main execution state management
//!
//! ## Checkpoint Token Management
//!
//! The SDK uses checkpoint tokens to ensure exactly-once checkpoint semantics:
//!
//! 1. **Initial Token**: The first checkpoint uses the `CheckpointToken` from the
//!    `DurableExecutionInvocationInput` provided by Lambda.
//!
//! 2. **Token Updates**: Each successful checkpoint returns a new token that MUST
//!    be used for the next checkpoint. The SDK automatically updates the token
//!    after each successful checkpoint.
//!
//! 3. **Token Consumption**: Once a token is used for a checkpoint, it is consumed
//!    and cannot be reused. Attempting to reuse a consumed token results in an
//!    `InvalidParameterValueException` error.
//!
//! 4. **Error Handling**: If a checkpoint fails with "Invalid checkpoint token",
//!    the error is marked as retriable so Lambda can retry with a fresh token.
//!
//! ## Requirements
//!
//! - 2.9: THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
//! - 2.10: THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
//! - 2.11: THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry

mod checkpoint_result;
mod replay_status;
mod batcher;
mod execution_state;

pub use checkpoint_result::CheckpointedResult;
pub use replay_status::ReplayStatus;
pub use batcher::{
    CheckpointBatcherConfig,
    CheckpointRequest,
    BatchResult,
    CheckpointBatcher,
    CheckpointSender,
    create_checkpoint_queue,
};
pub use execution_state::ExecutionState;
