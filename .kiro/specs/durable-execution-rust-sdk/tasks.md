# Implementation Plan: AWS Durable Execution SDK for Rust

## Overview

This implementation plan breaks down the Rust SDK into incremental tasks that build on each other. The approach starts with core types and error handling, then builds up through the checkpointing system, operation handlers, and finally the Lambda integration macro. This update adds tasks to close gaps with the Language SDK Specification v1.2.

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
    - **PBT Status: PASSED**

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
    - **PBT Status: PASSED**

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
    - **Property 11: Lambda output matches execution outcome**
    - **Validates: Requirements 15.4, 15.5, 15.6, 15.7**
    - **PBT Status: PASSED**

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

## New Tasks - Language SDK Specification v1.2 Gap Closure

- [x] 16. Implement EXECUTION operation support
  - [x] 16.1 Add EXECUTION operation recognition
    - Recognize first operation in state as EXECUTION operation
    - Store execution_operation in ExecutionState
    - Extract original user input from ExecutionDetails.InputPayload
    - _Requirements: 19.1, 19.2_

  - [x] 16.2 Implement get_original_input method
    - Add get_original_input<T>() method to DurableContext
    - Deserialize input payload to requested type
    - Handle missing or invalid input gracefully
    - _Requirements: 1.11, 19.2_

  - [x] 16.3 Implement execution completion via checkpointing
    - Support SUCCEED action on EXECUTION operation
    - Support FAIL action on EXECUTION operation
    - Use for large results that exceed response size limits
    - _Requirements: 19.3, 19.4, 19.5_

- [x] 17. Implement enhanced operation types
  - [x] 17.1 Add missing OperationStatus values
    - Add PENDING status for step operations
    - Add READY status for retry operations
    - Ensure all status values match API specification
    - _Requirements: 3.7, 4.7_

  - [x] 17.2 Add operation metadata fields
    - Add SubType field to Operation and OperationUpdate
    - Add StartTimestamp field to Operation
    - Add EndTimestamp field to Operation
    - _Requirements: 23.1, 23.2, 23.3, 23.4_

  - [x] 17.3 Implement READY status handling
    - Detect READY status during replay
    - Resume execution without re-checkpointing START
    - _Requirements: 3.7_

  - [x] 17.4 Write property test for READY status handling
    - **Property 12: READY Status Resume Without Re-checkpoint**
    - **Validates: Requirements 3.7**

- [x] 18. Implement wait cancellation
  - [x] 18.1 Add cancel_wait method to DurableContext
    - Implement cancel_wait(operation_id) method
    - Checkpoint CANCEL action for WAIT operation
    - Handle already-completed waits gracefully
    - _Requirements: 5.5_

  - [x] 18.2 Update wait handler for cancellation
    - Check for CANCELLED status during replay
    - Return appropriate result for cancelled waits
    - _Requirements: 5.5_

- [x] 19. Implement extended duration support
  - [x] 19.1 Add Duration constructors for weeks, months, years
    - Implement from_weeks constructor
    - Implement from_months constructor (30 days)
    - Implement from_years constructor (365 days)
    - _Requirements: 5.6, 12.7_

- [x] 20. Implement RETRY action enhancements
  - [x] 20.1 Add RETRY action with Payload support
    - Support RETRY action with Payload (not just Error)
    - Enable wait-for-condition pattern using RETRY with state
    - Track attempt number in StepDetails.Attempt
    - _Requirements: 4.7, 4.8, 4.9_

  - [x] 20.2 Update wait_for_condition to use RETRY with Payload
    - Implement as single STEP with RETRY mechanism
    - Pass state as Payload on retry (not Error)
    - Use NextAttemptDelaySeconds for wait intervals
    - _Requirements: 1.8, 4.9_

- [x] 21. Implement ReplayChildren support
  - [x] 21.1 Add ContextConfig with replay_children option
    - Create ContextConfig struct with replay_children field
    - Pass ReplayChildren option to API when starting CONTEXT
    - _Requirements: 10.5, 10.6, 12.8_

  - [x] 21.2 Update child context handler for ReplayChildren
    - Request children state when replay_children is true
    - Replay each branch to combine output for large parallel operations
    - _Requirements: 10.5, 10.6_

- [-] 22. Implement promise combinators
  - [x] 22.1 Implement all combinator
    - Wait for all futures to complete successfully
    - Return error on first failure
    - Implement within a STEP operation for durability
    - _Requirements: 20.1, 20.5_

  - [x] 22.2 Implement all_settled combinator
    - Wait for all futures to settle (success or failure)
    - Return BatchResult with all outcomes
    - Implement within a STEP operation for durability
    - _Requirements: 20.2, 20.5_

  - [x] 22.3 Implement race combinator
    - Return result of first future to settle
    - Implement within a STEP operation for durability
    - _Requirements: 20.3, 20.5_

  - [x] 22.4 Implement any combinator
    - Return result of first future to succeed
    - Return error only if all futures fail
    - Implement within a STEP operation for durability
    - _Requirements: 20.4, 20.5_

  - [x] 22.5 Write property test for promise combinators
    - **Property 13: Promise Combinator Correctness**
    - **Validates: Requirements 20.1, 20.2, 20.3, 20.4**
    - **PBT Status: PASSED**

