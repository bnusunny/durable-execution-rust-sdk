# Implementation Plan: Rust Durable Execution SDK Testing Utilities

## Overview

This implementation plan covers creating the testing utilities crate and restructuring the workspace into 3 crates (sdk, testing, examples). Tasks are ordered to build incrementally, with each task building on previous work.

**IMPORTANT**: This plan has been updated to require the same checkpoint server architecture as the Node.js SDK. The LocalDurableTestRunner will use a worker thread-based checkpoint server with full execution state management instead of a simple mock client.

## Tasks

- [x] 1. Restructure workspace into 3 crates
  - [x] 1.1 Create workspace root Cargo.toml with shared dependencies
    - Define workspace members: sdk, testing, examples, macros
    - Configure workspace.package for shared metadata
    - Define workspace.dependencies for shared deps
    - _Requirements: Architecture restructure_

  - [x] 1.2 Move existing SDK code to sdk/ directory
    - Move src/ to sdk/src/
    - Create sdk/Cargo.toml referencing workspace deps
    - Update internal paths and imports
    - _Requirements: Architecture restructure_

  - [x] 1.3 Move existing examples to examples/ crate
    - Create examples/Cargo.toml
    - Move example files to examples/src/bin/
    - Create examples/src/lib.rs for shared utilities
    - Update example imports to use sdk crate
    - _Requirements: Architecture restructure_

  - [x] 1.4 Update macros crate to use workspace dependencies
    - Update macros/Cargo.toml to reference workspace
    - _Requirements: Architecture restructure_

  - [x] 1.5 Update GitHub Actions workflows for new workspace structure
    - Update .github/workflows/pr.yml working directories and cache paths
    - Update .github/workflows/release.yml working directories and publish order
    - Add testing crate to CI checks (check, fmt, clippy, test, doc)
    - Update release workflow to publish sdk, testing, and macros crates in correct order
    - _Requirements: CI/CD compatibility_

- [x] 2. Checkpoint - Verify workspace builds
  - Ensure all tests pass, ask the user if questions arise.

- [x] 3. Create testing crate foundation
  - [x] 3.1 Create testing/Cargo.toml with dependencies
    - Add dependency on sdk crate
    - Add tokio, serde, async-trait, thiserror
    - Add proptest as dev-dependency
    - _Requirements: 1.1_

  - [x] 3.2 Create testing/src/lib.rs with module structure
    - Define public module exports
    - Re-export key types from sdk
    - _Requirements: 1.1_

  - [x] 3.3 Create testing/src/types.rs with core types
    - Define ExecutionStatus enum
    - Define TestResultError struct
    - Define Invocation struct
    - Define WaitingOperationStatus enum
    - _Requirements: 3.1, 3.4, 10.1_

  - [x] 3.4 Create testing/src/error.rs with TestError type
    - Define TestError enum with all variants
    - Implement From conversions for DurableError
    - _Requirements: Error handling_

- [x] 4. Implement MockDurableServiceClient
  - [x] 4.1 Create testing/src/mock_client.rs
    - Implement MockDurableServiceClient struct
    - Add checkpoint_responses and get_operations_responses storage
    - Add checkpoint_calls recording
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [x] 4.2 Implement DurableServiceClient trait for MockDurableServiceClient
    - Implement checkpoint() method with response queue
    - Implement get_operations() method with response queue
    - Record all calls for verification
    - _Requirements: 9.1, 9.2, 9.3_

  - [x] 4.3 Write property test for mock client response ordering
    - **Property 11: Mock Client Response Order**
    - **Validates: Requirements 9.2**

  - [x] 4.4 Write property test for mock client call recording
    - **Property 12: Mock Client Call Recording**
    - **Validates: Requirements 9.3**

