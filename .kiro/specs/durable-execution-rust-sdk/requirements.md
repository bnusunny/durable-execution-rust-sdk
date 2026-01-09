# Requirements Document

## Introduction

This document specifies the requirements for implementing the AWS Durable Execution SDK for the Lambda Rust Runtime. The SDK enables developers to build reliable, long-running workflows in AWS Lambda using Rust, with automatic checkpointing, replay, and state management. The implementation should provide idiomatic Rust APIs while conforming to the AWS Lambda Durable Functions Language SDK Specification v1.2.

## Glossary

- **DurableContext**: The main interface that provides durable operations (step, wait, callback, map, parallel, invoke, child context) to user code
- **Checkpoint**: A persisted snapshot of execution state that enables resumption after interruptions
- **Replay**: The process of re-executing a function where completed operations return their checkpointed results instantly without re-execution
- **Step**: A unit of work that is checkpointed and can be retried with configurable semantics
- **Callback**: A mechanism to wait for external systems to signal completion via callback ID
- **ExecutionState**: Internal state manager that tracks operations, handles checkpointing, and manages replay
- **SerDes**: Serialization/Deserialization interface for custom data encoding in checkpoints
- **BatchResult**: Result type for map/parallel operations containing success/failure information for each item
- **CompletionConfig**: Configuration defining success/failure criteria for concurrent operations
- **SuspendExecution**: Signal to pause execution and return control to Lambda runtime
- **OperationIdentifier**: Unique identifier for each operation combining operation_id, parent_id, and optional name
- **EXECUTION_Operation**: Special operation representing the overall execution, providing access to original invocation input
- **ReplayChildren**: Option for CONTEXT operations to include child state during replay for large parallel operations
- **Checkpoint_Token**: A one-time-use token ensuring exactly-once checkpoint semantics

## Requirements

### Requirement 1: Core DurableContext Interface

**User Story:** As a Rust developer, I want a DurableContext that provides durable operations, so that I can build reliable workflows that survive Lambda restarts.

#### Acceptance Criteria

1. THE DurableContext SHALL provide a `step` method that executes a closure and checkpoints the result
2. THE DurableContext SHALL provide a `wait` method that pauses execution for a specified Duration
3. THE DurableContext SHALL provide a `create_callback` method that returns a Callback with a unique callback_id
4. THE DurableContext SHALL provide an `invoke` method that calls other durable functions
5. THE DurableContext SHALL provide a `map` method that processes a collection in parallel with configurable concurrency
6. THE DurableContext SHALL provide a `parallel` method that executes multiple closures concurrently
7. THE DurableContext SHALL provide a `run_in_child_context` method that creates isolated nested workflows
8. THE DurableContext SHALL provide a `wait_for_condition` method that polls until a condition is met
9. THE DurableContext SHALL provide a `wait_for_callback` method that combines callback creation with a submitter function
10. THE DurableContext SHALL generate deterministic operation IDs using a thread-safe counter and parent context
11. THE DurableContext SHALL provide access to the original user input from the EXECUTION operation

### Requirement 2: Checkpointing System

**User Story:** As a Rust developer, I want automatic checkpointing of my workflow state, so that my execution can resume exactly where it left off after interruptions.

#### Acceptance Criteria

1. WHEN an operation completes successfully, THE Checkpointing_System SHALL persist the result to AWS Lambda's durable execution service
2. WHEN an operation fails, THE Checkpointing_System SHALL persist the error state
3. THE Checkpointing_System SHALL batch multiple checkpoint operations for efficiency
4. THE Checkpointing_System SHALL support both synchronous (blocking) and asynchronous (non-blocking) checkpoints
5. WHEN a checkpoint fails with a retriable error, THE Checkpointing_System SHALL propagate the error to trigger Lambda retry
6. WHEN a checkpoint fails with a non-retriable error, THE Checkpointing_System SHALL return FAILED status without retry
7. THE Checkpointing_System SHALL use a background task for batch processing checkpoints
8. THE Checkpointing_System SHALL track parent-child relationships to prevent orphaned operations from checkpointing
9. THE Checkpointing_System SHALL use the CheckpointToken from invocation input for the first checkpoint
10. THE Checkpointing_System SHALL use the returned CheckpointToken from each checkpoint response for subsequent checkpoints
11. THE Checkpointing_System SHALL handle InvalidParameterValueException for invalid tokens by allowing propagation for retry
12. WHEN batching operations, THE Checkpointing_System SHALL checkpoint in execution order with EXECUTION completion last
13. THE Checkpointing_System SHALL support including both START and completion actions for STEP/CONTEXT in the same batch

