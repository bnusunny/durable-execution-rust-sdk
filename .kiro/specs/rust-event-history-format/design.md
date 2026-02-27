# Design Document: Rust Event History Format Alignment

## Overview

This document describes the design for aligning the Rust durable execution SDK's event history output format with the Node.js SDK format. The goal is to enable cross-SDK history comparison and ensure consistent behavior across both SDKs.

## Current State Analysis

### Node.js SDK Event Format

The Node.js SDK generates a flat JSON array of history events with the following structure:

```json
[
  {
    "EventType": "ExecutionStarted",
    "EventId": 1,
    "Id": "uuid-of-execution",
    "EventTimestamp": "2025-12-03T22:58:35.094Z",
    "ExecutionStartedDetails": {
      "Input": { "Payload": "{}" }
    }
  },
  {
    "EventType": "StepStarted",
    "SubType": "Step",
    "EventId": 2,
    "Id": "operation-id",
    "Name": "step-name",
    "EventTimestamp": "2025-12-03T22:58:35.096Z",
    "StepStartedDetails": {}
  }
]
```

### Current Rust SDK Event Format

The Rust SDK currently generates a simplified format:

```json
{
  "events": [
    { "event_type": "Execution" },
    { "event_type": "Step" }
  ]
}
```

### Key Differences

1. **Structure**: Node.js uses flat array `[]`, Rust uses object with `events` key
2. **Field naming**: Node.js uses PascalCase, Rust uses snake_case
3. **Event granularity**: Node.js generates Started/Succeeded/Failed events, Rust only captures operation type
4. **Event details**: Node.js includes rich event-specific details, Rust omits them
5. **Missing events**: Rust doesn't generate InvocationCompleted events

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     LocalDurableTestRunner                       │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┐    ┌─────────────────────────────────┐ │
│  │ TestExecutionOrch.  │───▶│    CheckpointWorkerManager      │ │
│  └─────────────────────┘    └─────────────────────────────────┘ │
│                                          │                       │
│                                          ▼                       │
│                              ┌─────────────────────────────────┐ │
│                              │      ExecutionManager           │ │
│                              └─────────────────────────────────┘ │
│                                          │                       │
│                                          ▼                       │
│                              ┌─────────────────────────────────┐ │
│                              │     CheckpointManager           │ │
│                              │  ┌───────────────────────────┐  │ │
│                              │  │   EventProcessor (NEW)    │  │ │
│                              │  │   - Node.js compatible    │  │ │
│                              │  │   - PascalCase fields     │  │ │
│                              │  │   - Full event details    │  │ │
│                              │  └───────────────────────────┘  │ │
│                              └─────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### New Types

#### NodeJsHistoryEvent

A new struct that matches the Node.js SDK's event format exactly:

```rust
/// A history event in Node.js SDK compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NodeJsHistoryEvent {
    /// The type of event (e.g., "ExecutionStarted", "StepSucceeded")
    pub event_type: NodeJsEventType,
    
    /// Sequential event ID starting from 1
    pub event_id: u64,
    
    /// The operation/execution ID (UUID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    
    /// ISO 8601 timestamp
    pub event_timestamp: String,
    
    /// Operation sub-type (e.g., "Step")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    
    /// Operation name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    
    /// Parent operation ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    
    /// Event-specific details (dynamically typed based on EventType)
    #[serde(flatten)]
    pub details: NodeJsEventDetails,
}
```

#### NodeJsEventType

An enum representing all supported event types:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeJsEventType {
    ExecutionStarted,
    ExecutionSucceeded,
    ExecutionFailed,
    StepStarted,
    StepSucceeded,
    StepFailed,
    WaitStarted,
    WaitSucceeded,
    WaitCancelled,
    CallbackStarted,
    CallbackSucceeded,
    CallbackFailed,
    CallbackTimedOut,
    ContextStarted,
    ContextSucceeded,
    ContextFailed,
    ChainedInvokeStarted,
    ChainedInvokeSucceeded,
    ChainedInvokeFailed,
    InvocationCompleted,
}
```

#### NodeJsEventDetails

An enum for event-specific details:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeJsEventDetails {
    ExecutionStarted(ExecutionStartedDetails),
    ExecutionSucceeded(ExecutionSucceededDetails),
    ExecutionFailed(ExecutionFailedDetails),
    StepStarted(StepStartedDetails),
    StepSucceeded(StepSucceededDetails),
    StepFailed(StepFailedDetails),
    WaitStarted(WaitStartedDetails),
    WaitSucceeded(WaitSucceededDetails),
    CallbackStarted(CallbackStartedDetails),
    ContextStarted(ContextStartedDetails),
    ContextSucceeded(ContextSucceededDetails),
    ContextFailed(ContextFailedDetails),
    InvocationCompleted(InvocationCompletedDetails),
}
```

### Detail Structures

