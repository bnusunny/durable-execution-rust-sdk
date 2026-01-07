# Implementation Plan: AWS Durable Execution SDK for Rust

## Overview

This implementation plan breaks down the Rust SDK into incremental tasks that build on each other. The approach starts with core types and error handling, then builds up through the checkpointing system, operation handlers, and finally the Lambda integration macro.

## Tasks

- [x] 1. Set up project structure and core types
  - [x] 1.1 Initialize Rust project with Cargo.toml and dependencies
    - Create `aws-durable-execution-sdk` crate
    - Add dependencies: tokio, serde, serde_json, thiserror, tracing, aws-sdk-lambda, lambda_runtime
    - Configure features and workspace settings
    - _Requirements: 14.1, 18.1_

  - [x] 1.2 Implement Duration type with constructors
    - Create Duration struct with seconds field
    - Implement from_seconds, from_minutes, from_hours, from_days constructors
    - Implement to_seconds method
    - Add validation for non-negative values
    - _Requirements: 12.7_

  - [x] 1.3 Write property test for Duration constructors
    - **Property 8: Duration Validation**
    - **Validates: Requirements 5.4, 12.7**

  - [x] 1.4 Implement error types hierarchy
    - Create DurableError enum with all variants (Execution, Invocation, Checkpoint, Callback, NonDeterministic, Validation, SerDes, Suspend, OrphanedChild, UserCode)
    - Implement TerminationReason enum
    - Implement From conversions for common error types
    - Add helper methods (is_retriable for CheckpointError)
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.5, 13.6, 13.7, 13.8_

  - [x] 1.5 Implement configuration types
    - Create StepConfig, StepSemantics enum
    - Create CallbackConfig
    - Create InvokeConfig with generics
    - Create MapConfig, ParallelConfig
    - Create CompletionConfig with factory methods (first_successful, all_completed, all_successful)
    - Create ItemBatcher, ChildConfig
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6_

- [x] 2. Implement serialization system
  - [x] 2.1 Define SerDes trait and context
    - Create SerDes<T> trait with serialize/deserialize methods
    - Create SerDesContext struct with operation_id and durable_execution_arn
    - Create SerDesError type
    - _Requirements: 11.1, 11.3, 11.4_

  - [x] 2.2 Implement JsonSerDes default implementation
    - Create JsonSerDes<T> struct with PhantomData
    - Implement SerDes<T> for JsonSerDes using serde_json
    - Handle serialization/deserialization errors
    - _Requirements: 11.2_

  - [x] 2.3 Write property test for SerDes round-trip
    - **Property 2: SerDes Round-Trip**
    - **Validates: Requirements 11.2**

- [x] 3. Implement Lambda service types and client
  - [x] 3.1 Define Operation and OperationUpdate types
    - Create Operation struct with all fields (operation_id, operation_type, status, result, error, parent_id, name)
    - Create OperationType enum (Execution, Step, Wait, Callback, Invoke, Context)
    - Create OperationStatus enum (Started, Succeeded, Failed, Cancelled, TimedOut, Stopped)
    - Create OperationUpdate struct with OperationAction enum
    - Create ErrorObject struct for error serialization
    - _Requirements: 3.1_

  - [x] 3.2 Define Lambda invocation input/output types
    - Create DurableExecutionInvocationInput with serde attributes
    - Create InitialExecutionState struct
    - Create DurableExecutionInvocationOutput struct
    - Create InvocationStatus enum (SUCCEEDED, FAILED, PENDING)
    - Implement From/Into conversions
    - _Requirements: 15.2, 15.4_

  - [x] 3.3 Implement DurableServiceClient trait and Lambda client
    - Define DurableServiceClient trait with checkpoint and get_operations methods
    - Implement LambdaClient using aws-sdk-lambda
    - Handle AWS API errors and map to DurableError
    - Support custom client configuration
    - _Requirements: 18.2, 18.3_

- [x] 4. Implement ExecutionState and checkpointing
  - [x] 4.1 Implement CheckpointedResult and replay logic
    - Create CheckpointedResult struct with status checking methods
    - Implement is_existent, is_succeeded, is_failed, is_cancelled, is_timed_out methods
    - Create ReplayStatus enum (Replay, New)
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [x] 4.2 Implement ExecutionState core structure
    - Create ExecutionState struct with all fields
    - Implement get_checkpoint_result method
    - Implement track_replay method
    - Implement operations loading and pagination
    - Use RwLock for operations HashMap
    - _Requirements: 3.1, 3.6_

  - [x] 4.3 Implement checkpoint queue and batching
    - Create CheckpointRequest struct with completion channel
    - Create CheckpointBatcher with config
    - Implement batch collection logic (size, time, count limits)
    - Implement background task that processes batches
    - _Requirements: 2.3, 2.7_

  - [x] 4.4 Write property test for checkpoint batching
    - **Property 4: Checkpoint Batching Efficiency**
    - **Validates: Requirements 2.3**

  - [x] 4.5 Implement sync/async checkpoint methods
    - Implement create_checkpoint with is_sync parameter
    - For sync: wait on completion channel
    - For async: fire and forget
    - Handle checkpoint errors appropriately
    - _Requirements: 2.1, 2.2, 2.4, 2.5, 2.6_

  - [x] 4.6 Implement parent-child tracking for orphan prevention
    - Add parent_done_lock Mutex<HashSet<String>>
    - Implement mark_parent_done method
    - Implement is_orphaned check
    - Reject checkpoints from orphaned children
    - _Requirements: 2.8_

  - [x] 4.7 Write property test for orphaned child prevention
    - **Property 9: Orphaned Child Prevention**
    - **Validates: Requirements 2.8**