### Requirement 3: Replay Mechanism

**User Story:** As a Rust developer, I want completed operations to return their checkpointed results instantly during replay, so that my workflow resumes efficiently without re-executing completed work.

#### Acceptance Criteria

1. WHEN a function resumes after interruption, THE Replay_Mechanism SHALL load all previously checkpointed operations
2. WHEN an operation ID matches a completed checkpoint, THE Replay_Mechanism SHALL return the stored result without re-execution
3. WHEN an operation ID matches a failed checkpoint, THE Replay_Mechanism SHALL return the stored error
4. WHEN an operation ID has no matching checkpoint, THE Replay_Mechanism SHALL execute the operation normally
5. THE Replay_Mechanism SHALL detect non-deterministic execution when operation types don't match checkpointed state
6. THE Replay_Mechanism SHALL support paginated loading of operations for large execution histories
7. WHEN an operation is in READY status, THE Replay_Mechanism SHALL resume execution without re-checkpointing START

### Requirement 4: Step Operations

**User Story:** As a Rust developer, I want to execute code steps with configurable retry and execution semantics, so that I can handle transient failures gracefully.

#### Acceptance Criteria

1. THE Step_Operation SHALL support AT_MOST_ONCE_PER_RETRY semantics that checkpoint before execution
2. THE Step_Operation SHALL support AT_LEAST_ONCE_PER_RETRY semantics that checkpoint after execution
3. WHEN a retry strategy is configured, THE Step_Operation SHALL retry failed steps according to the strategy
4. THE Step_Operation SHALL support custom SerDes for result serialization
5. WHEN a step succeeds, THE Step_Operation SHALL checkpoint the serialized result
6. WHEN a step fails after all retries, THE Step_Operation SHALL checkpoint the error
7. THE Step_Operation SHALL support RETRY action with NextAttemptDelaySeconds for backoff
8. THE Step_Operation SHALL track attempt numbers in StepDetails.Attempt
9. THE Step_Operation SHALL support RETRY action with Payload (not just Error) for wait-for-condition pattern

### Requirement 5: Wait Operations

**User Story:** As a Rust developer, I want to pause my workflow for a specified duration without blocking Lambda resources, so that I can implement time-based workflows efficiently.

#### Acceptance Criteria

1. WHEN wait is called with a Duration, THE Wait_Operation SHALL checkpoint the wait start time
2. IF the wait duration has not elapsed, THEN THE Wait_Operation SHALL suspend execution
3. WHEN the wait duration has elapsed, THE Wait_Operation SHALL allow execution to continue
4. THE Wait_Operation SHALL validate that duration is at least 1 second
5. THE Wait_Operation SHALL support cancellation of active waits via CANCEL action
6. THE Wait_Operation SHALL accept duration specifications in seconds, minutes, hours, days, weeks, months, and years

### Requirement 6: Callback Operations

**User Story:** As a Rust developer, I want to wait for external systems to signal my workflow via callbacks, so that I can integrate with asynchronous external processes.

#### Acceptance Criteria

1. WHEN create_callback is called, THE Callback_Operation SHALL generate a unique callback_id
2. THE Callback_Operation SHALL support configurable timeout duration (TimeoutSeconds)
3. THE Callback_Operation SHALL support configurable heartbeat timeout (HeartbeatTimeoutSeconds)
4. WHEN Callback.result() is called and result is not available, THE Callback_Operation SHALL suspend execution
5. WHEN the callback receives a success signal, THE Callback_Operation SHALL return the result
6. WHEN the callback receives a failure signal, THE Callback_Operation SHALL return a CallbackError
7. WHEN the callback times out, THE Callback_Operation SHALL return a timeout error with TIMED_OUT status

### Requirement 7: Invoke Operations

**User Story:** As a Rust developer, I want to invoke other durable functions from my workflow, so that I can compose complex multi-function workflows.

#### Acceptance Criteria

1. WHEN invoke is called, THE Invoke_Operation SHALL call the target Lambda function
2. THE Invoke_Operation SHALL support function name or ARN as target
3. THE Invoke_Operation SHALL support custom SerDes for payload and result
4. THE Invoke_Operation SHALL checkpoint the invocation and result
5. WHEN the invoked function fails, THE Invoke_Operation SHALL propagate the error
6. THE Invoke_Operation SHALL support optional TenantId for tenant isolation scenarios
7. THE Invoke_Operation SHALL handle STOPPED status when execution is stopped externally

