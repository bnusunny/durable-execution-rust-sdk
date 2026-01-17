//! Test helper utilities for example workflow tests.
//!
//! This module provides utilities for testing example workflows using the
//! `LocalDurableTestRunner`. It includes functionality for comparing execution
//! history against saved JSON files.
//!
//! # Features
//!
//! - `EventSignature`: A simplified representation of history events for comparison
//! - `assert_event_signatures`: Compare execution history against saved JSON files
//! - History file generation with `GENERATE_HISTORY=true` environment variable
//!
//! # Example
//!
//! ```ignore
//! use aws_durable_execution_sdk_examples::test_helper::{assert_event_signatures, EventSignature};
//!
//! #[tokio::test]
//! async fn test_my_workflow() {
//!     let mut runner = LocalDurableTestRunner::new(my_handler);
//!     let result = runner.run(input).await.unwrap();
//!     
//!     assert_event_signatures(
//!         &result,
//!         "src/bin/my_workflow/my_workflow.history.json",
//!     );
//! }
//! ```

use aws_durable_execution_sdk::{Operation, OperationType};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;

/// A simplified representation of an operation for comparison purposes.
///
/// This struct captures the essential identifying characteristics of an operation
/// without including volatile data like timestamps or unique IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventSignature {
    /// The type of operation (Step, Wait, Callback, etc.)
    pub event_type: String,
    /// Optional subtype for more specific categorization
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<String>,
    /// The operation name if provided
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EventSignature {
    /// Creates a new EventSignature from an operation type and optional name.
    pub fn new(event_type: impl Into<String>, name: Option<String>) -> Self {
        Self {
            event_type: event_type.into(),
            sub_type: None,
            name,
        }
    }

    /// Creates a new EventSignature with a subtype.
    pub fn with_sub_type(
        event_type: impl Into<String>,
        sub_type: impl Into<String>,
        name: Option<String>,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            sub_type: Some(sub_type.into()),
            name,
        }
    }

    /// Creates an EventSignature from an Operation.
    pub fn from_operation(op: &Operation) -> Self {
        let event_type = match op.operation_type {
            OperationType::Step => "Step",
            OperationType::Wait => "Wait",
            OperationType::Callback => "Callback",
            OperationType::Invoke => "Invoke",
            OperationType::Context => "Context",
            OperationType::Execution => "Execution",
        };

        Self {
            event_type: event_type.to_string(),
            sub_type: None,
            name: op.name.clone(),
        }
    }
}

/// A collection of event signatures representing the expected execution history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryFile {
    /// Description of the test case
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The expected event signatures in order
    pub events: Vec<EventSignature>,
}

impl HistoryFile {
    /// Creates a new HistoryFile with the given events.
    pub fn new(events: Vec<EventSignature>) -> Self {
        Self {
            description: None,
            events,
        }
    }

    /// Creates a new HistoryFile with a description.
    pub fn with_description(description: impl Into<String>, events: Vec<EventSignature>) -> Self {
        Self {
            description: Some(description.into()),
            events,
        }
    }
}

/// Extracts event signatures from a list of operations.
///
/// This function converts operations to their simplified signature form,
/// filtering out duplicate entries (e.g., Start and Succeed for the same operation).
pub fn extract_event_signatures(operations: &[Operation]) -> Vec<EventSignature> {
    let mut signatures = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for op in operations {
        // Only include each operation once (by ID)
        if seen_ids.insert(op.operation_id.clone()) {
            signatures.push(EventSignature::from_operation(op));
        }
    }

    signatures
}

/// Asserts that the execution history matches the expected signatures from a JSON file.
///
/// If the `GENERATE_HISTORY` environment variable is set to "true", this function
/// will generate/update the history file instead of comparing against it.
///
/// # Arguments
///
/// * `operations` - The operations from the test execution
/// * `history_file_path` - Path to the history JSON file (relative to workspace root)
///
/// # Panics
///
/// Panics if the history doesn't match and `GENERATE_HISTORY` is not set.
///
/// # Example
///
/// ```ignore
/// let result = runner.run(input).await.unwrap();
/// assert_event_signatures(
///     result.get_operations(),
///     "examples/src/bin/step/basic/step_basic.history.json",
/// );
/// ```
pub fn assert_event_signatures(operations: &[Operation], history_file_path: &str) {
    let actual_signatures = extract_event_signatures(operations);

    // Check if we should generate the history file
    if env::var("GENERATE_HISTORY").map(|v| v == "true").unwrap_or(false) {
        generate_history_file(&actual_signatures, history_file_path);
        return;
    }

    // Load and compare against expected history
    let expected = load_history_file(history_file_path);

    assert_eq!(
        actual_signatures.len(),
        expected.events.len(),
        "Number of events doesn't match.\nExpected {} events, got {}.\nActual events: {:#?}",
        expected.events.len(),
        actual_signatures.len(),
        actual_signatures
    );

    for (i, (actual, expected)) in actual_signatures.iter().zip(expected.events.iter()).enumerate()
    {
        assert_eq!(
            actual, expected,
            "Event {} doesn't match.\nExpected: {:#?}\nActual: {:#?}",
            i, expected, actual
        );
    }
}