- [x] 5. Implement TestResult
  - [x] 5.1 Create testing/src/test_result.rs
    - Implement TestResult struct with status, result, error, operations
    - Implement get_status(), get_result(), get_error() methods
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

  - [x] 5.2 Implement operation retrieval methods
    - Implement get_operations() returning all operations
    - Implement get_operations_by_status() with filtering
    - Implement get_invocations() and get_history_events()
    - _Requirements: 3.5, 3.6, 3.7_

  - [x] 5.3 Implement print methods for debugging
    - Implement print() with default columns
    - Implement print_with_config() with PrintConfig
    - _Requirements: 11.1, 11.2, 11.3_

  - [x] 5.4 Write property test for result retrieval consistency
    - **Property 5: Result Retrieval Consistency**
    - **Validates: Requirements 3.2, 3.3**

  - [x] 5.5 Write property test for operation filtering
    - **Property 6: Operation Filtering Correctness**
    - **Validates: Requirements 3.6**

- [x] 6. Implement DurableOperation
  - [x] 6.1 Create testing/src/operation.rs with DurableOperation struct
    - Define StepDetails, WaitDetails, CallbackDetails, InvokeDetails, ContextDetails
    - Implement basic getters: get_id(), get_name(), get_type(), get_status()
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 6.2 Implement type-specific detail methods
    - Implement get_step_details() with type checking
    - Implement get_wait_details() with type checking
    - Implement get_callback_details() with type checking
    - Implement get_invoke_details() with type checking
    - Implement get_context_details() with type checking
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [x] 6.3 Implement callback interaction methods
    - Implement send_callback_success()
    - Implement send_callback_failure()
    - Implement send_callback_heartbeat()
    - Add type checking to return error for non-callback operations
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [x] 6.4 Implement wait_for_data() async method
    - Implement waiting for Started status
    - Implement waiting for Submitted status (callbacks)
    - Implement waiting for Completed status
    - Handle execution completion before target state
    - _Requirements: 10.1, 10.2, 10.3, 10.4_

  - [x] 6.5 Write property test for type-specific details
    - **Property 8: Type-Specific Details Availability**
    - **Validates: Requirements 5.1, 5.2, 5.3, 5.4, 5.5, 5.6**

  - [x] 6.6 Write property test for callback method type safety
    - **Property 9: Callback Method Type Safety**
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.4**

