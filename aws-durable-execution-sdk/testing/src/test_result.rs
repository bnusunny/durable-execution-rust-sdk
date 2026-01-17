//! Test result types for durable execution testing.
//!
//! This module provides the `TestResult` struct which contains execution results
//! and provides inspection methods for verifying workflow behavior.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::TestError;
use crate::types::{ExecutionStatus, Invocation, TestResultError};
use aws_durable_execution_sdk::{Operation, OperationStatus};

/// Result of a durable execution test.
///
/// Contains the execution outcome, captured operations, and provides methods
/// for inspecting and verifying workflow behavior.
///
/// # Type Parameters
///
/// * `T` - The type of the successful result value
///
/// # Examples
///
/// ```ignore
/// use aws_durable_execution_sdk_testing::{TestResult, ExecutionStatus};
///
/// // After running a test
/// let result: TestResult<String> = runner.run("input").await?;
///
/// // Check status
/// assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
///
/// // Get the result value
/// let value = result.get_result()?;
/// assert_eq!(value, "expected output");
///
/// // Inspect operations
/// let ops = result.get_operations();
/// assert_eq!(ops.len(), 3);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult<T> {
    /// The execution status
    status: ExecutionStatus,
    /// The result value if execution succeeded
    result: Option<T>,
    /// Error information if execution failed
    error: Option<TestResultError>,
    /// All operations captured during execution
    operations: Vec<Operation>,
    /// Handler invocation details
    invocations: Vec<Invocation>,
    /// History events from the execution
    history_events: Vec<HistoryEvent>,
}

/// A history event from the execution.
///
/// Represents significant events that occurred during the durable execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    /// Event type identifier
    pub event_type: String,
    /// Timestamp of the event (milliseconds since epoch)
    pub timestamp: Option<i64>,
    /// Associated operation ID if applicable
    pub operation_id: Option<String>,
    /// Additional event data
    pub data: Option<String>,
}

impl HistoryEvent {
    /// Creates a new history event.
    pub fn new(event_type: impl Into<String>) -> Self {
        Self {
            event_type: event_type.into(),
            timestamp: None,
            operation_id: None,
            data: None,
        }
    }

    /// Sets the timestamp.
    pub fn with_timestamp(mut self, timestamp: i64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the operation ID.
    pub fn with_operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }

    /// Sets the event data.
    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }
}

impl<T> TestResult<T> {
    /// Creates a new successful TestResult.
    pub fn success(result: T, operations: Vec<Operation>) -> Self {
        Self {
            status: ExecutionStatus::Succeeded,
            result: Some(result),
            error: None,
            operations,
            invocations: Vec::new(),
            history_events: Vec::new(),
        }
    }

    /// Creates a new failed TestResult.
    pub fn failure(error: TestResultError, operations: Vec<Operation>) -> Self {
        Self {
            status: ExecutionStatus::Failed,
            result: None,
            error: Some(error),
            operations,
            invocations: Vec::new(),
            history_events: Vec::new(),
        }
    }

    /// Creates a new TestResult with a specific status.
    pub fn with_status(status: ExecutionStatus, operations: Vec<Operation>) -> Self {
        Self {
            status,
            result: None,
            error: None,
            operations,
            invocations: Vec::new(),
            history_events: Vec::new(),
        }
    }

    /// Sets the result value.
    pub fn set_result(&mut self, result: T) {
        self.result = Some(result);
    }

    /// Sets the error.
    pub fn set_error(&mut self, error: TestResultError) {
        self.error = Some(error);
    }

    /// Sets the invocations.
    pub fn set_invocations(&mut self, invocations: Vec<Invocation>) {
        self.invocations = invocations;
    }

    /// Adds an invocation.
    pub fn add_invocation(&mut self, invocation: Invocation) {
        self.invocations.push(invocation);
    }

    /// Sets the history events.
    pub fn set_history_events(&mut self, events: Vec<HistoryEvent>) {
        self.history_events = events;
    }

    /// Adds a history event.
    pub fn add_history_event(&mut self, event: HistoryEvent) {
        self.history_events.push(event);
    }