- [x] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Implement DurableContext core
  - [x] 6.1 Implement OperationIdentifier and ID generation
    - Create OperationIdentifier struct (operation_id, parent_id, name)
    - Implement deterministic ID generation using blake2b hash
    - Use AtomicU64 for thread-safe step counter
    - _Requirements: 1.10_

  - [x] 6.2 Write property test for operation ID determinism
    - **Property 3: Operation ID Determinism**
    - **Validates: Requirements 1.10**

  - [x] 6.3 Write property test for concurrent ID uniqueness
    - **Property 5: Concurrent ID Generation Uniqueness**
    - **Validates: Requirements 17.3**

  - [x] 6.4 Implement DurableContext struct and factory methods
    - Create DurableContext struct with all fields
    - Implement from_lambda_context factory
    - Implement create_child_context method
    - Implement set_logger method
    - Ensure Send + Sync bounds
    - _Requirements: 10.1, 16.5, 17.1_

- [x] 7. Implement operation handlers
  - [x] 7.1 Implement step handler
    - Create StepContext struct
    - Implement step_handler function
    - Handle AT_MOST_ONCE vs AT_LEAST_ONCE semantics
    - Implement retry logic with retry_strategy
    - Checkpoint result or error
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [x] 7.2 Write property test for step semantics
    - **Property 7: Step Semantics Checkpoint Ordering**
    - **Validates: Requirements 4.1, 4.2**
    - **PBT Status: PASSED** - All 5 property tests passed (prop_at_most_once_checkpoints_before_execution, prop_at_least_once_checkpoints_after_execution, prop_at_most_once_checkpoints_error_on_failure, prop_at_least_once_checkpoints_error_on_failure, prop_replay_returns_checkpointed_result)

  - [x] 7.3 Implement wait handler
    - Implement wait_handler function
    - Validate duration >= 1 second
    - Checkpoint wait start
    - Check if wait has elapsed, suspend if not
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [x] 7.4 Implement callback handler
    - Implement callback_handler function
    - Generate unique callback_id via checkpoint
    - Create Callback<T> struct with result() method
    - Handle timeout and heartbeat configuration
    - Suspend when result not available
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7_

  - [x] 7.5 Implement invoke handler
    - Implement invoke_handler function
    - Call target Lambda function via service client
    - Handle timeout configuration
    - Checkpoint invocation and result
    - Propagate errors from invoked function
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

  - [x] 7.6 Implement child context handler
    - Implement child_handler function
    - Create child context with parent operation_id
    - Execute function in child context
    - Checkpoint child result
    - Propagate errors to parent
    - _Requirements: 10.1, 10.2, 10.3, 10.4_

- [x] 8. Implement concurrency system
  - [x] 8.1 Implement ExecutionCounters and completion logic
    - Create ExecutionCounters with atomic counters
    - Implement complete_task, fail_task methods
    - Implement should_complete based on CompletionConfig
    - Implement is_min_successful_reached, is_failure_tolerance_exceeded
    - _Requirements: 14.4_

  - [x] 8.2 Implement ConcurrentExecutor
    - Create ConcurrentExecutor struct
    - Implement execute method with Tokio tasks
    - Respect max_concurrency limit using Semaphore
    - Handle task completion and suspension
    - Signal completion via Notify
    - _Requirements: 14.2, 14.3, 14.5_

  - [x] 8.3 Implement BatchResult and BatchItem types
    - Create BatchResult<T> struct
    - Create BatchItem<T> with status, result, error
    - Create BatchItemStatus and CompletionReason enums
    - Implement succeeded(), failed(), get_results() methods
    - _Requirements: 8.5, 9.4_

  - [x] 8.4 Implement map handler
    - Implement map_handler function
    - Create child context for each item
    - Use ConcurrentExecutor for parallel execution
    - Support ItemBatcher for batching
    - Return BatchResult
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [x] 8.5 Implement parallel handler
    - Implement parallel_handler function
    - Create child context for each branch
    - Use ConcurrentExecutor for parallel execution
    - Return BatchResult
    - _Requirements: 9.1, 9.2, 9.5_

  - [x] 8.6 Write property test for completion criteria
    - **Property 6: Map/Parallel Completion Criteria**
    - **Validates: Requirements 8.6, 8.7, 9.3**
    - **PBT Status: PASSED** - All 9 property tests passed (prop_min_successful_triggers_completion, prop_failure_tolerance_exceeded_triggers_completion, prop_all_completed_triggers_when_all_done, prop_suspended_triggers_when_tasks_suspend, prop_success_count_accurate, prop_failure_count_accurate, prop_completed_count_is_sum, prop_pending_count_accurate, prop_failure_percentage_calculation)