- [x] 7. Checkpoint - Verify core types
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7.5. Implement Checkpoint Server (Node.js SDK Parity) - NEW
  - [x] 7.5.1 Create checkpoint_server module structure
    - Create testing/src/checkpoint_server/mod.rs
    - Create testing/src/checkpoint_server/types.rs with shared types
    - Define WorkerRequest, WorkerResponse, ApiType enums
    - _Requirements: 9.1, 13.1_

  - [x] 7.5.2 Implement CheckpointToken utilities
    - Create testing/src/checkpoint_server/checkpoint_token.rs
    - Implement CheckpointTokenData struct
    - Implement encode_checkpoint_token() function
    - Implement decode_checkpoint_token() function
    - _Requirements: 10.1_

  - [x] 7.5.3 Implement EventProcessor
    - Create testing/src/checkpoint_server/event_processor.rs
    - Implement EventProcessor struct with event storage
    - Implement create_history_event() method
    - Implement get_events() and clear() methods
    - _Requirements: 12.1, 12.2, 12.3, 12.4_

  - [x] 7.5.4 Implement CallbackManager
    - Create testing/src/checkpoint_server/callback_manager.rs
    - Implement CallbackManager struct with callback state tracking
    - Implement register_callback() method
    - Implement send_success(), send_failure(), send_heartbeat() methods
    - Implement check_timeouts() method
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [x] 7.5.5 Implement CheckpointManager
    - Create testing/src/checkpoint_server/checkpoint_manager.rs
    - Implement CheckpointManager struct with operation state
    - Implement initialize() method for first operation
    - Implement start_invocation() and complete_invocation() methods
    - Implement process_checkpoint() method for operation updates
    - Implement get_state() and get_dirty_operations() methods
    - Integrate with CallbackManager and EventProcessor
    - _Requirements: 10.2, 10.3, 10.4, 10.5_

  - [x] 7.5.6 Implement ExecutionManager
    - Create testing/src/checkpoint_server/execution_manager.rs
    - Implement ExecutionManager struct with execution storage
    - Implement start_execution() method
    - Implement start_invocation() method
    - Implement complete_invocation() method
    - Implement get_checkpoints_by_execution() and get_checkpoints_by_token() methods
    - _Requirements: 10.1, 10.2_

  - [x] 7.5.7 Implement CheckpointWorkerManager
    - Create testing/src/checkpoint_server/worker_manager.rs
    - Implement CheckpointWorkerManager struct with thread handle and channels
    - Implement get_instance() singleton pattern
    - Implement setup() to spawn checkpoint server thread
    - Implement send_api_request() for thread communication
    - Implement teardown() for graceful shutdown
    - Implement DurableServiceClient trait
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 13.1, 13.2, 13.3, 13.4_

  - [ ] 7.5.8 Write property test for checkpoint server thread lifecycle
    - **Property 11: Checkpoint Server Thread Lifecycle**
    - **Validates: Requirements 9.1, 9.2, 9.5, 9.6**

  - [ ] 7.5.9 Write property test for execution state consistency
    - **Property 12: Execution State Consistency**
    - **Validates: Requirements 10.1, 10.2, 10.3**

  - [ ] 7.5.10 Write property test for callback lifecycle
    - **Property 13: Callback Lifecycle Correctness**
    - **Validates: Requirements 11.1, 11.4, 11.5**

  - [ ] 7.5.11 Write property test for history event generation
    - **Property 14: History Event Generation**
    - **Validates: Requirements 12.1, 12.2, 12.3, 12.4**

  - [ ] 7.5.12 Write property test for thread communication
    - **Property 15: Thread Communication Reliability**
    - **Validates: Requirements 13.1, 13.2, 13.3, 13.4**

- [x] 7.6. Checkpoint - Verify checkpoint server
  - Ensure all checkpoint server tests pass, ask the user if questions arise.

- [-] 7.7. Implement Test Execution Orchestrator (Node.js SDK Parity) - NEW
  - [x] 7.7.1 Create Scheduler trait and implementations
    - Create testing/src/checkpoint_server/scheduler.rs
    - Define Scheduler trait with schedule_function(), has_scheduled_function(), flush_timers()
    - Implement QueueScheduler for time-skipping mode (FIFO order, ignores timestamps)
    - Implement TimerScheduler for real-time mode (respects timestamps using tokio timers)
    - _Requirements: 17.1, 17.2, 17.3, 17.4, 17.5_

  - [x] 7.7.2 Create TestExecutionOrchestrator struct
    - Create testing/src/checkpoint_server/orchestrator.rs
    - Define TestExecutionOrchestrator with handler, operation_storage, checkpoint_api, scheduler
    - Define SkipTimeConfig for time skipping configuration
    - Define TestExecutionResult for execution outcomes
    - _Requirements: 16.1, 16.2_

  - [x] 7.7.3 Implement execute_handler() method
    - Start execution via checkpoint API
    - Begin polling for checkpoint updates (spawn task)
    - Schedule initial handler invocation
    - Wait for execution completion
    - Return TestExecutionResult
    - _Requirements: 16.4, 16.5, 18.1_

  - [x] 7.7.4 Implement poll_for_checkpoint_data() method
    - Continuously poll checkpoint API for operation updates
    - Process each operation update via process_operations()
    - Stop polling when execution completes or abort signal received
    - _Requirements: 18.1, 18.2, 18.6_

  - [x] 7.7.5 Implement operation processing methods
    - Implement process_operations() to iterate over operation updates
    - Implement process_operation() to dispatch based on operation type
    - Implement handle_wait_update() to schedule re-invocation at wait end time
    - Implement handle_step_update() to schedule retry at next attempt time
    - Implement handle_callback_update() to schedule re-invocation when callback completes
    - Implement handle_execution_update() to resolve execution
    - _Requirements: 18.2, 18.3, 18.4, 18.5_

  - [x] 7.7.6 Implement schedule_invocation_at_timestamp() method
    - Schedule handler re-invocation via scheduler
    - When time skipping enabled, advance tokio time before invocation
    - Update checkpoint data (mark wait as SUCCEEDED) before re-invoking
    - _Requirements: 16.2, 16.3, 17.3_

  - [x] 7.7.7 Implement invoke_handler() method
    - Check for active invocations (prevent concurrent invocations in time-skip mode)
    - Start invocation via checkpoint API
    - Invoke handler with checkpoint token and operations
    - Process handler result (PENDING, SUCCEEDED, FAILED)
    - Schedule re-invocation if dirty operations exist
    - _Requirements: 16.4, 16.5, 16.6_

  - [-] 7.7.8 Write property test for wait operation completion
    - **Property 19: Wait Operation Completion (Orchestrator)**
    - **Validates: Requirements 16.1, 16.2, 16.3**

  - [ ] 7.7.9 Write property test for execution lifecycle orchestration
    - **Property 20: Execution Lifecycle Orchestration**
    - **Validates: Requirements 16.4, 16.5**

  - [ ] 7.7.10 Write property test for scheduler FIFO order
    - **Property 21: Scheduler FIFO Order (Time Skipping)**
    - **Validates: Requirements 17.1, 17.3**

  - [ ] 7.7.11 Write property test for checkpoint polling continuity
    - **Property 22: Checkpoint Polling Continuity**
    - **Validates: Requirements 18.1, 18.6**

