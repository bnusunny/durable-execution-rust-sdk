# Requirements Document

## Introduction

The Rust testing crate (`aws-durable-execution-sdk-testing`) has functional gaps compared to the Node.js testing package (`@aws/durable-execution-sdk-js-testing`). These gaps prevent Rust SDK users from writing idiomatic callback interaction tests, pre-run operation querying, and child operation enumeration patterns that Node.js users rely on. This spec defines the requirements to close those gaps and achieve testing utility parity between the two SDKs.

## Glossary

- **Local_Test_Runner**: The `LocalDurableTestRunner` struct in the Rust testing crate that executes durable function handlers in-process for testing
- **Durable_Operation**: The `DurableOperation` struct in the Rust testing crate that represents a single operation captured during execution, providing inspection and interaction methods
- **Test_Result**: The `TestResult` struct in the Rust testing crate that holds the outcome of a test execution including status, result/error, operations, and invocations
- **Waiting_Operation_Status**: The `WaitingOperationStatus` enum representing the status thresholds an operation can be waited on: Started, Submitted, Completed
- **Callback_Sender**: The `CallbackSender` trait in the Rust testing crate used to send callback success, failure, and heartbeat responses
- **Operation_Handle**: A lazy reference to a named operation that is registered before `run()` and auto-populates with operation data during execution
- **Test_Result_Error**: The `TestResultError` struct in the Rust testing crate representing error details from a failed execution or operation
- **Checkpoint_Worker**: The `CheckpointWorkerManager` that manages the in-process checkpoint server for local test execution

## Requirements

### Requirement 1: Lazy Operation Registration (Pre-Run Querying)

**User Story:** As a Rust SDK test author, I want to obtain an operation handle by name before calling `run()`, so that I can set up `wait_for_data()` listeners and interact with operations mid-execution without needing execution to complete first.

#### Acceptance Criteria

1. WHEN a developer calls `get_operation(name)` on the Local_Test_Runner before calling `run()`, THE Local_Test_Runner SHALL return an Operation_Handle that is initially unpopulated
2. WHEN the handler executes and produces an operation matching the registered name, THE Local_Test_Runner SHALL populate the Operation_Handle with the operation data
3. WHEN the Operation_Handle is populated, THE Operation_Handle SHALL support all Durable_Operation inspection methods (get_id, get_name, get_type, get_status, get_step_details, get_callback_details, get_wait_details, get_invoke_details, get_context_details)
4. WHEN the Operation_Handle is populated, THE Operation_Handle SHALL support `wait_for_data(status)` to await a specific Waiting_Operation_Status
5. WHEN the Operation_Handle is populated with a callback operation, THE Operation_Handle SHALL support `send_callback_success(result)`, `send_callback_failure(error)`, and `send_callback_heartbeat()`
6. IF `run()` completes and no operation matches the registered name, THEN THE Operation_Handle SHALL return an error when inspection methods are called
7. WHEN `get_operation_by_index(index)` is called before `run()`, THE Local_Test_Runner SHALL return an Operation_Handle that populates with the operation at that execution order index
8. WHEN `get_operation_by_id(id)` is called before `run()`, THE Local_Test_Runner SHALL return an Operation_Handle that populates with the operation matching that unique ID

### Requirement 2: Non-Blocking Run with Mid-Execution Interaction

**User Story:** As a Rust SDK test author, I want `run()` to return a future I can await concurrently while interacting with operations mid-execution, so that I can test callback workflows where external input is required before the handler can complete.

#### Acceptance Criteria

1. WHEN a developer calls `run()` on the Local_Test_Runner, THE Local_Test_Runner SHALL return a future that resolves to a Test_Result when the handler completes
2. WHEN `run()` is executing, THE Local_Test_Runner SHALL allow concurrent calls to `wait_for_data()` on pre-registered Operation_Handles
3. WHEN `run()` is executing and an Operation_Handle reaches the Submitted status for a callback operation, THE Operation_Handle SHALL allow `send_callback_success(result)` to deliver the callback response to the running handler
4. WHEN `run()` is executing and an Operation_Handle reaches the Submitted status for a callback operation, THE Operation_Handle SHALL allow `send_callback_failure(error)` to deliver a callback failure to the running handler
5. WHEN a callback response is sent via an Operation_Handle during execution, THE Local_Test_Runner SHALL deliver the response to the checkpoint server and resume handler execution
6. IF `run()` completes before `wait_for_data()` resolves on an Operation_Handle, THEN THE Operation_Handle SHALL return an error indicating the execution completed before the operation reached the requested status

### Requirement 3: Child Operation Enumeration

**User Story:** As a Rust SDK test author, I want to retrieve all child operations nested under a parent context, map, or parallel operation, so that I can inspect the results of individual branches in composite operations.

#### Acceptance Criteria

1. WHEN `get_child_operations()` is called on a Durable_Operation that has child operations, THE Durable_Operation SHALL return a list of Durable_Operation instances whose parent_id matches the parent operation's ID
2. WHEN `get_child_operations()` is called on a Durable_Operation that has no children, THE Durable_Operation SHALL return an empty list
3. WHEN child Durable_Operation instances are returned, each child SHALL support all inspection methods including `get_child_operations()` for nested hierarchies
4. THE Durable_Operation SHALL preserve the execution order of child operations in the returned list