### Requirement 8: Map Operations

**User Story:** As a Rust developer, I want to process collections in parallel with configurable concurrency and failure tolerance, so that I can efficiently handle batch workloads.

#### Acceptance Criteria

1. WHEN map is called with a collection, THE Map_Operation SHALL execute the function for each item
2. THE Map_Operation SHALL support configurable max_concurrency to limit parallel execution
3. THE Map_Operation SHALL support CompletionConfig to define success/failure criteria
4. THE Map_Operation SHALL support ItemBatcher for grouping items into batches
5. THE Map_Operation SHALL return a BatchResult containing results for all items
6. WHEN min_successful is reached, THE Map_Operation SHALL complete successfully
7. WHEN failure tolerance is exceeded, THE Map_Operation SHALL complete with failure
8. THE Map_Operation SHALL use a parent CONTEXT operation with child CONTEXT operations for each item

### Requirement 9: Parallel Operations

**User Story:** As a Rust developer, I want to execute multiple independent operations concurrently, so that I can optimize workflow performance.

#### Acceptance Criteria

1. WHEN parallel is called with a list of closures, THE Parallel_Operation SHALL execute them concurrently
2. THE Parallel_Operation SHALL support configurable max_concurrency
3. THE Parallel_Operation SHALL support CompletionConfig to define success/failure criteria
4. THE Parallel_Operation SHALL return a BatchResult containing results for all branches
5. WHEN a branch suspends, THE Parallel_Operation SHALL handle the suspension appropriately
6. THE Parallel_Operation SHALL support named and unnamed branches
7. THE Parallel_Operation SHALL use a parent CONTEXT operation with child CONTEXT operations for each branch

### Requirement 10: Child Context Operations

**User Story:** As a Rust developer, I want to create isolated nested workflows, so that I can organize complex logic and handle errors at different scopes.

#### Acceptance Criteria

1. WHEN run_in_child_context is called, THE Child_Context_Operation SHALL create a new context with the parent's operation_id
2. THE Child_Context_Operation SHALL checkpoint the child context result when complete
3. THE Child_Context_Operation SHALL support custom SerDes for result serialization
4. WHEN a child context fails, THE Child_Context_Operation SHALL propagate the error to the parent
5. THE Child_Context_Operation SHALL support ReplayChildren option for large parallel operations
6. WHEN ReplayChildren is true, THE Child_Context_Operation SHALL include child operations in state loads for replay

### Requirement 11: Serialization System

**User Story:** As a Rust developer, I want to customize how my data is serialized in checkpoints, so that I can use efficient or domain-specific encodings.

#### Acceptance Criteria

1. THE Serialization_System SHALL provide a SerDes trait with serialize and deserialize methods
2. THE Serialization_System SHALL provide a default JSON SerDes implementation
3. THE Serialization_System SHALL pass SerDesContext with operation_id and execution_arn to serializers
4. WHEN serialization fails, THE Serialization_System SHALL return a SerDesError
5. THE Serialization_System SHALL support primitive types (string, number, boolean, null)
6. THE Serialization_System SHALL support complex types (objects, arrays)
7. WHEN deserialization fails, THE Serialization_System SHALL treat it as a terminal failure

### Requirement 12: Configuration Types

**User Story:** As a Rust developer, I want type-safe configuration for all operations, so that I can configure behavior with compile-time guarantees.

#### Acceptance Criteria

1. THE Configuration_System SHALL provide StepConfig with retry_strategy, step_semantics, and serdes
2. THE Configuration_System SHALL provide CallbackConfig with timeout and heartbeat_timeout
3. THE Configuration_System SHALL provide InvokeConfig with function_name, tenant_id, and payload/result serdes
4. THE Configuration_System SHALL provide MapConfig with max_concurrency, item_batcher, completion_config, and serdes
5. THE Configuration_System SHALL provide ParallelConfig with max_concurrency, completion_config, and serdes
6. THE Configuration_System SHALL provide CompletionConfig with min_successful, tolerated_failure_count, and tolerated_failure_percentage
7. THE Configuration_System SHALL provide Duration type with constructors from seconds, minutes, hours, days, weeks, months, and years
8. THE Configuration_System SHALL provide ContextConfig with replay_children option

