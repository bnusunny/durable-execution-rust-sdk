# Implementation Plan: SDK Ergonomics Improvements

## Overview

This implementation plan breaks down the SDK ergonomics improvements into incremental tasks. The approach starts with the ItemBatcher enhancements (simplest), then moves to logging improvements, and finally the wait_for_callback enhancement. Each task builds on previous work and includes property-based tests for validation.

## Tasks

- [x] 1. Enhance ItemBatcher with byte-based batching
  - [x] 1.1 Add byte-based batching to ItemBatcher
    - Add `batch<T: Serialize>()` method that respects both item count and byte limits
    - Estimate item size using serde_json serialization
    - Ensure batches never exceed configured limits
    - _Requirements: 2.1, 2.2, 2.3, 2.6_

  - [x] 1.2 Write property test for ItemBatcher batching constraints
    - **Property 5: ItemBatcher Configuration Respected**
    - **Validates: Requirements 2.1, 2.2**

  - [x] 1.3 Write property test for ItemBatcher ordering preservation
    - **Property 6: ItemBatcher Ordering Preservation**
    - **Validates: Requirements 2.3, 2.4, 2.6, 2.7**

  - [x] 1.4 Update map handler to use enhanced ItemBatcher
    - Integrate byte-based batching in batch_items function
    - Process each batch as a single child context operation
    - _Requirements: 2.4_

- [x] 2. Checkpoint - Ensure ItemBatcher tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 3. Implement tracing spans for operations
  - [x] 3.1 Create operation span helper function
    - Add `create_operation_span()` function in context module
    - Include operation_type, operation_id, parent_id, durable_execution_arn fields
    - Return tracing::Span for use in handlers
    - _Requirements: 3.2, 3.3, 3.4, 3.5_

  - [x] 3.2 Add tracing spans to step handler
    - Create span at start of step_handler
    - Enter span during execution
    - Record status on completion
    - _Requirements: 3.1, 3.6_

  - [x] 3.3 Add tracing spans to wait handler
    - Create span at start of wait_handler
    - Enter span during execution
    - Record status on completion
    - _Requirements: 3.1, 3.6_

  - [x] 3.4 Add tracing spans to callback handler
    - Create span at start of callback_handler
    - Enter span during execution
    - Record status on completion
    - _Requirements: 3.1, 3.6_

  - [x] 3.5 Add tracing spans to invoke handler
    - Create span at start of invoke_handler
    - Enter span during execution
    - Record status on completion
    - _Requirements: 3.1, 3.6_

  - [x] 3.6 Add tracing spans to map and parallel handlers
    - Create span at start of map_handler and parallel_handler
    - Enter span during execution
    - Record status on completion
    - _Requirements: 3.1, 3.6_

  - [x] 3.7 Write property test for tracing span fields
    - **Property 7: Tracing Span Fields**
    - **Validates: Requirements 3.2, 3.3, 3.4, 3.5**

- [x] 4. Checkpoint - Ensure tracing span tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement simplified logging API
  - [x] 5.1 Add log_info, log_debug, log_warn, log_error methods to DurableContext
    - Implement log_with_level helper method
    - Add log_info, log_debug, log_warn, log_error convenience methods
    - Automatically include durable_execution_arn and parent_id
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 5.2 Add log_*_with methods for extra fields
    - Implement log_info_with, log_debug_with, log_warn_with, log_error_with
    - Accept slice of (key, value) tuples for extra fields
    - _Requirements: 4.6_

  - [x] 5.3 Write property test for logging automatic context
    - **Property 9: Logging Methods Automatic Context**
    - **Validates: Requirements 4.5**

  - [x] 5.4 Write property test for logging extra fields
    - **Property 10: Logging Methods Extra Fields**
    - **Validates: Requirements 4.6**

- [x] 6. Implement extra fields passthrough in TracingLogger
  - [x] 6.1 Update TracingLogger to include extra fields
    - Modify debug, info, warn, error methods to include extra fields
    - Format extra fields as key-value pairs in tracing output
    - _Requirements: 5.1, 5.2_

  - [x] 6.2 Write property test for extra field passthrough
    - **Property 11: TracingLogger Extra Field Passthrough**
    - **Validates: Requirements 5.1, 5.2**

- [x] 7. Checkpoint - Ensure logging tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Enhance wait_for_callback method
  - [x] 8.1 Refactor wait_for_callback to use child context for submitter
    - Create child context for the wait_for_callback operation
    - Execute submitter within a step inside the child context
    - Ensure submitter is not re-executed during replay
    - _Requirements: 1.1, 1.2_

  - [x] 8.2 Implement error propagation for submitter failures
    - Catch submitter errors and wrap in DurableError::UserCode
    - Set error_type to "SubmitterError"
    - Preserve original error message
    - _Requirements: 1.3_

  - [x] 8.3 Ensure configuration passthrough
    - Pass CallbackConfig to create_callback
    - Verify timeout and heartbeat_timeout are respected
    - _Requirements: 1.4_

  - [x] 8.4 Write property test for wait_for_callback checkpointing
    - **Property 1: wait_for_callback Checkpoints Submitter Execution**
    - **Validates: Requirements 1.1, 1.2**

  - [x] 8.5 Write property test for wait_for_callback error propagation
    - **Property 2: wait_for_callback Error Propagation**
    - **Validates: Requirements 1.3**

  - [x] 8.6 Write property test for wait_for_callback configuration
    - **Property 3: wait_for_callback Configuration Passthrough**
    - **Validates: Requirements 1.4**

- [x] 9. Checkpoint - Ensure wait_for_callback tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 10. Add tracing best practices documentation
  - [x] 10.1 Create TRACING.md documentation file
    - Document how to configure tracing for Lambda
    - Explain log correlation across operations
    - Describe structured field usage for filtering
    - Cover replay-aware logging configuration
    - Provide common tracing pattern examples
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [x] 10.2 Update lib.rs documentation
    - Add tracing section to crate-level docs
    - Reference TRACING.md for detailed guidance
    - Add examples of logging API usage
    - _Requirements: 5.4_

- [x] 11. Final checkpoint - All improvements complete
  - Ensure all tests pass
  - Verify documentation is complete
  - Ask the user if questions arise

## Notes

- All tasks including property-based tests are required for comprehensive validation
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties
- Unit tests validate specific examples and edge cases
- The implementation order (ItemBatcher → Logging → wait_for_callback) minimizes dependencies

