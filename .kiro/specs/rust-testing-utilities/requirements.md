# Requirements Document

## Introduction

This document defines the requirements for creating a testing utilities crate for the AWS Durable Execution SDK for Rust. The testing crate will provide developers with tools to test durable functions locally and against deployed AWS Lambda functions, mirroring the functionality available in the JavaScript SDK's `@aws/durable-execution-sdk-js-testing` package.

**IMPORTANT**: The Rust SDK testing utilities MUST use the same architectural approach as the Node.js SDK, which uses a worker thread-based checkpoint server with full execution state management. This ensures consistent behavior across SDKs and provides realistic testing capabilities.

## Glossary

- **Test_Runner**: A component that executes durable functions and captures execution results for testing purposes
- **Local_Test_Runner**: A test runner that executes durable functions in-process with a checkpoint server running in a separate thread
- **Cloud_Test_Runner**: A test runner that invokes deployed Lambda functions and polls for execution results
- **Test_Result**: An object containing the execution outcome, operations history, and methods for inspection
- **Durable_Operation**: A representation of a single operation (step, wait, callback, invoke, context) within a durable execution
- **Time_Control**: Utilities for manipulating time during tests to skip wait operations
- **Checkpoint_Server**: A separate thread/process that manages execution state, checkpoint tokens, and operation lifecycle (replaces simple mock client)
- **Checkpoint_Worker_Manager**: Component that manages the lifecycle of the checkpoint server thread, handling spawning, communication, and cleanup
- **Execution_Manager**: Component that manages the state of all executions, with each execution having its own Checkpoint_Manager
- **Checkpoint_Manager**: Component that manages checkpoints for a single execution, including operation state, callback lifecycle, and history events
- **Callback_Manager**: Component that manages callback lifecycle including timeouts and heartbeats
- **Event_Processor**: Component that generates history events for execution tracking
- **Operation_Status**: The current state of an operation (Started, Pending, Succeeded, Failed, Cancelled, TimedOut, Stopped)
- **Execution_Status**: The overall state of a durable execution (Running, Succeeded, Failed, etc.)
- **Test_Execution_Orchestrator**: Component that orchestrates the full test execution lifecycle, including polling for checkpoint updates, processing operations, and scheduling handler re-invocations (matches Node.js SDK pattern)
- **Scheduler**: Component that manages when handler re-invocations occur, with queue-based (time-skipping) and timer-based (real-time) implementations
- **Queue_Scheduler**: Scheduler implementation for time-skipping mode that processes functions in FIFO order, ignoring timestamps
- **Timer_Scheduler**: Scheduler implementation for real-time mode that respects actual timestamps using tokio timers

## Requirements

### Requirement 1: Local Test Runner

**User Story:** As a Rust developer, I want to execute and test durable functions locally without AWS deployment, so that I can rapidly iterate on my workflow logic during development.

#### Acceptance Criteria

1. WHEN a developer creates a Local_Test_Runner with a handler function, THE Local_Test_Runner SHALL accept any async function that takes a payload and DurableContext
2. WHEN a developer calls run() with a payload, THE Local_Test_Runner SHALL execute the handler function and return a Test_Result
3. WHEN the handler function completes successfully, THE Test_Result SHALL contain the execution result and status SUCCEEDED
4. WHEN the handler function fails with an error, THE Test_Result SHALL contain the error details and status FAILED
5. WHEN the handler function performs durable operations, THE Local_Test_Runner SHALL capture all operations in the Test_Result
6. WHEN a developer calls reset(), THE Local_Test_Runner SHALL clear all cached operations and history for reuse between tests

### Requirement 2: Time Control for Local Testing

**User Story:** As a Rust developer, I want to skip wait operations during local testing, so that my tests execute quickly without waiting for actual delays.

#### Acceptance Criteria

1. WHEN time skipping is enabled via setup_test_environment(), THE Local_Test_Runner SHALL use Tokio's time manipulation to skip wait durations
2. WHEN a wait operation is encountered with time skipping enabled, THE Local_Test_Runner SHALL advance time instantly without blocking
3. WHEN time skipping is disabled, THE Local_Test_Runner SHALL execute wait operations with real timing
4. WHEN teardown_test_environment() is called, THE Local_Test_Runner SHALL restore normal time behavior

### Requirement 3: Test Result Inspection

