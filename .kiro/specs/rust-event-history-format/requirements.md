# Requirements Document

## Introduction

This document specifies the requirements for fixing the event history output format in the Rust durable execution SDK to match the Node.js SDK format. The current Rust SDK generates a simplified event history format that is incompatible with the Node.js SDK, making cross-SDK history comparison impossible. This feature will align the Rust SDK's event history output with the Node.js SDK's format for consistent behavior across SDKs.

## Glossary

- **Event_History**: A chronological list of events that occurred during a durable execution, used for debugging, testing, and cross-SDK comparison
- **History_Event**: A single event in the event history, containing event type, ID, timestamp, and event-specific details
- **Event_Processor**: The component responsible for generating history events from operation updates
- **Checkpoint_Manager**: The component that manages execution state and coordinates event generation
- **Event_Type**: The classification of an event (e.g., ExecutionStarted, StepSucceeded, InvocationCompleted)
- **Event_Details**: Event-specific data stored in a dedicated details field (e.g., ExecutionStartedDetails, StepSucceededDetails)
- **Operation_Action**: The action being performed on an operation (Start, Succeed, Fail, Retry, Cancel)
- **Operation_Type**: The type of durable operation (Execution, Step, Wait, Callback, Invoke, Context)

## Requirements

### Requirement 1: Event Structure Format

**User Story:** As a developer, I want the Rust SDK to generate history events with the same structure as the Node.js SDK, so that I can compare execution histories across SDKs.

#### Acceptance Criteria

1. THE History_Event SHALL use PascalCase field names (EventType, EventId, EventTimestamp, Id) instead of snake_case
2. THE History_Event SHALL include an EventId field containing a sequential integer starting from 1
3. THE History_Event SHALL include an Id field containing the operation's unique identifier (UUID)
4. THE History_Event SHALL include an EventTimestamp field containing an ISO 8601 formatted timestamp
5. WHEN an operation has a SubType, THE History_Event SHALL include a SubType field with the operation's sub-type value
6. WHEN an operation has a Name, THE History_Event SHALL include a Name field with the operation's name
7. WHEN an operation has a ParentId, THE History_Event SHALL include a ParentId field with the parent operation's ID

### Requirement 2: Event Type Granularity

**User Story:** As a developer, I want the Rust SDK to generate separate Started/Succeeded/Failed events for each operation, so that I can track the full lifecycle of operations.

#### Acceptance Criteria

1. WHEN an Execution operation starts, THE Event_Processor SHALL generate an ExecutionStarted event
2. WHEN an Execution operation succeeds, THE Event_Processor SHALL generate an ExecutionSucceeded event
3. WHEN an Execution operation fails, THE Event_Processor SHALL generate an ExecutionFailed event
4. WHEN a Step operation starts, THE Event_Processor SHALL generate a StepStarted event
5. WHEN a Step operation succeeds, THE Event_Processor SHALL generate a StepSucceeded event
6. WHEN a Step operation fails, THE Event_Processor SHALL generate a StepFailed event
7. WHEN a Wait operation starts, THE Event_Processor SHALL generate a WaitStarted event
8. WHEN a Wait operation succeeds, THE Event_Processor SHALL generate a WaitSucceeded event
9. WHEN a Callback operation starts, THE Event_Processor SHALL generate a CallbackStarted event
10. WHEN a Context operation starts, THE Event_Processor SHALL generate a ContextStarted event
11. WHEN a Context operation succeeds, THE Event_Processor SHALL generate a ContextSucceeded event
12. WHEN a Context operation fails, THE Event_Processor SHALL generate a ContextFailed event

### Requirement 3: Event Details Structure

**User Story:** As a developer, I want each event type to include its specific details in a dedicated field, so that I can access event-specific information consistently.

#### Acceptance Criteria