- [x] 23. Implement enhanced invoke support
  - [x] 23.1 Add TenantId support to invoke
    - Add tenant_id field to InvokeConfig
    - Pass TenantId in ChainedInvokeOptions
    - _Requirements: 7.6_

  - [x] 23.2 Handle STOPPED status for invoke
    - Detect STOPPED status during replay
    - Return appropriate error for stopped invocations
    - _Requirements: 7.7_

- [x] 24. Implement checkpoint token handling
  - [x] 24.1 Implement checkpoint token management
    - Use CheckpointToken from invocation input for first checkpoint
    - Use returned CheckpointToken for subsequent checkpoints
    - Never reuse consumed checkpoint tokens
    - _Requirements: 2.9, 2.10_

  - [x] 24.2 Handle InvalidParameterValueException for tokens
    - Detect "Invalid checkpoint token" error message
    - Allow propagation for Lambda retry
    - _Requirements: 2.11_

- [x] 25. Implement batch checkpoint ordering
  - [x] 25.1 Enforce checkpoint ordering rules
    - Checkpoint operations in execution order
    - Ensure EXECUTION completion is last in batch
    - Ensure child operations after parent CONTEXT starts
    - _Requirements: 2.12_

  - [x] 25.2 Support START+completion in same batch
    - Allow STEP START and SUCCEED/FAIL in same batch
    - Allow CONTEXT START and SUCCEED/FAIL in same batch
    - _Requirements: 2.13_

- [x] 26. Implement replay-aware logging
  - [x] 26.1 Add replay detection to logging
    - Detect replay mode in ExecutionState
    - Add is_replay flag to log context
    - _Requirements: 16.6_

  - [x] 26.2 Implement configurable replay log suppression
    - Add replay_logging_enabled configuration
    - Suppress logs during replay when disabled
    - Allow users to configure behavior
    - _Requirements: 16.6, 16.7_

- [x] 27. Implement replay-safe helpers
  - [x] 27.1 Implement deterministic UUID generator
    - Create uuid_from_operation(operation_id, seed) function
    - Use blake2b hash for deterministic generation
    - Document usage for replay-safe UUIDs
    - _Requirements: 22.1_

  - [x] 27.2 Implement replay-safe timestamp helper
    - Create timestamp_from_execution(state) function
    - Return StartTimestamp from EXECUTION operation
    - Document usage for replay-safe timestamps
    - _Requirements: 22.2, 22.3_

- [x] 28. Implement performance configuration
  - [x] 28.1 Add CheckpointingMode configuration
    - Create CheckpointingMode enum (Eager, Batched, Optimistic)
    - Add checkpointing_mode to ExecutionState
    - Document trade-offs for each mode
    - _Requirements: 24.1, 24.2, 24.3, 24.4_

  - [x] 28.2 Implement eager checkpointing mode
    - Checkpoint after every operation
    - Maximum durability, more API calls
    - _Requirements: 24.1_

  - [x] 28.3 Implement optimistic execution mode
    - Execute multiple operations before checkpointing
    - Best performance, replay may redo more work
    - _Requirements: 24.3_

- [x] 29. Implement enhanced error handling
  - [x] 29.1 Add SizeLimit and Throttling error variants
    - Add SizeLimit error for payload size exceeded
    - Add Throttling error for rate limit exceeded
    - Add ResourceNotFound error for missing executions
    - _Requirements: 13.9, 18.5, 18.6_

  - [x] 29.2 Handle size limit errors gracefully
    - Catch size limit errors from API
    - Return FAILED status with clear message
    - Do not propagate for retry
    - _Requirements: 25.6_

  - [x] 29.3 Handle throttling with retry
    - Detect ThrottlingException from API
    - Retry with exponential backoff
    - _Requirements: 18.5_

- [x] 30. Checkpoint - Ensure all new tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 31. Documentation updates
  - [x] 31.1 Add determinism documentation
    - Document why determinism is required for replay
    - List common sources of non-determinism in Rust
    - Provide examples of correct and incorrect patterns
    - Warn about HashMap iteration order
    - _Requirements: 21.1, 21.2, 21.3, 21.4, 21.5_

  - [x] 31.2 Add execution limits documentation
    - Document maximum execution duration (1 year)
    - Document maximum response payload (6MB)
    - Document checkpoint and history size limits
    - Document wait duration limits
    - _Requirements: 25.1, 25.2, 25.3, 25.4, 25.5_

  - [x] 31.3 Update API documentation
    - Document all new methods and types
    - Add examples for new features
    - Update getting started guide
    - _Requirements: 21.1, 21.2, 21.3, 21.4_

- [x] 32. Final checkpoint - All specification gaps closed
  - Ensure all tests pass
  - Verify conformance to Language SDK Specification v1.2
  - Ask the user if questions arise

## Notes

- Tasks 1-15 are from the original implementation and are marked complete
- Tasks 16-32 are new tasks to close gaps with Language SDK Specification v1.2
- All tasks including property-based tests are required for comprehensive validation
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