**User Story:** As a Rust developer, I want to inspect execution results and operation history, so that I can write assertions to verify my workflow behavior.

#### Acceptance Criteria

1. WHEN a developer calls get_status() on Test_Result, THE Test_Result SHALL return the execution status (Succeeded, Failed, Running, etc.)
2. WHEN a developer calls get_result() on a successful execution, THE Test_Result SHALL return the deserialized result value
3. WHEN a developer calls get_result() on a failed execution, THE Test_Result SHALL return an error
4. WHEN a developer calls get_error() on a failed execution, THE Test_Result SHALL return the error details
5. WHEN a developer calls get_operations() on Test_Result, THE Test_Result SHALL return all operations from the execution
6. WHEN a developer calls get_operations() with a status filter, THE Test_Result SHALL return only operations matching that status
7. WHEN a developer calls get_invocations() on Test_Result, THE Test_Result SHALL return details about each handler invocation

### Requirement 4: Operation Inspection

**User Story:** As a Rust developer, I want to inspect individual operations by name, index, or ID, so that I can verify specific steps in my workflow executed correctly.

#### Acceptance Criteria

1. WHEN a developer calls get_operation(name) on Test_Runner, THE Test_Runner SHALL return the first operation with that name
2. WHEN a developer calls get_operation_by_index(index) on Test_Runner, THE Test_Runner SHALL return the operation at that execution order index
3. WHEN a developer calls get_operation_by_name_and_index(name, index) on Test_Runner, THE Test_Runner SHALL return the nth operation with that name
4. WHEN a developer calls get_operation_by_id(id) on Test_Runner, THE Test_Runner SHALL return the operation with that unique identifier
5. WHEN an operation is not found, THE Test_Runner SHALL return None or an appropriate error

### Requirement 5: Operation Details by Type

**User Story:** As a Rust developer, I want to access type-specific details for each operation, so that I can verify operation-specific behavior like retry attempts, wait durations, and callback results.

#### Acceptance Criteria

1. WHEN a developer calls get_step_details() on a Step operation, THE Durable_Operation SHALL return attempt count, result, and error information
2. WHEN a developer calls get_wait_details() on a Wait operation, THE Durable_Operation SHALL return the wait duration and scheduled end timestamp
3. WHEN a developer calls get_callback_details() on a Callback operation, THE Durable_Operation SHALL return the callback ID, result, and error
4. WHEN a developer calls get_invoke_details() on an Invoke operation, THE Durable_Operation SHALL return the invocation result and error
5. WHEN a developer calls get_context_details() on a Context operation, THE Durable_Operation SHALL return the context result and error
6. IF a developer calls a type-specific method on the wrong operation type, THEN THE Durable_Operation SHALL return an error

### Requirement 6: Callback Testing Support

**User Story:** As a Rust developer, I want to send callback responses during testing, so that I can verify my workflow correctly handles external callback signals.

#### Acceptance Criteria

1. WHEN a developer calls send_callback_success(result) on a callback operation, THE Durable_Operation SHALL send a success response to the checkpoint service
2. WHEN a developer calls send_callback_failure(error) on a callback operation, THE Durable_Operation SHALL send a failure response to the checkpoint service
3. WHEN a developer calls send_callback_heartbeat() on a callback operation, THE Durable_Operation SHALL send a heartbeat to keep the callback active
4. IF a developer calls callback methods on a non-callback operation, THEN THE Durable_Operation SHALL return an error

### Requirement 7: Function Registration for Chained Invokes

**User Story:** As a Rust developer, I want to register additional functions for local testing, so that I can test workflows that invoke other durable functions.

#### Acceptance Criteria

1. WHEN a developer calls register_durable_function(name, handler), THE Local_Test_Runner SHALL store the handler for use during invoke operations
2. WHEN a developer calls register_function(name, handler), THE Local_Test_Runner SHALL store a non-durable handler for use during invoke operations
3. WHEN the workflow invokes a registered function, THE Local_Test_Runner SHALL execute the registered handler locally
4. WHEN the workflow invokes an unregistered function, THE Local_Test_Runner SHALL return an error indicating the function is not registered

### Requirement 8: Cloud Test Runner

**User Story:** As a Rust developer, I want to test against deployed Lambda functions, so that I can validate my workflow behavior in a real AWS environment.

#### Acceptance Criteria