    /// Gets the execution status.
    ///
    /// # Returns
    ///
    /// The current execution status (Succeeded, Failed, Running, etc.)
    ///
    /// # Requirements
    ///
    /// - 3.1: WHEN a developer calls get_status() on Test_Result, THE Test_Result SHALL return the execution status
    pub fn get_status(&self) -> ExecutionStatus {
        self.status
    }

    /// Gets the result value if execution succeeded.
    ///
    /// # Returns
    ///
    /// - `Ok(&T)` - Reference to the result value if execution succeeded
    /// - `Err(TestError)` - Error if execution failed or result is not available
    ///
    /// # Requirements
    ///
    /// - 3.2: WHEN a developer calls get_result() on a successful execution, THE Test_Result SHALL return the deserialized result value
    /// - 3.3: WHEN a developer calls get_result() on a failed execution, THE Test_Result SHALL return an error
    pub fn get_result(&self) -> Result<&T, TestError> {
        match self.status {
            ExecutionStatus::Succeeded => self.result.as_ref().ok_or_else(|| {
                TestError::result_not_available("Execution succeeded but result is not set")
            }),
            ExecutionStatus::Failed => Err(TestError::result_not_available(
                "Cannot get result from failed execution",
            )),
            ExecutionStatus::Running => Err(TestError::result_not_available(
                "Execution is still running",
            )),
            ExecutionStatus::Cancelled => {
                Err(TestError::result_not_available("Execution was cancelled"))
            }
            ExecutionStatus::TimedOut => {
                Err(TestError::result_not_available("Execution timed out"))
            }
        }
    }

    /// Gets the error if execution failed.
    ///
    /// # Returns
    ///
    /// - `Ok(&TestResultError)` - Reference to the error if execution failed
    /// - `Err(&str)` - Error message if execution succeeded or error is not available
    ///
    /// # Requirements
    ///
    /// - 3.4: WHEN a developer calls get_error() on a failed execution, THE Test_Result SHALL return the error details
    pub fn get_error(&self) -> Result<&TestResultError, &str> {
        match self.status {
            ExecutionStatus::Failed | ExecutionStatus::Cancelled | ExecutionStatus::TimedOut => {
                self.error
                    .as_ref()
                    .ok_or("Execution failed but error details are not available")
            }
            ExecutionStatus::Succeeded => Err("Cannot get error from successful execution"),
            ExecutionStatus::Running => Err("Execution is still running"),
        }
    }

    /// Gets all operations from the execution.
    ///
    /// # Returns
    ///
    /// A slice of all operations captured during execution, in execution order.
    ///
    /// # Requirements
    ///
    /// - 3.5: WHEN a developer calls get_operations() on Test_Result, THE Test_Result SHALL return all operations from the execution
    pub fn get_operations(&self) -> &[Operation] {
        &self.operations
    }

    /// Gets operations filtered by status.
    ///
    /// # Arguments
    ///
    /// * `status` - The operation status to filter by
    ///
    /// # Returns
    ///
    /// A vector of references to operations matching the given status.
    ///
    /// # Requirements
    ///
    /// - 3.6: WHEN a developer calls get_operations() with a status filter, THE Test_Result SHALL return only operations matching that status
    pub fn get_operations_by_status(&self, status: OperationStatus) -> Vec<&Operation> {
        self.operations
            .iter()
            .filter(|op| op.status == status)
            .collect()
    }

    /// Gets handler invocation details.
    ///
    /// # Returns
    ///
    /// A slice of invocation details for each time the handler was invoked.
    ///
    /// # Requirements
    ///
    /// - 3.7: WHEN a developer calls get_invocations() on Test_Result, THE Test_Result SHALL return details about each handler invocation
    pub fn get_invocations(&self) -> &[Invocation] {
        &self.invocations
    }

    /// Gets history events from the execution.
    ///
    /// # Returns
    ///
    /// A slice of history events that occurred during execution.
    pub fn get_history_events(&self) -> &[HistoryEvent] {
        &self.history_events
    }

    /// Checks if the execution succeeded.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Checks if the execution failed.
    pub fn is_failure(&self) -> bool {
        self.status.is_failure()
    }

    /// Checks if the execution is still running.
    pub fn is_running(&self) -> bool {
        matches!(self.status, ExecutionStatus::Running)
    }

    /// Gets the number of operations.
    pub fn operation_count(&self) -> usize {
        self.operations.len()
    }