- [ ] 7.8. Checkpoint - Verify orchestrator
  - Ensure all orchestrator tests pass, ask the user if questions arise.

- [x] 8. Implement LocalDurableTestRunner
  - [x] 8.1 Create testing/src/local_runner.rs with struct definition
    - Define LocalDurableTestRunner with handler, mock_client, operation_storage
    - Implement new() constructor
    - _Requirements: 1.1_
    - **NOTE: This task needs to be updated to use CheckpointWorkerManager instead of mock_client**

  - [x] 8.1.1 Update LocalDurableTestRunner to use CheckpointWorkerManager - NEW
    - Replace mock_client with checkpoint_worker: Arc<CheckpointWorkerManager>
    - Update new() to get CheckpointWorkerManager instance
    - Update all checkpoint calls to use the worker manager
    - _Requirements: 9.1, 9.2_

  - [x] 8.1.2 Integrate TestExecutionOrchestrator into LocalDurableTestRunner - NEW
    - Create TestExecutionOrchestrator in run() method
    - Configure SkipTimeConfig based on test environment settings
    - Delegate execution to orchestrator.execute_handler()
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5_

  - [x] 8.2 Implement test environment setup/teardown
    - Implement setup_test_environment() with time control
    - Implement teardown_test_environment()
    - Use tokio::time::pause() for time skipping
    - _Requirements: 2.1, 2.2, 2.3, 2.4_
    - **NOTE: This task needs to be updated to spawn/teardown checkpoint server thread**

  - [x] 8.2.1 Update setup/teardown to manage checkpoint server - NEW
    - Update setup_test_environment() to spawn checkpoint server thread via CheckpointWorkerManager
    - Update teardown_test_environment() to gracefully shutdown checkpoint server
    - Note: Checkpoint server is spawned automatically via singleton pattern when LocalDurableTestRunner is created
    - _Requirements: 9.1, 9.5_

  - [x] 8.3 Implement run() method
    - Execute handler with mock client
    - Capture operations during execution
    - Build and return TestResult
    - _Requirements: 1.2, 1.3, 1.4, 1.5_
    - **NOTE: This task needs to be updated to use checkpoint server for realistic execution**

  - [x] 8.3.1 Update run() to use checkpoint server - NEW
    - Start execution via CheckpointWorkerManager
    - Process checkpoints through the checkpoint server thread
    - Retrieve history events from checkpoint server
    - Build TestResult with full execution state
    - _Requirements: 9.2, 10.1, 10.2, 12.5_

  - [x] 8.3.2 Update run() to use TestExecutionOrchestrator - NEW
    - Create orchestrator with appropriate scheduler (QueueScheduler for time-skip, TimerScheduler otherwise)
    - Call orchestrator.execute_handler() instead of direct handler invocation
    - Orchestrator handles polling, wait completion, and re-invocation automatically
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5, 16.6_

  - [x] 8.4 Implement reset() method
    - Clear operation storage
    - Reset mock client state
    - _Requirements: 1.6_
    - **NOTE: This task needs to be updated to reset checkpoint server state**

  - [x] 8.4.1 Update reset() to clear checkpoint server state - NEW
    - Clear execution state in checkpoint server
    - Reset CheckpointWorkerManager for new execution
    - _Requirements: 1.6_
    - Clear execution state in checkpoint server
    - Reset CheckpointWorkerManager for new execution
    - _Requirements: 1.6_

  - [x] 8.5 Implement operation lookup methods
    - Implement get_operation(name)
    - Implement get_operation_by_index(index)
    - Implement get_operation_by_name_and_index(name, index)
    - Implement get_operation_by_id(id)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 8.6 Implement function registration
    - Implement register_durable_function()
    - Implement register_function()
    - Store registered functions for invoke handling
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [x] 8.7 Write property test for execution status consistency
    - **Property 1: Execution Status Consistency**
    - **Validates: Requirements 1.3, 1.4**

  - [x] 8.8 Write property test for operation capture completeness
    - **Property 2: Operation Capture Completeness**
    - **Validates: Requirements 1.5, 3.5**

  - [x] 8.9 Write property test for reset clears state
    - **Property 3: Reset Clears State**
    - **Validates: Requirements 1.6**

  - [x] 8.10 Write property test for time skipping
    - **Property 4: Time Skipping Acceleration**
    - **Validates: Requirements 2.1, 2.2**

  - [x] 8.11 Write property test for operation lookup
    - **Property 7: Operation Lookup Consistency**
    - **Validates: Requirements 4.1, 4.2, 4.3, 4.4**

  - [x] 8.12 Write property test for function registration
    - **Property 10: Function Registration Retrieval**
    - **Validates: Requirements 7.1, 7.2, 7.3**

