# Implementation Tasks: Rust Event History Format Alignment

## Overview
This task list implements the changes needed to align the Rust SDK's event history output format with the Node.js SDK format, enabling cross-SDK history comparison.

## Tasks

### 1. Create Node.js-Compatible Event Types
- [x] 1.1 Create `nodejs_event_types.rs` module with `NodeJsEventType` enum containing all event types (ExecutionStarted, ExecutionSucceeded, ExecutionFailed, StepStarted, StepSucceeded, StepFailed, WaitStarted, WaitSucceeded, CallbackStarted, ContextStarted, ContextSucceeded, ContextFailed, InvocationCompleted)
  - **Validates: Requirements 2.1-2.12**
- [x] 1.2 Create `NodeJsHistoryEvent` struct with PascalCase field names (EventType, EventId, Id, EventTimestamp, SubType, Name, ParentId) using `#[serde(rename_all = "PascalCase")]`
  - **Validates: Requirements 1.1-1.7**
- [x] 1.3 Create `PayloadWrapper` struct with `Payload` field for wrapping input/result/error payloads
  - **Validates: Requirements 3.1-3.11**
- [x] 1.4 Create `RetryDetails` struct with `CurrentAttempt` and `NextAttemptDelaySeconds` fields
  - **Validates: Requirements 7.3, 7.4**

### 2. Create Event Detail Structures
- [x] 2.1 Create `ExecutionStartedDetails` struct with `Input` (PayloadWrapper) and optional `ExecutionTimeout` fields
  - **Validates: Requirement 3.1**
- [x] 2.2 Create `ExecutionSucceededDetails` struct with `Result` (PayloadWrapper) field
  - **Validates: Requirement 3.2**
- [x] 2.3 Create `ExecutionFailedDetails` struct with `Error` (PayloadWrapper) field
  - **Validates: Requirement 3.3**
- [x] 2.4 Create `StepStartedDetails` struct (may be empty object)
  - **Validates: Requirement 3.4**
- [x] 2.5 Create `StepSucceededDetails` struct with `Result` (PayloadWrapper) and `RetryDetails` fields
  - **Validates: Requirement 3.5**
- [x] 2.6 Create `StepFailedDetails` struct with `Error` (PayloadWrapper) and `RetryDetails` fields
  - **Validates: Requirement 3.6**
- [x] 2.7 Create `WaitStartedDetails` struct with `Duration` and `ScheduledEndTimestamp` fields
  - **Validates: Requirement 3.7**
- [x] 2.8 Create `WaitSucceededDetails` struct (may be empty object)
  - **Validates: Requirement 2.8**
- [x] 2.9 Create `CallbackStartedDetails` struct with `CallbackId`, `Timeout`, `HeartbeatTimeout`, and `Input` (PayloadWrapper) fields
  - **Validates: Requirement 3.8**
- [x] 2.10 Create `ContextStartedDetails`, `ContextSucceededDetails`, and `ContextFailedDetails` structs
  - **Validates: Requirements 3.9-3.11**
- [x] 2.11 Create `InvocationCompletedDetails` struct with `StartTimestamp`, `EndTimestamp`, `RequestId`, and `Error` fields
  - **Validates: Requirements 4.2-4.6**
- [x] 2.12 Create `NodeJsEventDetails` enum with variants for each detail type using `#[serde(flatten)]` for proper serialization
  - **Validates: Requirements 3.1-3.11**

### 3. Update EventProcessor for Node.js-Compatible Events
- [x] 3.1 Add `create_nodejs_event` method to EventProcessor that generates events with sequential EventId starting from 1
  - **Validates: Requirements 1.2, 1.3, 1.4**
- [x] 3.2 Add `process_operation_update` method that maps (OperationAction, OperationType) to appropriate NodeJsEventType
  - **Validates: Requirements 2.1-2.12**
- [x] 3.3 Implement timestamp formatting as ISO 8601 strings (e.g., "2025-12-03T22:58:35.094Z")
  - **Validates: Requirements 1.4, 5.5**
- [x] 3.4 Add storage for Node.js-compatible events alongside existing events
  - **Validates: Requirement 5.3**
- [x] 3.5 Export new types from `checkpoint_server/mod.rs`
  - **Validates: Requirements 1.1-1.7**

### 4. Update CheckpointManager for Event Generation
- [x] 4.1 Update `initialize` method to generate `ExecutionStarted` event with `ExecutionStartedDetails`
  - **Validates: Requirements 2.1, 3.1**