- [x] 9. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 10. Wire DurableContext operations
  - [x] 10.1 Implement DurableContext.step method
    - Call step_handler with appropriate parameters
    - Track replay after completion
    - _Requirements: 1.1_

  - [x] 10.2 Implement DurableContext.wait method
    - Validate duration
    - Call wait_handler
    - Track replay after completion
    - _Requirements: 1.2_

  - [x] 10.3 Implement DurableContext.create_callback method
    - Call callback_handler
    - Return Callback<T> instance
    - Track replay after completion
    - _Requirements: 1.3_

  - [x] 10.4 Implement DurableContext.invoke method
    - Call invoke_handler
    - Track replay after completion
    - _Requirements: 1.4_

  - [x] 10.5 Implement DurableContext.map method
    - Create child context for map operation
    - Call map_handler via child_handler
    - Track replay after completion
    - _Requirements: 1.5_

  - [x] 10.6 Implement DurableContext.parallel method
    - Create child context for parallel operation
    - Call parallel_handler via child_handler
    - Track replay after completion
    - _Requirements: 1.6_

  - [x] 10.7 Implement DurableContext.run_in_child_context method
    - Create child context
    - Call child_handler
    - Track replay after completion
    - _Requirements: 1.7_

  - [x] 10.8 Implement DurableContext.wait_for_condition method
    - Implement polling logic with wait_strategy
    - Track state across iterations
    - _Requirements: 1.8_

  - [x] 10.9 Implement DurableContext.wait_for_callback method
    - Combine callback creation with submitter function
    - Use run_in_child_context internally
    - _Requirements: 1.9_

- [x] 11. Implement replay mechanism
  - [x] 11.1 Implement replay detection in operation handlers
    - Check checkpoint result before executing
    - Return stored result if operation completed
    - Return stored error if operation failed
    - Execute normally if no checkpoint exists
    - _Requirements: 3.2, 3.3, 3.4_

  - [x] 11.2 Implement non-deterministic execution detection
    - Compare operation type with checkpointed type
    - Raise NonDeterministicExecutionError on mismatch
    - _Requirements: 3.5_

  - [x] 11.3 Write property test for replay round-trip
    - **Property 1: Replay Round-Trip Consistency**
    - **Validates: Requirements 3.2, 3.3, 3.4**

  - [x] 11.4 Write property test for non-deterministic detection
    - **Property 10: Non-Deterministic Execution Detection**
    - **Validates: Requirements 3.5**

- [x] 12. Implement Lambda integration
  - [x] 12.1 Implement durable_execution proc macro
    - Create proc-macro crate for #[durable_execution]
    - Parse function signature
    - Generate wrapper that handles DurableExecutionInvocationInput
    - Create ExecutionState and DurableContext
    - Handle result/error/suspend outcomes
    - _Requirements: 15.1, 15.3_

  - [x] 12.2 Implement handler result processing
    - Serialize successful results to JSON
    - Handle large responses (>6MB) by checkpointing
    - Create DurableExecutionInvocationOutput with appropriate status
    - _Requirements: 15.5, 15.6, 15.7, 15.8_

  - [x] 12.3 Write property test for Lambda output
    - **Property: Lambda output matches execution outcome**
    - **Validates: Requirements 15.4, 15.5, 15.6, 15.7**
    - **PBT Status: PASSED** - All 5 property tests passed (prop_lambda_output_success_status, prop_lambda_output_failure_status, prop_lambda_output_suspend_status, prop_lambda_output_serialization_preserves_status, prop_result_size_check_consistency)

- [x] 13. Implement logging integration
  - [x] 13.1 Define Logger trait and default implementation
    - Create Logger trait compatible with tracing
    - Create LogInfo struct with execution context
    - Implement default logger using tracing macros
    - _Requirements: 16.1_

  - [x] 13.2 Add structured logging throughout SDK
    - Include durable_execution_arn in spans
    - Include operation_id in operation logs
    - Include parent_id for child operations
    - _Requirements: 16.2, 16.3, 16.4_

- [x] 14. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 15. Documentation and examples
  - [x] 15.1 Write crate-level documentation
    - Add overview and getting started guide
    - Document all public types and methods
    - Add code examples for common patterns

  - [x] 15.2 Create example Lambda functions
    - Simple step-based workflow
    - Parallel processing example
    - Callback/approval workflow example

## Notes

- All tasks including property-based tests are required for comprehensive validation
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