### Requirement 13: Error Handling

**User Story:** As a Rust developer, I want a comprehensive error type hierarchy, so that I can handle different failure modes appropriately.

#### Acceptance Criteria

1. THE Error_System SHALL define DurableExecutionError as the base error type
2. THE Error_System SHALL define ExecutionError for errors that return FAILED without retry
3. THE Error_System SHALL define InvocationError for errors that trigger Lambda retry
4. THE Error_System SHALL define CheckpointError for checkpoint failures with retriable/non-retriable distinction
5. THE Error_System SHALL define CallbackError for callback-specific failures including TIMED_OUT
6. THE Error_System SHALL define NonDeterministicExecutionError for replay mismatches
7. THE Error_System SHALL define ValidationError for invalid configuration or arguments
8. THE Error_System SHALL define SerDesError for serialization failures
9. THE Error_System SHALL provide ErrorObject with ErrorType, ErrorMessage, StackTrace, and ErrorData fields

### Requirement 14: Concurrency Implementation

**User Story:** As a Rust developer, I want map and parallel operations to execute efficiently using async Rust, so that I can maximize throughput.

#### Acceptance Criteria

1. THE Concurrency_System SHALL use Tokio for async execution of concurrent operations
2. THE Concurrency_System SHALL track execution state for each concurrent branch
3. THE Concurrency_System SHALL support suspension and resumption of individual branches
4. THE Concurrency_System SHALL use ExecutionCounters to track success/failure counts
5. WHEN completion criteria are met, THE Concurrency_System SHALL signal completion to waiting branches

### Requirement 15: Lambda Integration

**User Story:** As a Rust developer, I want a decorator/macro that integrates with the Lambda Rust Runtime, so that I can easily create durable Lambda functions.

#### Acceptance Criteria

1. THE Lambda_Integration SHALL provide a `#[durable_execution]` attribute macro for handler functions
2. THE Lambda_Integration SHALL parse DurableExecutionInvocationInput from the Lambda event
3. THE Lambda_Integration SHALL create ExecutionState and DurableContext for the handler
4. THE Lambda_Integration SHALL return DurableExecutionInvocationOutput with appropriate status
5. WHEN the handler returns successfully, THE Lambda_Integration SHALL return SUCCEEDED status with result
6. WHEN the handler fails, THE Lambda_Integration SHALL return FAILED status with error
7. WHEN execution suspends, THE Lambda_Integration SHALL return PENDING status
8. THE Lambda_Integration SHALL handle large responses by checkpointing via EXECUTION operation before returning
9. THE Lambda_Integration SHALL extract original user input from ExecutionDetails.InputPayload
10. THE Lambda_Integration SHALL recognize the first operation in state as the EXECUTION operation

### Requirement 16: Logging Integration

**User Story:** As a Rust developer, I want structured logging that includes execution context, so that I can debug and monitor my workflows effectively.

#### Acceptance Criteria

1. THE Logging_System SHALL support integration with the tracing crate
2. THE Logging_System SHALL include durable_execution_arn in log context
3. THE Logging_System SHALL include operation_id in log context when available
4. THE Logging_System SHALL include parent_id in log context for child operations
5. THE DurableContext SHALL provide a method to set a custom logger
6. THE Logging_System SHALL support replay-aware logging that can suppress logs during replay
7. THE Logging_System SHALL allow users to configure replay logging behavior

### Requirement 17: Thread Safety

**User Story:** As a Rust developer, I want the SDK to be safe for use in concurrent contexts, so that I can use it with async Rust without data races.

#### Acceptance Criteria

1. THE DurableContext SHALL be Send + Sync for use across async tasks
2. THE ExecutionState SHALL use appropriate synchronization primitives for concurrent access
3. THE step counter SHALL use atomic operations or locks for thread-safe ID generation
4. THE checkpoint queue SHALL be thread-safe for producer-consumer access

### Requirement 18: AWS SDK Integration

**User Story:** As a Rust developer, I want the SDK to use the official AWS SDK for Rust, so that I have consistent AWS integration and credential handling.

#### Acceptance Criteria