Each event type has its own details structure:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionStartedDetails {
    pub input: PayloadWrapper,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_timeout: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ExecutionSucceededDetails {
    pub result: PayloadWrapper,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StepSucceededDetails {
    pub result: PayloadWrapper,
    pub retry_details: RetryDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct InvocationCompletedDetails {
    pub start_timestamp: String,
    pub end_timestamp: String,
    pub error: ErrorWrapper,
    pub request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PayloadWrapper {
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RetryDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_attempt_delay_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_attempt: Option<u32>,
}
```

### Modified EventProcessor

The `EventProcessor` will be enhanced to generate Node.js-compatible events:

```rust
impl EventProcessor {
    /// Create a Node.js-compatible history event.
    pub fn create_nodejs_event(
        &mut self,
        event_type: NodeJsEventType,
        operation: Option<&Operation>,
        details: NodeJsEventDetails,
    ) -> NodeJsHistoryEvent {
        self.event_counter += 1;
        
        NodeJsHistoryEvent {
            event_type,
            event_id: self.event_counter,
            id: operation.map(|op| op.operation_id.clone()),
            event_timestamp: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            sub_type: operation.and_then(|op| op.sub_type.clone()),
            name: operation.and_then(|op| op.name.clone()),
            parent_id: operation.and_then(|op| op.parent_id.clone()),
            details,
        }
    }
    
    /// Process an operation update and generate appropriate events.
    pub fn process_update(
        &mut self,
        update: &OperationUpdate,
        operation: &Operation,
    ) -> Vec<NodeJsHistoryEvent> {
        let mut events = Vec::new();
        
        match (update.action, update.operation_type) {
            (OperationAction::Start, OperationType::Execution) => {
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ExecutionStarted,
                    Some(operation),
                    NodeJsEventDetails::ExecutionStarted(ExecutionStartedDetails {
                        input: PayloadWrapper {
                            payload: update.result.clone().unwrap_or_default(),
                        },
                        execution_timeout: None,
                    }),
                ));
            }
            (OperationAction::Succeed, OperationType::Execution) => {
                events.push(self.create_nodejs_event(
                    NodeJsEventType::ExecutionSucceeded,
                    Some(operation),
                    NodeJsEventDetails::ExecutionSucceeded(ExecutionSucceededDetails {
                        result: PayloadWrapper {
                            payload: update.result.clone().unwrap_or_default(),
                        },
                    }),
                ));
            }
            // ... similar for other action/type combinations
            _ => {}
        }
        
        events
    }
}
```

### History Event Detail Mapping

The mapping from (OperationAction, OperationType) to EventType follows the Node.js SDK:

| Action | Type | EventType | Details Field |
|--------|------|-----------|---------------|
| Start | Execution | ExecutionStarted | ExecutionStartedDetails |
| Succeed | Execution | ExecutionSucceeded | ExecutionSucceededDetails |
| Fail | Execution | ExecutionFailed | ExecutionFailedDetails |
| Start | Step | StepStarted | StepStartedDetails |
| Succeed | Step | StepSucceeded | StepSucceededDetails |
| Fail | Step | StepFailed | StepFailedDetails |
| Retry | Step | StepFailed/StepSucceeded | StepFailedDetails/StepSucceededDetails |
| Start | Wait | WaitStarted | WaitStartedDetails |
| Succeed | Wait | WaitSucceeded | WaitSucceededDetails |
| Start | Callback | CallbackStarted | CallbackStartedDetails |
| Start | Context | ContextStarted | ContextStartedDetails |
| Succeed | Context | ContextSucceeded | ContextSucceededDetails |
| Fail | Context | ContextFailed | ContextFailedDetails |

### Updated Test Helper

The `test_helper.rs` will be updated to:

1. Generate history files in Node.js-compatible format
2. Compare events by EventType, SubType, and Name (ignoring volatile fields)

```rust
/// A Node.js-compatible history file (flat array).
pub type NodeJsHistoryFile = Vec<NodeJsHistoryEvent>;

/// Extract Node.js-compatible events from operations and history.
pub fn extract_nodejs_events(
    operations: &[Operation],
    history_events: &[NodeJsHistoryEvent],
) -> NodeJsHistoryFile {
    history_events.to_vec()
}

/// Assert that event history matches expected (ignoring volatile fields).
pub fn assert_nodejs_event_signatures(
    actual: &[NodeJsHistoryEvent],
    expected_path: &str,
) {
    // Compare by EventType, SubType, Name only
    // Ignore EventId, Id, EventTimestamp
}
```

## File Changes

### New Files

1. `aws-durable-execution-sdk/testing/src/checkpoint_server/nodejs_event_types.rs`
   - Contains all Node.js-compatible event types and detail structures

### Modified Files

1. `aws-durable-execution-sdk/testing/src/checkpoint_server/event_processor.rs`
   - Add methods to generate Node.js-compatible events
   - Add mapping from (Action, Type) to EventType

2. `aws-durable-execution-sdk/testing/src/checkpoint_server/checkpoint_manager.rs`
   - Update to generate Node.js-compatible events during operation processing
   - Add InvocationCompleted event generation

3. `aws-durable-execution-sdk/testing/src/checkpoint_server/mod.rs`
   - Export new types

4. `aws-durable-execution-sdk/testing/src/test_result.rs`
   - Add method to get Node.js-compatible history events

5. `aws-durable-execution-sdk/examples/src/test_helper.rs`
   - Update to generate/compare Node.js-compatible history files

6. `aws-durable-execution-sdk/examples/tests/history/*.history.json`
   - Update all history files to Node.js-compatible format

## Correctness Properties

### Property 1: Event ID Monotonicity
**Validates: Requirements 1.2**

For any sequence of events E₁, E₂, ..., Eₙ generated by the EventProcessor:
- E₁.EventId = 1
- ∀i ∈ [2, n]: Eᵢ.EventId = Eᵢ₋₁.EventId + 1

**Test Strategy**: Property-based test generating random sequences of operations and verifying event IDs are strictly increasing starting from 1.

### Property 2: Event Type Mapping Consistency
**Validates: Requirements 2.1-2.12**

For any operation update with action A and type T, the generated event type must match:

| Action | Type | Expected EventType |
|--------|------|-------------------|
| Start | Execution | ExecutionStarted |
| Succeed | Execution | ExecutionSucceeded |
| Fail | Execution | ExecutionFailed |
| Start | Step | StepStarted |
| Succeed | Step | StepSucceeded |
| Fail | Step | StepFailed |
| Start | Wait | WaitStarted |
| Succeed | Wait | WaitSucceeded |
| Start | Callback | CallbackStarted |
| Start | Context | ContextStarted |
| Succeed | Context | ContextSucceeded |
| Fail | Context | ContextFailed |

**Test Strategy**: Property-based test generating all valid (Action, Type) combinations and verifying the correct EventType is produced.

### Property 3: Timestamp Format Validity
**Validates: Requirements 1.4, 4.3, 4.4, 5.5**

For any event E:
- E.EventTimestamp matches ISO 8601 format: `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$`

For InvocationCompletedDetails:
- StartTimestamp matches ISO 8601 format
- EndTimestamp matches ISO 8601 format
- StartTimestamp ≤ EndTimestamp (chronologically)

**Test Strategy**: Property-based test generating events at various times and verifying all timestamps match the ISO 8601 pattern.

### Property 4: PascalCase Field Names
**Validates: Requirements 1.1, 5.4**

For any serialized event JSON:
- All top-level field names match PascalCase pattern: `^[A-Z][a-zA-Z0-9]*$`
- All nested field names match PascalCase pattern

**Test Strategy**: Property-based test serializing various events to JSON and verifying all keys match PascalCase pattern.

### Property 5: JSON Array Output Format
**Validates: Requirements 5.1, 5.2, 5.3**

For any serialized history H:
- H is a valid JSON array at root level (starts with `[`, ends with `]`)
- H does not contain an "events" wrapper key
- Events are ordered by EventId (strictly increasing)

**Test Strategy**: Property-based test generating histories and verifying the output format.

### Property 6: Details Field Correctness
**Validates: Requirements 3.1-3.11**

For any event E with EventType T:
- E contains exactly one details field named `{T}Details`
- The details field is present (may be empty object for some types)

**Test Strategy**: Example-based tests for each event type verifying the correct details field is present with required structure.

## Testing Strategy

### Unit Tests

1. **EventProcessor tests**
   - Test each (Action, Type) → EventType mapping
   - Test event ID incrementing
   - Test timestamp format

2. **Detail structure tests**
   - Test serialization of each detail type
   - Test PascalCase field naming

3. **History file tests**
   - Test JSON array format (no wrapper)
   - Test event ordering

### Integration Tests

1. **Cross-SDK comparison tests**
   - Run same workflow in both SDKs
   - Compare history output (ignoring volatile fields)

2. **Existing example tests**
   - Update all existing history files
   - Verify tests pass with new format

### Property-Based Tests

1. **Event ID monotonicity**
   - Generate random sequences of operations
   - Verify event IDs are strictly increasing

2. **Timestamp validity**
   - Generate events at various times
   - Verify all timestamps match ISO 8601 format

## Migration Plan

1. **Phase 1: Add new types** (non-breaking)
   - Add NodeJsHistoryEvent and related types
   - Add new methods to EventProcessor

2. **Phase 2: Update CheckpointManager**
   - Generate Node.js-compatible events alongside existing events
   - Add InvocationCompleted event generation

3. **Phase 3: Update test_helper**
   - Add Node.js-compatible history file generation
   - Add comparison utilities

4. **Phase 4: Update history files**
   - Regenerate all history files in new format
   - Update tests to use new format

5. **Phase 5: Deprecate old format** 
   - Remove all old event types 