1. WHEN a developer creates a Cloud_Test_Runner with a function name, THE Cloud_Test_Runner SHALL configure the Lambda client for that function
2. WHEN a developer calls run() with a payload, THE Cloud_Test_Runner SHALL invoke the Lambda function and poll for completion
3. WHEN the execution completes, THE Cloud_Test_Runner SHALL return a Test_Result with the execution outcome
4. WHEN a developer provides a custom Lambda client, THE Cloud_Test_Runner SHALL use that client for invocations
5. WHEN a developer configures poll_interval, THE Cloud_Test_Runner SHALL use that interval when polling for execution status

### Requirement 9: Checkpoint Server Architecture (Node.js SDK Parity)

**User Story:** As a Rust developer, I want the local test runner to use a checkpoint server architecture matching the Node.js SDK, so that I get realistic testing behavior and consistent cross-SDK experience.

#### Acceptance Criteria

1. WHEN a developer creates a Local_Test_Runner, THE Local_Test_Runner SHALL spawn a checkpoint server in a separate thread (using Rust's std::thread or tokio::spawn)
2. WHEN the checkpoint server is running, THE Checkpoint_Worker_Manager SHALL manage the lifecycle of the server thread including spawning, communication, and cleanup
3. WHEN an execution starts, THE Execution_Manager SHALL create a new Checkpoint_Manager for that execution
4. WHEN a checkpoint call is made, THE Checkpoint_Manager SHALL process operation updates and maintain full execution state
5. WHEN the test runner is torn down, THE Checkpoint_Worker_Manager SHALL gracefully shut down the checkpoint server thread
6. IF the checkpoint server thread fails, THEN THE Checkpoint_Worker_Manager SHALL report the error and clean up resources

### Requirement 10: Execution State Management

**User Story:** As a Rust developer, I want the checkpoint server to maintain full execution state, so that my tests accurately reflect real-world checkpoint behavior.

#### Acceptance Criteria

1. WHEN an execution is started, THE Execution_Manager SHALL generate a unique execution ID and initial checkpoint token
2. WHEN a checkpoint is received, THE Checkpoint_Manager SHALL update operation states based on the operation updates
3. WHEN operations are queried, THE Checkpoint_Manager SHALL return the current state of all operations for that execution
4. WHEN an invocation starts, THE Checkpoint_Manager SHALL track invocation timestamps and clear dirty operation flags
5. WHEN an invocation completes, THE Checkpoint_Manager SHALL record completion timestamps and generate history events

### Requirement 11: Callback Lifecycle Management

**User Story:** As a Rust developer, I want the checkpoint server to manage callback lifecycle including timeouts and heartbeats, so that my callback tests behave realistically.

#### Acceptance Criteria

1. WHEN a callback operation is created, THE Callback_Manager SHALL track the callback with its timeout configuration
2. WHEN a callback heartbeat is received, THE Callback_Manager SHALL reset the heartbeat timeout for that callback
3. WHEN a callback timeout expires, THE Callback_Manager SHALL mark the callback as timed out and update the operation status
4. WHEN a callback success/failure is received, THE Callback_Manager SHALL complete the callback and update the operation status
5. IF a callback response is received for an already-completed callback, THEN THE Callback_Manager SHALL return an appropriate error

### Requirement 12: History Event Generation

**User Story:** As a Rust developer, I want the checkpoint server to generate history events, so that I can inspect the full execution history in my tests.

#### Acceptance Criteria

1. WHEN an operation status changes, THE Event_Processor SHALL generate a corresponding history event
2. WHEN an invocation completes, THE Event_Processor SHALL generate an InvocationCompleted event with timestamps
3. WHEN a callback is completed, THE Event_Processor SHALL generate callback-related history events
4. WHEN the execution completes, THE Event_Processor SHALL generate execution completion events
5. WHEN a developer queries history events, THE Test_Result SHALL return all generated events in chronological order

### Requirement 13: Thread Communication

**User Story:** As a Rust developer, I want reliable communication between the test runner and checkpoint server thread, so that checkpoint operations work correctly.

#### Acceptance Criteria

1. WHEN the test runner sends a request to the checkpoint server, THE Checkpoint_Worker_Manager SHALL use channels (mpsc or crossbeam) for communication
2. WHEN the checkpoint server processes a request, THE Checkpoint_Worker_Manager SHALL return the response through the channel
3. WHEN multiple requests are sent concurrently, THE Checkpoint_Worker_Manager SHALL handle them correctly without race conditions
4. IF communication with the checkpoint server fails, THEN THE Checkpoint_Worker_Manager SHALL return an appropriate error

### Requirement 14: Debug Output

**User Story:** As a Rust developer, I want to print operation tables for debugging, so that I can quickly understand what happened during test execution.

#### Acceptance Criteria

1. WHEN a developer calls print() on Test_Result, THE Test_Result SHALL print a formatted table of all operations to stdout
2. WHEN a developer calls print() with column configuration, THE Test_Result SHALL include only the specified columns
3. THE printed table SHALL include operation name, type, status, and timing information by default

### Requirement 15: Async Operation Waiting

**User Story:** As a Rust developer, I want to wait for operations to reach specific states, so that I can synchronize my test assertions with asynchronous workflow execution.

#### Acceptance Criteria

1. WHEN a developer calls wait_for_data() on a Durable_Operation, THE Durable_Operation SHALL return a future that resolves when the operation has started
2. WHEN a developer calls wait_for_data(WaitingStatus::Completed) on a Durable_Operation, THE Durable_Operation SHALL return a future that resolves when the operation has completed
3. WHEN a developer calls wait_for_data(WaitingStatus::Submitted) on a callback operation, THE Durable_Operation SHALL return a future that resolves when the callback is ready to receive responses
4. IF the execution completes before the operation reaches the requested state, THEN THE Durable_Operation SHALL return an error

### Requirement 16: Test Execution Orchestrator (Node.js SDK Parity)

**User Story:** As a Rust developer, I want the test runner to orchestrate full execution lifecycle including wait completion and re-invocation, so that my tests accurately simulate real durable execution behavior.

#### Acceptance Criteria

1. WHEN a wait operation is encountered, THE Test_Execution_Orchestrator SHALL track the wait's scheduled end timestamp
2. WHEN time skipping is enabled and a wait's scheduled end time is reached, THE Test_Execution_Orchestrator SHALL mark the wait as SUCCEEDED and schedule handler re-invocation
3. WHEN time skipping is enabled, THE Test_Execution_Orchestrator SHALL use tokio::time::advance() to skip wait durations instantly
4. WHEN a handler invocation returns PENDING status, THE Test_Execution_Orchestrator SHALL continue polling for operation updates and re-invoke the handler when operations complete
5. WHEN a handler invocation returns SUCCEEDED or FAILED status, THE Test_Execution_Orchestrator SHALL resolve the execution and stop polling
6. WHEN multiple operations are pending (waits, callbacks, steps with retries), THE Test_Execution_Orchestrator SHALL process them in scheduled order

### Requirement 17: Scheduler for Operation Processing

**User Story:** As a Rust developer, I want the test runner to schedule handler re-invocations based on operation completion times, so that wait operations and retries are processed at the correct times.

#### Acceptance Criteria

1. WHEN time skipping is enabled, THE Scheduler SHALL use a queue-based approach that processes operations in FIFO order
2. WHEN time skipping is disabled, THE Scheduler SHALL use timer-based scheduling that respects actual timestamps
3. WHEN a function is scheduled, THE Scheduler SHALL execute any checkpoint updates before invoking the handler
4. WHEN the scheduler has pending functions, THE Scheduler SHALL report that scheduled functions exist via has_scheduled_function()
5. WHEN execution completes, THE Scheduler SHALL flush any remaining scheduled functions

### Requirement 18: Checkpoint Polling

**User Story:** As a Rust developer, I want the test runner to poll for checkpoint updates, so that operation state changes are detected and processed.

#### Acceptance Criteria

1. WHEN an execution starts, THE Test_Execution_Orchestrator SHALL begin polling for checkpoint data
2. WHEN checkpoint polling receives operation updates, THE Test_Execution_Orchestrator SHALL process each operation based on its type
3. WHEN a WAIT operation update is received with START action, THE Test_Execution_Orchestrator SHALL schedule re-invocation at the wait's scheduled end timestamp
4. WHEN a STEP operation update is received with RETRY action, THE Test_Execution_Orchestrator SHALL schedule re-invocation at the step's next attempt timestamp
5. WHEN a CALLBACK operation status changes to SUCCEEDED or FAILED, THE Test_Execution_Orchestrator SHALL schedule handler re-invocation
6. WHEN execution completes (SUCCEEDED or FAILED), THE Test_Execution_Orchestrator SHALL stop polling
