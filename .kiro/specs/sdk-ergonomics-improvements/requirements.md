# Requirements Document

## Introduction

This document specifies requirements for ergonomic improvements to the AWS Durable Execution SDK for Rust. These improvements address gaps in the current implementation related to callback convenience methods, map operation batching, and logging ergonomics. The goal is to bring the Rust SDK closer to parity with the Python SDK's developer experience while maintaining idiomatic Rust patterns.

## Glossary

- **wait_for_callback**: A convenience method that combines callback creation with a submitter function, allowing external systems to be notified in a single operation
- **ItemBatcher**: Configuration for batching items in map operations, supporting both count-based and size-based batching
- **Tracing_Span**: A structured logging span from the `tracing` crate that provides hierarchical context for operations
- **Structured_Fields**: Key-value pairs attached to log messages that can be queried and filtered
- **Replay_Aware_Logging**: Logging that can distinguish between fresh execution and replay, optionally suppressing logs during replay

## Requirements

### Requirement 1: Enhanced wait_for_callback Method

**User Story:** As a Rust developer, I want a more ergonomic wait_for_callback method that properly handles the submitter function within a durable step, so that my callback submissions are checkpointed and replay-safe.

#### Acceptance Criteria

1. WHEN wait_for_callback is called, THE SDK SHALL create a callback and execute the submitter function within a durable step
2. THE wait_for_callback method SHALL checkpoint the submitter execution to ensure replay safety
3. IF the submitter function fails, THEN THE SDK SHALL propagate the error with appropriate context
4. THE wait_for_callback method SHALL support configurable callback timeout and heartbeat timeout
5. THE wait_for_callback method SHALL return the callback result after the external system signals completion
6. THE wait_for_callback method SHALL support generic result types that implement Serialize and DeserializeOwned

### Requirement 2: ItemBatcher for Map Operations

**User Story:** As a Rust developer, I want to batch items in map operations, so that I can process large collections efficiently with reduced checkpoint overhead.

#### Acceptance Criteria

1. THE ItemBatcher SHALL support configuring maximum items per batch
2. THE ItemBatcher SHALL support configuring maximum bytes per batch
3. WHEN ItemBatcher is configured, THE map operation SHALL group items into batches before processing
4. THE map operation SHALL process each batch as a single child context operation
5. THE ItemBatcher SHALL provide a default configuration with reasonable limits (100 items, 256KB)
6. WHEN batch size limits are exceeded, THE ItemBatcher SHALL split items into multiple batches
7. THE ItemBatcher SHALL preserve item ordering within and across batches

### Requirement 3: Tracing Spans for Operations

**User Story:** As a Rust developer, I want tracing spans for durable operations, so that I can trace execution flow and correlate logs across operations.

#### Acceptance Criteria

1. THE Logging_System SHALL create a tracing span for each durable operation (step, wait, callback, invoke, map, parallel)
2. THE tracing span SHALL include the operation_id as a structured field
3. THE tracing span SHALL include the operation_type as a structured field
4. THE tracing span SHALL include the parent_id as a structured field when available
5. THE tracing span SHALL include the durable_execution_arn as a structured field
6. WHEN an operation completes, THE tracing span SHALL be closed with the operation status

### Requirement 4: Simplified Logging API

**User Story:** As a Rust developer, I want a simpler logging API that matches Python's ergonomics, so that I can log messages with less boilerplate.

#### Acceptance Criteria

1. THE DurableContext SHALL provide a log_info method that logs at INFO level with automatic context
2. THE DurableContext SHALL provide a log_debug method that logs at DEBUG level with automatic context
3. THE DurableContext SHALL provide a log_warn method that logs at WARN level with automatic context
4. THE DurableContext SHALL provide a log_error method that logs at ERROR level with automatic context
5. THE logging methods SHALL automatically include durable_execution_arn, operation_id, and parent_id
6. THE logging methods SHALL support additional structured fields via a builder pattern or variadic arguments

### Requirement 5: Extra Fields in Tracing Output

**User Story:** As a Rust developer, I want extra fields from LogInfo to be passed through to tracing output, so that I can include custom context in my logs.

#### Acceptance Criteria

1. WHEN LogInfo contains extra fields, THE TracingLogger SHALL include them in the tracing output
2. THE extra fields SHALL be formatted as key-value pairs in the tracing event
3. THE extra fields SHALL be queryable in log aggregation systems
4. THE SDK SHALL document how to add extra fields to log messages

### Requirement 6: Tracing Best Practices Documentation

**User Story:** As a Rust developer, I want documentation on tracing best practices, so that I can effectively monitor and debug my durable workflows.

#### Acceptance Criteria

1. THE Documentation SHALL explain how to configure tracing for Lambda
2. THE Documentation SHALL explain how to correlate logs across operations
3. THE Documentation SHALL explain how to use structured fields for filtering
4. THE Documentation SHALL explain replay-aware logging configuration
5. THE Documentation SHALL provide examples of common tracing patterns