- [x] 9. Checkpoint - Verify local runner
  - Ensure all tests pass, ask the user if questions arise.

- [x] 10. Implement CloudDurableTestRunner
  - [x] 10.1 Create testing/src/cloud_runner.rs with struct definition
    - Define CloudDurableTestRunner with function_name, lambda_client, config
    - Define CloudTestRunnerConfig with poll_interval, timeout
    - _Requirements: 8.1, 8.4, 8.5_

  - [x] 10.2 Implement constructors
    - Implement new() with default AWS config
    - Implement with_client() for custom Lambda client
    - Implement with_config() for configuration
    - _Requirements: 8.1, 8.4, 8.5_

  - [x] 10.3 Implement run() method
    - Invoke Lambda function with payload
    - Poll for execution completion
    - Build and return TestResult
    - _Requirements: 8.2, 8.3_

  - [x] 10.4 Implement operation lookup methods
    - Implement get_operation(name)
    - Implement get_operation_by_index(index)
    - Implement get_operation_by_name_and_index(name, index)
    - Implement get_operation_by_id(id)
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

- [x] 11. Implement time control utilities
  - [x] 11.1 Create testing/src/time_control.rs
    - Implement TimeControl struct for managing fake time
    - Integrate with tokio::time::pause/resume
    - Added is_time_paused() helper to handle concurrent test scenarios
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