1. WHEN an ExecutionStarted event is generated, THE History_Event SHALL include an ExecutionStartedDetails field containing Input.Payload and optional ExecutionTimeout
2. WHEN an ExecutionSucceeded event is generated, THE History_Event SHALL include an ExecutionSucceededDetails field containing Result.Payload
3. WHEN an ExecutionFailed event is generated, THE History_Event SHALL include an ExecutionFailedDetails field containing Error.Payload
4. WHEN a StepStarted event is generated, THE History_Event SHALL include a StepStartedDetails field (may be empty object)
5. WHEN a StepSucceeded event is generated, THE History_Event SHALL include a StepSucceededDetails field containing Result.Payload and RetryDetails
6. WHEN a StepFailed event is generated, THE History_Event SHALL include a StepFailedDetails field containing Error.Payload and RetryDetails
7. WHEN a WaitStarted event is generated, THE History_Event SHALL include a WaitStartedDetails field containing Duration and ScheduledEndTimestamp
8. WHEN a CallbackStarted event is generated, THE History_Event SHALL include a CallbackStartedDetails field containing CallbackId, Timeout, HeartbeatTimeout, and Input.Payload
9. WHEN a ContextStarted event is generated, THE History_Event SHALL include a ContextStartedDetails field (may be empty object)
10. WHEN a ContextSucceeded event is generated, THE History_Event SHALL include a ContextSucceededDetails field containing Result.Payload
11. WHEN a ContextFailed event is generated, THE History_Event SHALL include a ContextFailedDetails field containing Error.Payload

### Requirement 4: Invocation Events

**User Story:** As a developer, I want the Rust SDK to generate InvocationCompleted events, so that I can track handler invocation lifecycle.

#### Acceptance Criteria

1. WHEN a handler invocation completes, THE Event_Processor SHALL generate an InvocationCompleted event
2. THE InvocationCompleted event SHALL include an InvocationCompletedDetails field
3. THE InvocationCompletedDetails SHALL contain StartTimestamp with the invocation start time in ISO 8601 format
4. THE InvocationCompletedDetails SHALL contain EndTimestamp with the invocation end time in ISO 8601 format
5. THE InvocationCompletedDetails SHALL contain RequestId with the invocation identifier
6. THE InvocationCompletedDetails SHALL contain an Error object (empty if no error occurred)

### Requirement 5: JSON Output Format

**User Story:** As a developer, I want the history file to be a flat JSON array, so that it matches the Node.js SDK format exactly.

#### Acceptance Criteria

1. THE Event_History output SHALL be a JSON array at the root level (not wrapped in an object)
2. THE Event_History output SHALL NOT use an "events" wrapper key
3. THE Event_History output SHALL contain History_Event objects in chronological order by EventId
4. WHEN serializing to JSON, THE Event_Processor SHALL use PascalCase for all field names
5. WHEN serializing to JSON, THE Event_Processor SHALL format timestamps as ISO 8601 strings (e.g., "2025-12-03T22:58:35.094Z")

### Requirement 6: History File Generation

**User Story:** As a developer, I want to generate history files that can be compared with Node.js SDK history files, so that I can verify cross-SDK compatibility.

#### Acceptance Criteria

1. WHEN GENERATE_HISTORY environment variable is set to "true", THE test_helper SHALL generate a history file in the Node.js-compatible format
2. THE generated history file SHALL contain all events from the execution in chronological order
3. THE generated history file SHALL be parseable by the Node.js SDK's history comparison utilities
4. WHEN comparing history files, THE test_helper SHALL compare events by EventType, SubType, and Name (ignoring volatile fields like EventId, Id, and EventTimestamp)

### Requirement 7: Retry Event Handling

**User Story:** As a developer, I want retry operations to generate appropriate events, so that I can track retry behavior in the history.

#### Acceptance Criteria

1. WHEN a Step operation is retried with success, THE Event_Processor SHALL generate a StepSucceeded event
2. WHEN a Step operation is retried with failure, THE Event_Processor SHALL generate a StepFailed event
3. THE RetryDetails in StepSucceeded/StepFailed events SHALL contain CurrentAttempt with the attempt number
4. THE RetryDetails in StepSucceeded/StepFailed events SHALL contain NextAttemptDelaySeconds when applicable