1. THE AWS_Integration SHALL use aws-sdk-lambda for Lambda API calls
2. THE AWS_Integration SHALL support custom AWS SDK client configuration
3. THE AWS_Integration SHALL handle AWS API errors and map them to SDK error types
4. THE AWS_Integration SHALL support standard AWS credential resolution
5. THE AWS_Integration SHALL handle ThrottlingException with appropriate retry behavior
6. THE AWS_Integration SHALL handle ResourceNotFoundException appropriately

### Requirement 19: EXECUTION Operation

**User Story:** As a Rust developer, I want access to the original invocation input and the ability to complete execution via checkpointing, so that I can handle large results and access input data.

#### Acceptance Criteria

1. THE EXECUTION_Operation SHALL be recognized as the first operation in state
2. THE EXECUTION_Operation SHALL provide access to original user input from ExecutionDetails.InputPayload
3. THE EXECUTION_Operation SHALL support completing execution via SUCCEED action with result
4. THE EXECUTION_Operation SHALL support completing execution via FAIL action with error
5. WHEN execution result exceeds response size limits, THE EXECUTION_Operation SHALL checkpoint the result and return empty Result field

### Requirement 20: Promise Combinators

**User Story:** As a Rust developer, I want to coordinate multiple durable promises using familiar patterns, so that I can compose complex async workflows.

#### Acceptance Criteria

1. THE Promise_Combinators SHALL provide an `all` method that waits for all promises to complete successfully
2. THE Promise_Combinators SHALL provide an `all_settled` method that waits for all promises to settle
3. THE Promise_Combinators SHALL provide a `race` method that returns the first promise to settle
4. THE Promise_Combinators SHALL provide an `any` method that returns the first promise to succeed
5. THE Promise_Combinators SHALL be implemented within a STEP operation to ensure durability
6. THE Promise_Combinators SHALL support proper error propagation and result collection

### Requirement 21: Determinism Documentation

**User Story:** As a Rust developer, I want clear documentation about determinism requirements, so that I can write correct durable functions that replay properly.

#### Acceptance Criteria

1. THE Documentation SHALL clearly explain why determinism is required for replay
2. THE Documentation SHALL list common sources of non-determinism in Rust
3. THE Documentation SHALL provide examples of correct and incorrect patterns
4. THE Documentation SHALL explain how to refactor non-deterministic code
5. THE Documentation SHALL warn about Rust-specific non-deterministic constructs (e.g., HashMap iteration order)

### Requirement 22: Replay-Safe Helpers

**User Story:** As a Rust developer, I want helpers to generate replay-safe non-deterministic values, so that I can use UUIDs and timestamps safely in my workflows.

#### Acceptance Criteria

1. THE Replay_Safe_Helpers MAY provide a deterministic UUID generator seeded by operation ID
2. THE Replay_Safe_Helpers MAY provide replay-safe timestamps derived from execution state
3. THE Replay_Safe_Helpers SHALL document how to use these helpers correctly

### Requirement 23: Operation Metadata

**User Story:** As a Rust developer, I want operations to include metadata like timestamps and subtypes, so that I can track operation timing and categorization.

#### Acceptance Criteria

1. THE Operation_Metadata SHALL include StartTimestamp when operation started
2. THE Operation_Metadata SHALL include EndTimestamp when operation completed
3. THE Operation_Metadata SHALL support SubType for SDK-level categorization of operations
4. THE Operation_Metadata SHALL include Name for human-readable operation identification

### Requirement 24: Performance Configuration

**User Story:** As a Rust developer, I want to configure the trade-off between performance and durability, so that I can optimize for my specific use case.

#### Acceptance Criteria

1. THE Performance_Configuration SHALL support eager checkpointing mode (checkpoint after every operation)
2. THE Performance_Configuration SHALL support batched checkpointing mode (group operations per checkpoint)
3. THE Performance_Configuration SHALL support optimistic execution mode (execute multiple operations before checkpointing)
4. THE Performance_Configuration SHALL document the default behavior and trade-offs

### Requirement 25: Execution Limits Documentation

**User Story:** As a Rust developer, I want clear documentation of execution limits, so that I can design my workflows within service constraints.

#### Acceptance Criteria

1. THE Limits_Documentation SHALL document maximum execution duration (1 year)
2. THE Limits_Documentation SHALL document maximum response payload (6MB)
3. THE Limits_Documentation SHALL document checkpoint and history size limits
4. THE Limits_Documentation SHALL document minimum and maximum wait duration
5. THE Limits_Documentation SHALL document maximum callback timeout
6. THE SDK SHALL gracefully handle execution limits by returning clear error messages