/// Asserts that the execution history contains the same events as expected, regardless of order.
///
/// This is useful for testing map operations with concurrency where the order of
/// operations can vary between runs.
///
/// If the `GENERATE_HISTORY` environment variable is set to "true", this function
/// will generate/update the history file instead of comparing against it.
///
/// # Arguments
///
/// * `operations` - The operations from the test execution
/// * `history_file_path` - Path to the history JSON file (relative to workspace root)
///
/// # Panics
///
/// Panics if the history doesn't match and `GENERATE_HISTORY` is not set.
pub fn assert_event_signatures_unordered(operations: &[Operation], history_file_path: &str) {
    let actual_signatures = extract_event_signatures(operations);

    // Check if we should generate the history file
    if env::var("GENERATE_HISTORY").map(|v| v == "true").unwrap_or(false) {
        generate_history_file(&actual_signatures, history_file_path);
        return;
    }

    // Load expected history
    let expected = load_history_file(history_file_path);

    assert_eq!(
        actual_signatures.len(),
        expected.events.len(),
        "Number of events doesn't match.\nExpected {} events, got {}.\nActual events: {:#?}",
        expected.events.len(),
        actual_signatures.len(),
        actual_signatures
    );

    // Count occurrences of each event signature
    let mut expected_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut actual_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for sig in &expected.events {
        let key = format!("{:?}", sig);
        *expected_counts.entry(key).or_insert(0) += 1;
    }

    for sig in &actual_signatures {
        let key = format!("{:?}", sig);
        *actual_counts.entry(key).or_insert(0) += 1;
    }

    // Compare counts
    for (key, expected_count) in &expected_counts {
        let actual_count = actual_counts.get(key).unwrap_or(&0);
        assert_eq!(
            actual_count, expected_count,
            "Event count mismatch for {}.\nExpected {} occurrences, got {}.\nExpected events: {:#?}\nActual events: {:#?}",
            key, expected_count, actual_count, expected.events, actual_signatures
        );
    }

    // Check for unexpected events
    for (key, actual_count) in &actual_counts {
        if !expected_counts.contains_key(key) {
            panic!(
                "Unexpected event found: {} (count: {}).\nExpected events: {:#?}\nActual events: {:#?}",
                key, actual_count, expected.events, actual_signatures
            );
        }
    }
}

/// Loads a history file from the given path.
///
/// # Panics
///
/// Panics if the file doesn't exist or can't be parsed.
pub fn load_history_file(path: &str) -> HistoryFile {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read history file '{}': {}", path, e));

    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse history file '{}': {}", path, e))
}

/// Generates a history file at the given path.
fn generate_history_file(signatures: &[EventSignature], path: &str) {
    let history = HistoryFile::new(signatures.to_vec());
    let content = serde_json::to_string_pretty(&history)
        .expect("Failed to serialize history file");

    // Ensure parent directory exists
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directory");
    }

    fs::write(path, content).unwrap_or_else(|e| panic!("Failed to write history file '{}': {}", path, e));

    println!("Generated history file: {}", path);
}

/// Helper macro for creating event signatures in tests.
///
/// # Example
///
/// ```ignore
/// let expected = vec![
///     event_sig!("Step"),
///     event_sig!("Step", "process_data"),
///     event_sig!("Wait", "delay"),
/// ];
/// ```
#[macro_export]
macro_rules! event_sig {
    ($event_type:expr) => {
        $crate::test_helper::EventSignature::new($event_type, None)
    };
    ($event_type:expr, $name:expr) => {
        $crate::test_helper::EventSignature::new($event_type, Some($name.to_string()))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::{Operation, OperationType};

    fn create_test_operation(name: Option<&str>, op_type: OperationType) -> Operation {
        let mut op = Operation::new(
            format!("op-{}", uuid::Uuid::new_v4()),
            op_type,
        );
        op.name = name.map(|s| s.to_string());
        op
    }

    #[test]
    fn test_event_signature_from_operation() {
        let op = create_test_operation(Some("test_step"), OperationType::Step);
        let sig = EventSignature::from_operation(&op);

        assert_eq!(sig.event_type, "Step");
        assert_eq!(sig.name, Some("test_step".to_string()));
        assert_eq!(sig.sub_type, None);
    }

    #[test]
    fn test_extract_event_signatures() {
        let operations = vec![
            create_test_operation(Some("step1"), OperationType::Step),
            create_test_operation(Some("wait1"), OperationType::Wait),
            create_test_operation(None, OperationType::Step),
        ];

        let signatures = extract_event_signatures(&operations);

        assert_eq!(signatures.len(), 3);
        assert_eq!(signatures[0].event_type, "Step");
        assert_eq!(signatures[0].name, Some("step1".to_string()));
        assert_eq!(signatures[1].event_type, "Wait");
        assert_eq!(signatures[1].name, Some("wait1".to_string()));
        assert_eq!(signatures[2].event_type, "Step");
        assert_eq!(signatures[2].name, None);
    }

    #[test]
    fn test_history_file_serialization() {
        let history = HistoryFile::with_description(
            "Test workflow",
            vec![
                EventSignature::new("Step", Some("step1".to_string())),
                EventSignature::new("Wait", None),
            ],
        );

        let json = serde_json::to_string_pretty(&history).unwrap();
        let parsed: HistoryFile = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.description, Some("Test workflow".to_string()));
        assert_eq!(parsed.events.len(), 2);
    }

    #[test]
    fn test_event_sig_macro() {
        let sig1 = event_sig!("Step");
        assert_eq!(sig1.event_type, "Step");
        assert_eq!(sig1.name, None);

        let sig2 = event_sig!("Wait", "delay");
        assert_eq!(sig2.event_type, "Wait");
        assert_eq!(sig2.name, Some("delay".to_string()));
    }
}