    /// Gets the number of invocations.
    pub fn invocation_count(&self) -> usize {
        self.invocations.len()
    }
}

/// Configuration for print output.
///
/// Controls which columns are displayed when printing the operations table.
#[derive(Debug, Clone)]
pub struct PrintConfig {
    /// Show the operation ID column
    pub show_id: bool,
    /// Show the parent ID column
    pub show_parent_id: bool,
    /// Show the operation name column
    pub show_name: bool,
    /// Show the operation type column
    pub show_type: bool,
    /// Show the operation status column
    pub show_status: bool,
    /// Show the start timestamp column
    pub show_start_time: bool,
    /// Show the end timestamp column
    pub show_end_time: bool,
    /// Show the duration column
    pub show_duration: bool,
    /// Show the result/error column
    pub show_result: bool,
}

impl Default for PrintConfig {
    fn default() -> Self {
        Self {
            show_id: false,
            show_parent_id: false,
            show_name: true,
            show_type: true,
            show_status: true,
            show_start_time: true,
            show_end_time: true,
            show_duration: true,
            show_result: false,
        }
    }
}

impl PrintConfig {
    /// Creates a new PrintConfig with all columns enabled.
    pub fn all() -> Self {
        Self {
            show_id: true,
            show_parent_id: true,
            show_name: true,
            show_type: true,
            show_status: true,
            show_start_time: true,
            show_end_time: true,
            show_duration: true,
            show_result: true,
        }
    }

    /// Creates a minimal PrintConfig with only essential columns.
    pub fn minimal() -> Self {
        Self {
            show_id: false,
            show_parent_id: false,
            show_name: true,
            show_type: true,
            show_status: true,
            show_start_time: false,
            show_end_time: false,
            show_duration: false,
            show_result: false,
        }
    }

    /// Builder method to set show_id.
    pub fn with_id(mut self, show: bool) -> Self {
        self.show_id = show;
        self
    }

    /// Builder method to set show_parent_id.
    pub fn with_parent_id(mut self, show: bool) -> Self {
        self.show_parent_id = show;
        self
    }

    /// Builder method to set show_name.
    pub fn with_name(mut self, show: bool) -> Self {
        self.show_name = show;
        self
    }

    /// Builder method to set show_type.
    pub fn with_type(mut self, show: bool) -> Self {
        self.show_type = show;
        self
    }

    /// Builder method to set show_status.
    pub fn with_status(mut self, show: bool) -> Self {
        self.show_status = show;
        self
    }

    /// Builder method to set show_start_time.
    pub fn with_start_time(mut self, show: bool) -> Self {
        self.show_start_time = show;
        self
    }

    /// Builder method to set show_end_time.
    pub fn with_end_time(mut self, show: bool) -> Self {
        self.show_end_time = show;
        self
    }

    /// Builder method to set show_duration.
    pub fn with_duration(mut self, show: bool) -> Self {
        self.show_duration = show;
        self
    }

    /// Builder method to set show_result.
    pub fn with_result(mut self, show: bool) -> Self {
        self.show_result = show;
        self
    }
}

impl<T> TestResult<T> {
    /// Prints a formatted table of all operations to stdout.
    ///
    /// Uses default column configuration showing name, type, status, and timing information.
    ///
    /// # Requirements
    ///
    /// - 11.1: WHEN a developer calls print() on Test_Result, THE Test_Result SHALL print a formatted table of all operations to stdout
    /// - 11.3: THE printed table SHALL include operation name, type, status, and timing information by default
    pub fn print(&self) {
        self.print_with_config(PrintConfig::default());
    }