- [x] 12. Create testing crate README
  - [x] 12.1 Write testing/README.md with documentation
    - Add installation instructions
    - Add quick start examples
    - Document LocalDurableTestRunner API
    - Document CloudDurableTestRunner API
    - Document TestResult and DurableOperation APIs
    - _Requirements: Documentation_

- [-] 13. Add tests for existing examples using testing utilities (following Node.js SDK pattern)
  - [x] 13.1 Add testing crate as dev-dependency to examples/Cargo.toml
    - Add aws-durable-execution-sdk-testing dependency
    - Add serde_json for history file serialization
    - _Requirements: Examples_

  - [x] 13.2 Create test helper module for examples
    - Create examples/src/test_helper.rs with shared test utilities
    - Implement assert_event_signatures() to compare history events against saved JSON
    - Support generating history files with GENERATE_HISTORY=true env var
    - Implement EventSignature struct for comparing events (EventType, SubType, Name)
    - Store history files as .history.json alongside each example
    - _Requirements: Examples_

  - [x] 13.3 Add tests for step examples
    - Create tests for step/basic, step/named, step/with_config
    - Each test uses LocalDurableTestRunner to execute the handler
    - Each test calls assert_event_signatures() to validate history
    - Generate .history.json files for each example
    - _Requirements: Examples_

  - [x] 13.4 Add tests for wait examples
    - Create tests for wait/basic, wait/named, wait/extended_duration
    - Create tests for wait_for_condition/basic
    - Create tests for multiple_waits
    - Each test uses LocalDurableTestRunner with time skipping enabled
    - Generate .history.json files for each example
    - **Full end-to-end wait completion testing requires tasks 7.7.x (TestExecutionOrchestrator) and 8.3.2 to be implemented.**
    - _Requirements: Examples_

  - [x] 13.4.1 Update wait tests for full end-to-end completion - NEW
    - Update tests to verify wait operations complete (status changes to SUCCEEDED)
    - Verify handler is re-invoked after wait completes
    - Verify execution completes with final result after all waits
    - Test time skipping advances time correctly for wait durations
    - **Depends on: 7.7.x (TestExecutionOrchestrator), 8.3.2**
    - _Requirements: 16.2, 16.3, Examples_

  - [x] 13.5 Add tests for callback examples
    - Create tests for callback/simple, callback/concurrent, callback/with_timeout
    - Create tests for callback/wait_for_callback
    - Each test uses send_callback_success/failure to simulate external responses
    - Generate .history.json files for each example
    - _Requirements: Examples_

  - [x] 13.6 Add tests for parallel and map examples
    - Create tests for parallel/basic, parallel/heterogeneous, parallel/first_successful
    - Create tests for map/basic, map/with_concurrency, map/failure_tolerance, map/min_successful
    - Verify correct concurrency and completion behavior
    - Generate .history.json files for each example
    - _Requirements: Examples_

  - [x] 13.7 Add tests for promise combinator examples
    - Create tests for promise_combinators/all, all_settled, any, race
    - Verify correct combinator semantics
    - Generate .history.json files for each example
    - _Requirements: Examples_

  - [x] 13.8 Add tests for remaining examples
    - Create tests for hello_world, child_context/basic, child_context/nested
    - Create tests for invoke/basic, serde/custom_serialization
    - Create tests for error_handling/step_error, comprehensive/order_workflow
    - Generate .history.json files for each example
    - _Requirements: Examples_

- [x] 14. Final checkpoint - Full integration test
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- All tasks including property-based tests are required for comprehensive coverage
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- **NEW**: Tasks 7.5.x implement the checkpoint server architecture matching the Node.js SDK
- **NEW**: Tasks 8.x.1 update the LocalDurableTestRunner to use the checkpoint server instead of simple mock client
- The MockDurableServiceClient is retained for simple unit testing scenarios but is not used by LocalDurableTestRunner