### Requirement 4: WaitingOperationStatus Integration with DurableOperation

**User Story:** As a Rust SDK test author, I want `wait_for_data(status)` on a Durable_Operation to work with all Waiting_Operation_Status variants, so that I can wait for operations to reach Started, Submitted, or Completed states.

#### Acceptance Criteria

1. WHEN `wait_for_data(WaitingOperationStatus::Started)` is called, THE Durable_Operation SHALL resolve when the operation has begun execution
2. WHEN `wait_for_data(WaitingOperationStatus::Submitted)` is called on a callback operation, THE Durable_Operation SHALL resolve when the callback is ready to receive responses (callback_id is available)
3. WHEN `wait_for_data(WaitingOperationStatus::Completed)` is called, THE Durable_Operation SHALL resolve when the operation reaches a terminal status (Succeeded, Failed, Cancelled, Stopped, or Timed_Out)
4. WHEN `wait_for_data()` is called without a status argument, THE Durable_Operation SHALL default to waiting for the Started status
5. IF the operation has already reached the requested status when `wait_for_data()` is called, THEN THE Durable_Operation SHALL resolve immediately

### Requirement 5: Concurrent Callback Testing Pattern

**User Story:** As a Rust SDK test author, I want to use the idiomatic callback testing pattern (pre-register operation, start non-blocking run, wait for callback readiness, send callback response, await final result), so that I can test workflows requiring external callback input.

#### Acceptance Criteria

1. WHEN a developer pre-registers a callback operation handle, starts `run()`, waits for Submitted status, sends a callback success, and awaits the run result, THE Local_Test_Runner SHALL complete the execution with the callback result incorporated
2. WHEN a developer pre-registers a callback operation handle, starts `run()`, waits for Submitted status, and sends a callback failure, THE Local_Test_Runner SHALL propagate the callback failure to the handler
3. WHEN multiple callback operations are active concurrently, THE Local_Test_Runner SHALL allow independent interaction with each callback via separate Operation_Handles
4. IF a callback response is sent before the operation reaches Submitted status, THEN THE Local_Test_Runner SHALL return an error indicating the callback is not ready

### Requirement 6: Reset Between Test Runs

**User Story:** As a Rust SDK test author, I want to reset the test runner state between test runs, so that I can reuse a single runner instance across multiple test cases without state leakage.

#### Acceptance Criteria

1. WHEN `reset()` is called on the Local_Test_Runner, THE Local_Test_Runner SHALL clear all captured operations from previous runs
2. WHEN `reset()` is called on the Local_Test_Runner, THE Local_Test_Runner SHALL clear all pre-registered Operation_Handles
3. WHEN `reset()` is called on the Local_Test_Runner, THE Local_Test_Runner SHALL reset the checkpoint server state
4. WHEN `run()` is called after `reset()`, THE Local_Test_Runner SHALL execute with a clean state equivalent to a newly constructed runner

### Requirement 7: Function Registration for Invoke Operations

**User Story:** As a Rust SDK test author, I want to register durable and regular function handlers by name, so that `context.invoke()` calls within the handler under test can resolve to locally registered functions.

#### Acceptance Criteria

1. WHEN `register_durable_function(name, handler)` is called, THE Local_Test_Runner SHALL store the handler and make it available for `context.invoke()` calls using that name during execution
2. WHEN `register_function(name, handler)` is called, THE Local_Test_Runner SHALL store the non-durable handler and make it available for `context.invoke()` calls using that name during execution
3. WHEN `context.invoke()` is called during handler execution with a registered function name, THE Local_Test_Runner SHALL execute the registered handler and return its result
4. IF `context.invoke()` is called with an unregistered function name, THEN THE Local_Test_Runner SHALL return an error indicating the function is not registered
5. WHEN `clear_registered_functions()` is called, THE Local_Test_Runner SHALL remove all registered function handlers

### Requirement 8: TestResult Ergonomic Accessors

**User Story:** As a Rust SDK test author, I want ergonomic accessors on Test_Result for retrieving the result value and error details, so that common assertion patterns are concise and readable.

#### Acceptance Criteria

1. WHEN `get_result()` is called on a successful Test_Result, THE Test_Result SHALL return a reference to the result value
2. IF `get_result()` is called on a failed Test_Result, THEN THE Test_Result SHALL return an error
3. WHEN `get_error()` is called on a failed Test_Result, THE Test_Result SHALL return a reference to the Test_Result_Error containing error_type, error_message, and optional error_data and stack_trace
4. IF `get_error()` is called on a successful Test_Result, THEN THE Test_Result SHALL return an error indicating the execution succeeded

### Requirement 9: InvokeRequest Input Wrapper

**User Story:** As a Rust SDK test author, I want a structured input wrapper for `run()` that supports optional payload, function name, and region parameters, so that the invocation API matches the Node.js SDK's `InvokeRequest` structure.

#### Acceptance Criteria

1. THE Local_Test_Runner SHALL accept an `InvokeRequest` struct with an optional `payload` field for the `run()` method
2. WHEN `run()` is called with an InvokeRequest containing a payload, THE Local_Test_Runner SHALL pass the payload to the handler function
3. WHEN `run()` is called with an InvokeRequest containing no payload, THE Local_Test_Runner SHALL use a default empty payload