- [x] 4.2 Update `process_operation_update` to generate Started/Succeeded/Failed events based on action and type
  - **Validates: Requirements 2.1-2.12**
- [x] 4.3 Update `complete_invocation` to generate `InvocationCompleted` event with proper details
  - **Validates: Requirements 4.1-4.6**
- [x] 4.4 Add method to retrieve Node.js-compatible history events
  - **Validates: Requirement 5.3**
- [x] 4.5 Handle retry operations to generate appropriate StepSucceeded/StepFailed events with RetryDetails
  - **Validates: Requirements 7.1-7.4**

### 5. Update TestResult for Node.js History Events
- [x] 5.1 Add `NodeJsHistoryEvent` storage to `TestResult` struct
  - **Validates: Requirement 5.3**
- [x] 5.2 Add `get_nodejs_history_events` method to retrieve Node.js-compatible events
  - **Validates: Requirement 5.3**
- [x] 5.3 Update orchestrator to populate Node.js history events in TestResult
  - **Validates: Requirement 5.3**

### 6. Update Test Helper for Node.js-Compatible History Files
- [x] 6.1 Create `NodeJsHistoryFile` type alias for `Vec<NodeJsHistoryEvent>` (flat JSON array)
  - **Validates: Requirements 5.1, 5.2**
- [x] 6.2 Add `extract_nodejs_events` function to convert operations to Node.js-compatible events
  - **Validates: Requirements 5.1-5.5**
- [x] 6.3 Add `assert_nodejs_event_signatures` function that compares events by EventType, SubType, and Name (ignoring volatile fields)
  - **Validates: Requirement 6.4**
- [x] 6.4 Update history file generation to output flat JSON array format when GENERATE_HISTORY=true
  - **Validates: Requirements 6.1-6.3**

### 7. Update Existing History Files
- [x] 7.1 Regenerate `hello_world.history.json` in Node.js-compatible format
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.2 Regenerate all step-related history files (step_basic, step_named, step_with_config, step_error_success, step_error_failure)
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.3 Regenerate all wait-related history files (wait_basic, wait_named, wait_extended, wait_for_callback, wait_for_condition_basic, multiple_waits)
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.4 Regenerate all callback-related history files (callback_simple, callback_concurrent, callback_with_timeout)
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.5 Regenerate all parallel/map-related history files (parallel_basic, parallel_first_successful, parallel_heterogeneous, map_basic, map_with_concurrency, map_failure_tolerance, map_min_successful)
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.6 Regenerate all promise combinator history files (promise_all, promise_any, promise_race, promise_all_settled, promise_all_macro, promise_any_macro, promise_race_macro, promise_all_settled_macro, promise_race_timeout, promise_all_with_map)
  - **Validates: Requirements 5.1-5.5, 6.2**
- [x] 7.7 Regenerate remaining history files (invoke_basic, child_context_basic, child_context_nested, custom_serialization, order_workflow_no_approval, order_workflow_with_approval)
  - **Validates: Requirements 5.1-5.5, 6.2**

### 8. Property-Based Tests for Correctness Properties
- [x] 8.1 [PBT] Write property test for Event ID Monotonicity - verify EventIds are strictly increasing starting from 1
  - **Validates: Correctness Property 1, Requirement 1.2**
- [x] 8.2 [PBT] Write property test for Event Type Mapping Consistency - verify (Action, Type) → EventType mapping is correct
  - **Validates: Correctness Property 2, Requirements 2.1-2.12**
- [x] 8.3 [PBT] Write property test for Timestamp Format Validity - verify all timestamps match ISO 8601 format
  - **Validates: Correctness Property 3, Requirements 1.4, 4.3, 4.4, 5.5**
- [x] 8.4 [PBT] Write property test for PascalCase Field Names - verify all serialized JSON keys are PascalCase
  - **Validates: Correctness Property 4, Requirements 1.1, 5.4**
- [x] 8.5 [PBT] Write property test for JSON Array Output Format - verify history output is flat JSON array without wrapper
  - **Validates: Correctness Property 5, Requirements 5.1, 5.2, 5.3**

### 9. Integration Tests
- [x] 9.1 Update existing example tests to use new Node.js-compatible history comparison
  - **Validates: Requirements 6.1-6.4**
- [x] 9.2 Add integration test verifying cross-SDK history format compatibility
  - **Validates: Requirement 6.3**