    /// Prints a formatted table of operations with custom column configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration specifying which columns to include
    ///
    /// # Requirements
    ///
    /// - 11.2: WHEN a developer calls print() with column configuration, THE Test_Result SHALL include only the specified columns
    pub fn print_with_config(&self, config: PrintConfig) {
        // Build header row
        let mut headers: Vec<&str> = Vec::new();
        if config.show_id {
            headers.push("ID");
        }
        if config.show_parent_id {
            headers.push("Parent ID");
        }
        if config.show_name {
            headers.push("Name");
        }
        if config.show_type {
            headers.push("Type");
        }
        if config.show_status {
            headers.push("Status");
        }
        if config.show_start_time {
            headers.push("Start Time");
        }
        if config.show_end_time {
            headers.push("End Time");
        }
        if config.show_duration {
            headers.push("Duration");
        }
        if config.show_result {
            headers.push("Result/Error");
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

        // Build rows and update widths
        let rows: Vec<Vec<String>> = self
            .operations
            .iter()
            .map(|op| {
                let mut row: Vec<String> = Vec::new();
                let mut col_idx = 0;

                if config.show_id {
                    let val = op.operation_id.clone();
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_parent_id {
                    let val = op.parent_id.clone().unwrap_or_else(|| "-".to_string());
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_name {
                    let val = op.name.clone().unwrap_or_else(|| "-".to_string());
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_type {
                    let val = format!("{}", op.operation_type);
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_status {
                    let val = format!("{}", op.status);
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_start_time {
                    let val = op
                        .start_timestamp
                        .map(format_timestamp)
                        .unwrap_or_else(|| "-".to_string());
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_end_time {
                    let val = op
                        .end_timestamp
                        .map(format_timestamp)
                        .unwrap_or_else(|| "-".to_string());
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_duration {
                    let val = match (op.start_timestamp, op.end_timestamp) {
                        (Some(start), Some(end)) => format_duration(end - start),
                        _ => "-".to_string(),
                    };
                    widths[col_idx] = widths[col_idx].max(val.len());
                    row.push(val);
                    col_idx += 1;
                }
                if config.show_result {
                    let val = if let Some(ref err) = op.error {
                        format!("Error: {}", err.error_message)
                    } else if let Some(result) = op.get_result() {
                        truncate_string(result, 50)
                    } else {
                        "-".to_string()
                    };
                    widths[col_idx] = widths[col_idx].max(val.len().min(50));
                    row.push(val);
                }

                row
            })
            .collect();

        // Print execution status header
        println!("\n=== Execution Result ===");
        println!("Status: {}", self.status);
        if let Some(ref err) = self.error {
            println!("Error: {}", err);
        }
        println!("Operations: {}", self.operations.len());
        println!("Invocations: {}", self.invocations.len());
        println!();

        // Print table header
        print_row(
            &headers.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &widths,
        );
        print_separator(&widths);

        // Print rows
        for row in rows {
            print_row(&row, &widths);
        }

        println!();
    }
}

/// Formats a timestamp (milliseconds since epoch) as a human-readable string.
fn format_timestamp(millis: i64) -> String {
    use chrono::{TimeZone, Utc};
    match Utc.timestamp_millis_opt(millis) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        _ => format!("{}ms", millis),
    }
}

/// Formats a duration in milliseconds as a human-readable string.
fn format_duration(millis: i64) -> String {
    if millis < 1000 {
        format!("{}ms", millis)
    } else if millis < 60_000 {
        format!("{:.2}s", millis as f64 / 1000.0)
    } else if millis < 3_600_000 {
        let mins = millis / 60_000;
        let secs = (millis % 60_000) / 1000;
        format!("{}m {}s", mins, secs)
    } else {
        let hours = millis / 3_600_000;
        let mins = (millis % 3_600_000) / 60_000;
        format!("{}h {}m", hours, mins)
    }
}

/// Truncates a string to the specified length, adding "..." if truncated.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Prints a row with proper column alignment.
fn print_row(row: &[String], widths: &[usize]) {
    let formatted: Vec<String> = row
        .iter()
        .zip(widths.iter())
        .map(|(val, width)| format!("{:<width$}", val, width = width))
        .collect();
    println!("| {} |", formatted.join(" | "));
}

/// Prints a separator line.
fn print_separator(widths: &[usize]) {
    let separators: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("+-{}-+", separators.join("-+-"));
}

// Additional impl block for TestResult with DeserializeOwned bound
impl<T: DeserializeOwned> TestResult<T> {
    /// Attempts to deserialize the result from a JSON string.
    ///
    /// This is useful when the result was stored as a serialized string
    /// and needs to be deserialized into the expected type.
    pub fn deserialize_result_from_json(json: &str) -> Result<T, TestError> {
        serde_json::from_str(json).map_err(TestError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_durable_execution_sdk::{Operation, OperationStatus, OperationType};

    fn create_test_operation(
        name: &str,
        op_type: OperationType,
        status: OperationStatus,
    ) -> Operation {
        let mut op = Operation::new(format!("{}-001", name), op_type);
        op.name = Some(name.to_string());
        op.status = status;
        op
    }

    #[test]
    fn test_success_result() {
        let ops = vec![create_test_operation(
            "step1",
            OperationType::Step,
            OperationStatus::Succeeded,
        )];
        let result: TestResult<String> = TestResult::success("hello".to_string(), ops);

        assert_eq!(result.get_status(), ExecutionStatus::Succeeded);
        assert!(result.is_success());
        assert!(!result.is_failure());
        assert_eq!(result.get_result().unwrap(), "hello");
        assert!(result.get_error().is_err());
    }

    #[test]
    fn test_failure_result() {
        let ops = vec![create_test_operation(
            "step1",
            OperationType::Step,
            OperationStatus::Failed,
        )];
        let error = TestResultError::new("TestError", "Something went wrong");
        let result: TestResult<String> = TestResult::failure(error, ops);

        assert_eq!(result.get_status(), ExecutionStatus::Failed);
        assert!(!result.is_success());
        assert!(result.is_failure());
        assert!(result.get_result().is_err());
        assert!(result.get_error().is_ok());
        assert_eq!(
            result.get_error().unwrap().error_message,
            Some("Something went wrong".to_string())
        );
    }

    #[test]
    fn test_get_operations() {
        let ops = vec![
            create_test_operation("step1", OperationType::Step, OperationStatus::Succeeded),
            create_test_operation("wait1", OperationType::Wait, OperationStatus::Succeeded),
            create_test_operation("step2", OperationType::Step, OperationStatus::Failed),
        ];
        let result: TestResult<String> = TestResult::success("done".to_string(), ops);

        assert_eq!(result.get_operations().len(), 3);
        assert_eq!(result.operation_count(), 3);
    }

    #[test]
    fn test_get_operations_by_status() {
        let ops = vec![
            create_test_operation("step1", OperationType::Step, OperationStatus::Succeeded),
            create_test_operation("wait1", OperationType::Wait, OperationStatus::Succeeded),
            create_test_operation("step2", OperationType::Step, OperationStatus::Failed),
            create_test_operation("step3", OperationType::Step, OperationStatus::Started),
        ];
        let result: TestResult<String> = TestResult::success("done".to_string(), ops);

        let succeeded = result.get_operations_by_status(OperationStatus::Succeeded);
        assert_eq!(succeeded.len(), 2);

        let failed = result.get_operations_by_status(OperationStatus::Failed);
        assert_eq!(failed.len(), 1);

        let started = result.get_operations_by_status(OperationStatus::Started);
        assert_eq!(started.len(), 1);

        let pending = result.get_operations_by_status(OperationStatus::Pending);
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_invocations() {
        let mut result: TestResult<String> = TestResult::success("done".to_string(), vec![]);

        assert_eq!(result.get_invocations().len(), 0);
        assert_eq!(result.invocation_count(), 0);

        result.add_invocation(Invocation::new());
        result.add_invocation(Invocation::new());

        assert_eq!(result.get_invocations().len(), 2);
        assert_eq!(result.invocation_count(), 2);
    }

    #[test]
    fn test_history_events() {
        let mut result: TestResult<String> = TestResult::success("done".to_string(), vec![]);

        assert_eq!(result.get_history_events().len(), 0);

        result.add_history_event(HistoryEvent::new("ExecutionStarted"));
        result.add_history_event(HistoryEvent::new("StepCompleted").with_operation_id("step-001"));

        assert_eq!(result.get_history_events().len(), 2);
        assert_eq!(
            result.get_history_events()[0].event_type,
            "ExecutionStarted"
        );
        assert_eq!(
            result.get_history_events()[1].operation_id,
            Some("step-001".to_string())
        );
    }

    #[test]
    fn test_print_config_default() {
        let config = PrintConfig::default();
        assert!(!config.show_id);
        assert!(!config.show_parent_id);
        assert!(config.show_name);
        assert!(config.show_type);
        assert!(config.show_status);
        assert!(config.show_start_time);
        assert!(config.show_end_time);
        assert!(config.show_duration);
        assert!(!config.show_result);
    }

    #[test]
    fn test_print_config_all() {
        let config = PrintConfig::all();
        assert!(config.show_id);
        assert!(config.show_parent_id);
        assert!(config.show_name);
        assert!(config.show_type);
        assert!(config.show_status);
        assert!(config.show_start_time);
        assert!(config.show_end_time);
        assert!(config.show_duration);
        assert!(config.show_result);
    }

    #[test]
    fn test_print_config_minimal() {
        let config = PrintConfig::minimal();
        assert!(!config.show_id);
        assert!(!config.show_parent_id);
        assert!(config.show_name);
        assert!(config.show_type);
        assert!(config.show_status);
        assert!(!config.show_start_time);
        assert!(!config.show_end_time);
        assert!(!config.show_duration);
        assert!(!config.show_result);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(1500), "1.50s");
        assert_eq!(format_duration(65000), "1m 5s");
        assert_eq!(format_duration(3665000), "1h 1m");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("this is a long string", 10), "this is...");
    }

    #[test]
    fn test_history_event_builder() {
        let event = HistoryEvent::new("TestEvent")
            .with_timestamp(1234567890)
            .with_operation_id("op-001")
            .with_data("some data");

        assert_eq!(event.event_type, "TestEvent");
        assert_eq!(event.timestamp, Some(1234567890));
        assert_eq!(event.operation_id, Some("op-001".to_string()));
        assert_eq!(event.data, Some("some data".to_string()));
    }

    #[test]
    fn test_running_status() {
        let result: TestResult<String> = TestResult::with_status(ExecutionStatus::Running, vec![]);

        assert!(result.is_running());
        assert!(!result.is_success());
        assert!(!result.is_failure());
        assert!(result.get_result().is_err());
        assert!(result.get_error().is_err());
    }

    #[test]
    fn test_cancelled_status() {
        let mut result: TestResult<String> =
            TestResult::with_status(ExecutionStatus::Cancelled, vec![]);
        result.set_error(TestResultError::new(
            "CancelledError",
            "Execution was cancelled",
        ));

        assert!(!result.is_running());
        assert!(!result.is_success());
        assert!(result.is_failure());
        assert!(result.get_result().is_err());
        assert!(result.get_error().is_ok());
    }

    #[test]
    fn test_timed_out_status() {
        let mut result: TestResult<String> =
            TestResult::with_status(ExecutionStatus::TimedOut, vec![]);
        result.set_error(TestResultError::new("TimeoutError", "Execution timed out"));

        assert!(!result.is_running());
        assert!(!result.is_success());
        assert!(result.is_failure());
        assert!(result.get_result().is_err());
        assert!(result.get_error().is_ok());
    }
}

/// Property-based tests for TestResult
///
/// These tests verify the correctness properties defined in the design document.
#[cfg(test)]
mod property_tests {
    use super::*;
    use aws_durable_execution_sdk::{Operation, OperationStatus, OperationType};
    use proptest::prelude::*;

    /// Strategy to generate a random operation type
    fn operation_type_strategy() -> impl Strategy<Value = OperationType> {
        prop_oneof![
            Just(OperationType::Step),
            Just(OperationType::Wait),
            Just(OperationType::Callback),
            Just(OperationType::Invoke),
            Just(OperationType::Context),
        ]
    }

    /// Strategy to generate a random operation status
    fn operation_status_strategy() -> impl Strategy<Value = OperationStatus> {
        prop_oneof![
            Just(OperationStatus::Started),
            Just(OperationStatus::Pending),
            Just(OperationStatus::Ready),
            Just(OperationStatus::Succeeded),
            Just(OperationStatus::Failed),
            Just(OperationStatus::Cancelled),
            Just(OperationStatus::TimedOut),
            Just(OperationStatus::Stopped),
        ]
    }

    /// Strategy to generate a random operation
    fn operation_strategy() -> impl Strategy<Value = Operation> {
        (
            "[a-zA-Z0-9_-]{1,20}", // operation_id
            operation_type_strategy(),
            operation_status_strategy(),
            proptest::option::of("[a-zA-Z0-9_-]{1,20}"), // name
        )
            .prop_map(|(id, op_type, status, name)| {
                let mut op = Operation::new(id, op_type);
                op.status = status;
                op.name = name;
                op
            })
    }

    /// Strategy to generate a list of operations
    fn operations_strategy() -> impl Strategy<Value = Vec<Operation>> {
        prop::collection::vec(operation_strategy(), 0..=20)
    }

    /// Strategy to generate a result value (string for simplicity)
    fn result_value_strategy() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 _-]{0,100}"
    }

    /// Strategy to generate an error
    fn error_strategy() -> impl Strategy<Value = TestResultError> {
        (
            proptest::option::of("[a-zA-Z0-9_]{1,30}"), // error_type
            proptest::option::of("[a-zA-Z0-9 _-]{1,100}"), // error_message
        )
            .prop_map(|(error_type, error_message)| TestResultError {
                error_type,
                error_message,
                error_data: None,
                stack_trace: None,
            })
    }

    proptest! {
        /// **Feature: rust-testing-utilities, Property 5: Result Retrieval Consistency**
        ///
        /// *For any* successful execution with result value V, calling `get_result()` SHALL
        /// return a value equal to V. *For any* failed execution, calling `get_result()`
        /// SHALL return an error.
        ///
        /// **Validates: Requirements 3.2, 3.3**
        #[test]
        fn prop_result_retrieval_consistency(
            result_value in result_value_strategy(),
            error in error_strategy(),
            operations in operations_strategy(),
        ) {
            // Test successful execution
            let success_result: TestResult<String> = TestResult::success(result_value.clone(), operations.clone());

            // get_result() should return the value for successful execution
            let retrieved = success_result.get_result();
            prop_assert!(retrieved.is_ok(), "get_result() should succeed for successful execution");
            prop_assert_eq!(retrieved.unwrap(), &result_value, "Retrieved value should match original");

            // get_error() should fail for successful execution
            let error_result = success_result.get_error();
            prop_assert!(error_result.is_err(), "get_error() should fail for successful execution");

            // Test failed execution
            let failure_result: TestResult<String> = TestResult::failure(error.clone(), operations.clone());

            // get_result() should fail for failed execution
            let retrieved = failure_result.get_result();
            prop_assert!(retrieved.is_err(), "get_result() should fail for failed execution");

            // get_error() should return the error for failed execution
            let error_result = failure_result.get_error();
            prop_assert!(error_result.is_ok(), "get_error() should succeed for failed execution");
            let retrieved_error = error_result.unwrap();
            prop_assert_eq!(&retrieved_error.error_type, &error.error_type, "Error type should match");
            prop_assert_eq!(&retrieved_error.error_message, &error.error_message, "Error message should match");
        }

        /// **Feature: rust-testing-utilities, Property 6: Operation Filtering Correctness**
        ///
        /// *For any* TestResult with operations, calling `get_operations_by_status(S)` SHALL
        /// return only operations with status S, and the count SHALL equal the number of
        /// operations with that status.
        ///
        /// **Validates: Requirements 3.6**
        #[test]
        fn prop_operation_filtering_correctness(
            operations in operations_strategy(),
            filter_status in operation_status_strategy(),
        ) {
            let result: TestResult<String> = TestResult::success("test".to_string(), operations.clone());

            // Get filtered operations
            let filtered = result.get_operations_by_status(filter_status);

            // Count expected operations with the filter status
            let expected_count = operations.iter().filter(|op| op.status == filter_status).count();

            // Verify the count matches
            prop_assert_eq!(
                filtered.len(),
                expected_count,
                "Filtered count should match expected count for status {:?}",
                filter_status
            );

            // Verify all filtered operations have the correct status
            for op in &filtered {
                prop_assert_eq!(
                    op.status,
                    filter_status,
                    "All filtered operations should have status {:?}",
                    filter_status
                );
            }

            // Verify no operations with the filter status were missed
            let all_ops = result.get_operations();
            let missed_count = all_ops
                .iter()
                .filter(|op| op.status == filter_status)
                .filter(|op| !filtered.iter().any(|f| f.operation_id == op.operation_id))
                .count();

            prop_assert_eq!(
                missed_count,
                0,
                "No operations with status {:?} should be missed",
                filter_status
            );
        }
    }
}
