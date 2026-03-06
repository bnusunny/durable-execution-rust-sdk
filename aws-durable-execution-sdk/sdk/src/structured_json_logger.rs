//! Structured JSON logger for durable execution.
//!
//! This module provides a [`StructuredJsonLogger`] that emits structured JSON log entries
//! to stdout, matching the Node.js DefaultLogger output format. Each log entry includes
//! execution context (requestId, executionArn, tenantId) and optional operation metadata
//! (operationId, attempt, error details).
//!
//! # Requirements
//!
//! - 9.1: Output is valid JSON to stdout
//! - 9.2: Contains level, timestamp, requestId, executionArn, message
//! - 9.3: Includes tenantId when configured
//! - 9.4: Includes operationId from LogInfo
//! - 9.5: Level filtering via min_level

use crate::context::{LogInfo, Logger};
use crate::sealed::Sealed;
use serde::Serialize;
use std::time::SystemTime;

/// Log level for filtering structured JSON output.
///
/// Priority order: Debug(2) < Info(3) < Warn(4) < Error(5).
/// Log calls below the configured `min_level` are suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Debug level (priority 2) — verbose diagnostic information.
    Debug = 2,
    /// Info level (priority 3) — general operational messages.
    Info = 3,
    /// Warn level (priority 4) — potential issues that deserve attention.
    Warn = 4,
    /// Error level (priority 5) — failures requiring investigation.
    Error = 5,
}

/// Context fields included in every JSON log entry.
///
/// Set this on a [`StructuredJsonLogger`] to enrich all log output with
/// execution-level metadata such as `requestId` and `executionArn`.
pub struct JsonLogContext {
    /// The Lambda request ID for this invocation.
    pub request_id: String,
    /// The durable execution ARN.
    pub durable_execution_arn: String,
    /// Optional tenant identifier for multi-tenant workloads.
    pub tenant_id: Option<String>,
}

/// A single structured log entry serialized to JSON.
#[derive(Serialize)]
struct JsonLogEntry {
    level: String,
    timestamp: String,
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "executionArn")]
    execution_arn: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "tenantId")]
    tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "operationId")]
    operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "errorType")]
    error_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "errorMessage")]
    error_message: Option<String>,
}

/// Structured JSON logger for durable execution.
///
/// Emits one JSON object per line to stdout. Each entry includes execution
/// context from [`JsonLogContext`] and optional operation metadata extracted
/// from [`LogInfo`].
///
/// # Level Filtering
///
/// The `min_level` field controls which messages are emitted. Messages with
/// a level below `min_level` are silently dropped.
pub struct StructuredJsonLogger {
    execution_context: Option<JsonLogContext>,
    min_level: LogLevel,
}

impl Sealed for StructuredJsonLogger {}

impl StructuredJsonLogger {
    /// Creates a new `StructuredJsonLogger` with the given minimum log level.
    ///
    /// No execution context is set initially — call [`set_context`](Self::set_context)
    /// to attach request and execution metadata.
    pub fn new(min_level: LogLevel) -> Self {
        Self {
            execution_context: None,
            min_level,
        }
    }

    /// Creates a new `StructuredJsonLogger` with `min_level` derived from the
    /// `AWS_LAMBDA_LOG_LEVEL` environment variable.
    ///
    /// Valid values (case-insensitive): `"DEBUG"`, `"INFO"`, `"WARN"`, `"ERROR"`.
    /// If the variable is not set or contains an invalid value, defaults to
    /// [`LogLevel::Debug`] (all messages emitted).
    ///
    /// # Requirements
    ///
    /// - 10.1: Valid env var → corresponding log level
    /// - 10.2: Missing/invalid env var → defaults to DEBUG
    pub fn from_env() -> Self {
        let min_level = std::env::var("AWS_LAMBDA_LOG_LEVEL")
            .ok()
            .and_then(|val| match val.to_uppercase().as_str() {
                "DEBUG" => Some(LogLevel::Debug),
                "INFO" => Some(LogLevel::Info),
                "WARN" => Some(LogLevel::Warn),
                "ERROR" => Some(LogLevel::Error),
                _ => None,
            })
            .unwrap_or(LogLevel::Debug);

        Self {
            execution_context: None,
            min_level,
        }
    }

    /// Sets the execution context for all subsequent log entries.
    pub fn set_context(&mut self, context: JsonLogContext) {
        self.execution_context = Some(context);
    }

    /// Returns `true` if a message at the given level should be emitted.
    fn should_log(&self, level: LogLevel) -> bool {
        level >= self.min_level
    }

    /// Builds and prints a JSON log entry to stdout.
    fn emit(&self, level_str: &str, level: LogLevel, message: &str, info: &LogInfo) {
        if !self.should_log(level) {
            return;
        }

        if let Some(json) = self.build_json(level_str, message, info) {
            println!("{}", json);
        }
    }

    /// Builds a JSON string for a log entry, or `None` if serialization fails.
    fn build_json(&self, level_str: &str, message: &str, info: &LogInfo) -> Option<String> {
        let (request_id, execution_arn, tenant_id) = match &self.execution_context {
            Some(ctx) => (
                ctx.request_id.clone(),
                ctx.durable_execution_arn.clone(),
                ctx.tenant_id.clone(),
            ),
            None => (String::new(), String::new(), None),
        };

        // Extract optional fields from LogInfo extras
        let mut attempt: Option<u32> = None;
        let mut error_type: Option<String> = None;
        let mut error_message: Option<String> = None;

        for (key, value) in &info.extra {
            match key.as_str() {
                "attempt" => attempt = value.parse().ok(),
                "errorType" => error_type = Some(value.clone()),
                "errorMessage" => error_message = Some(value.clone()),
                _ => {}
            }
        }

        let entry = JsonLogEntry {
            level: level_str.to_string(),
            timestamp: iso8601_now(),
            request_id,
            execution_arn,
            message: message.to_string(),
            tenant_id,
            operation_id: info.operation_id.clone(),
            attempt,
            error_type,
            error_message,
        };

        serde_json::to_string(&entry).ok()
    }
}

impl Logger for StructuredJsonLogger {
    fn debug(&self, message: &str, info: &LogInfo) {
        self.emit("DEBUG", LogLevel::Debug, message, info);
    }

    fn info(&self, message: &str, info: &LogInfo) {
        self.emit("INFO", LogLevel::Info, message, info);
    }

    fn warn(&self, message: &str, info: &LogInfo) {
        self.emit("WARN", LogLevel::Warn, message, info);
    }

    fn error(&self, message: &str, info: &LogInfo) {
        self.emit("ERROR", LogLevel::Error, message, info);
    }
}

/// Returns the current UTC time as an ISO 8601 string with millisecond precision.
fn iso8601_now() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();

    // Convert seconds to date/time components
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Convert days since epoch to year/month/day
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

/// Converts days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::Mutex;

    /// Mutex to serialize tests that mutate the `AWS_LAMBDA_LOG_LEVEL` env var,
    /// preventing race conditions when tests run in parallel.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn make_logger_with_context() -> StructuredJsonLogger {
        let mut logger = StructuredJsonLogger::new(LogLevel::Debug);
        logger.set_context(JsonLogContext {
            request_id: "req-abc-123".to_string(),
            durable_execution_arn:
                "arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:exec-1".to_string(),
            tenant_id: None,
        });
        logger
    }

    #[test]
    fn test_output_is_valid_json() {
        let logger = make_logger_with_context();
        let info = LogInfo::new("arn:aws:test");
        let json_str = logger.build_json("INFO", "hello world", &info).unwrap();
        let parsed: Value = serde_json::from_str(&json_str).expect("output must be valid JSON");
        assert!(parsed.is_object());
    }

    #[test]
    fn test_required_fields_present() {
        let logger = make_logger_with_context();
        let info = LogInfo::default();
        let json_str = logger.build_json("WARN", "test message", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["level"], "WARN");
        assert!(v["timestamp"].as_str().unwrap().ends_with('Z'));
        assert_eq!(v["requestId"], "req-abc-123");
        assert_eq!(
            v["executionArn"],
            "arn:aws:lambda:us-east-1:123456789012:function:my-func:durable:exec-1"
        );
        assert_eq!(v["message"], "test message");
    }

    #[test]
    fn test_timestamp_iso8601_format() {
        let logger = make_logger_with_context();
        let info = LogInfo::default();
        let json_str = logger.build_json("DEBUG", "ts check", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();
        let ts = v["timestamp"].as_str().unwrap();
        // ISO 8601: YYYY-MM-DDTHH:MM:SS.mmmZ
        assert_eq!(ts.len(), 24, "timestamp should be 24 chars: {}", ts);
        assert!(ts.contains('T'));
        assert!(ts.ends_with('Z'));
    }

    #[test]
    fn test_optional_fields_absent_when_not_set() {
        let logger = make_logger_with_context();
        let info = LogInfo::default();
        let json_str = logger.build_json("INFO", "minimal", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert!(v.get("tenantId").is_none());
        assert!(v.get("operationId").is_none());
        assert!(v.get("attempt").is_none());
        assert!(v.get("errorType").is_none());
        assert!(v.get("errorMessage").is_none());
    }

    #[test]
    fn test_tenant_id_included_when_configured() {
        let mut logger = StructuredJsonLogger::new(LogLevel::Debug);
        logger.set_context(JsonLogContext {
            request_id: "req-1".to_string(),
            durable_execution_arn: "arn:test".to_string(),
            tenant_id: Some("tenant-xyz".to_string()),
        });
        let info = LogInfo::default();
        let json_str = logger.build_json("INFO", "with tenant", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["tenantId"], "tenant-xyz");
    }

    #[test]
    fn test_operation_id_from_log_info() {
        let logger = make_logger_with_context();
        let info = LogInfo::default().with_operation_id("op-456");
        let json_str = logger.build_json("INFO", "with op", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["operationId"], "op-456");
    }

    #[test]
    fn test_attempt_from_extras() {
        let logger = make_logger_with_context();
        let info = LogInfo::default().with_extra("attempt", "3");
        let json_str = logger.build_json("INFO", "retry", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["attempt"], 3);
    }

    #[test]
    fn test_error_fields_from_extras() {
        let logger = make_logger_with_context();
        let info = LogInfo::default()
            .with_extra("errorType", "TimeoutError")
            .with_extra("errorMessage", "connection timed out");
        let json_str = logger.build_json("ERROR", "failed", &info).unwrap();
        let v: Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(v["errorType"], "TimeoutError");
        assert_eq!(v["errorMessage"], "connection timed out");
    }

    #[test]
    fn test_level_filtering_debug_allows_all() {
        let logger = StructuredJsonLogger::new(LogLevel::Debug);
        assert!(logger.should_log(LogLevel::Debug));
        assert!(logger.should_log(LogLevel::Info));
        assert!(logger.should_log(LogLevel::Warn));
        assert!(logger.should_log(LogLevel::Error));
    }

    #[test]
    fn test_level_filtering_warn_suppresses_debug_and_info() {
        let logger = StructuredJsonLogger::new(LogLevel::Warn);
        assert!(!logger.should_log(LogLevel::Debug));
        assert!(!logger.should_log(LogLevel::Info));
        assert!(logger.should_log(LogLevel::Warn));
        assert!(logger.should_log(LogLevel::Error));
    }

    #[test]
    fn test_level_filtering_error_only() {
        let logger = StructuredJsonLogger::new(LogLevel::Error);
        assert!(!logger.should_log(LogLevel::Debug));
        assert!(!logger.should_log(LogLevel::Info));
        assert!(!logger.should_log(LogLevel::Warn));
        assert!(logger.should_log(LogLevel::Error));
    }

    #[test]
    fn test_level_filtering_info_suppresses_debug() {
        let logger = StructuredJsonLogger::new(LogLevel::Info);
        assert!(!logger.should_log(LogLevel::Debug));
        assert!(logger.should_log(LogLevel::Info));
        assert!(logger.should_log(LogLevel::Warn));
        assert!(logger.should_log(LogLevel::Error));
    }

    #[test]
    fn test_logger_trait_methods_set_correct_level() {
        let mut logger = StructuredJsonLogger::new(LogLevel::Debug);
        logger.set_context(JsonLogContext {
            request_id: "r".to_string(),
            durable_execution_arn: "a".to_string(),
            tenant_id: None,
        });
        let info = LogInfo::default();

        // Verify each trait method produces the correct level string
        let debug_json = logger.build_json("DEBUG", "d", &info).unwrap();
        let info_json = logger.build_json("INFO", "i", &info).unwrap();
        let warn_json = logger.build_json("WARN", "w", &info).unwrap();
        let error_json = logger.build_json("ERROR", "e", &info).unwrap();

        let dv: Value = serde_json::from_str(&debug_json).unwrap();
        let iv: Value = serde_json::from_str(&info_json).unwrap();
        let wv: Value = serde_json::from_str(&warn_json).unwrap();
        let ev: Value = serde_json::from_str(&error_json).unwrap();

        assert_eq!(dv["level"], "DEBUG");
        assert_eq!(iv["level"], "INFO");
        assert_eq!(wv["level"], "WARN");
        assert_eq!(ev["level"], "ERROR");
    }

    // --- AWS_LAMBDA_LOG_LEVEL env var tests (Reqs 10.1, 10.2) ---

    #[test]
    fn test_from_env_debug() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "DEBUG");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Debug);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_info() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "INFO");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Info);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_warn() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "WARN");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Warn);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_error() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "ERROR");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Error);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_case_insensitive() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "warn");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Warn);

        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "Info");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Info);

        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "error");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Error);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_invalid_defaults_to_debug() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "TRACE");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Debug);

        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "garbage");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Debug);

        std::env::set_var("AWS_LAMBDA_LOG_LEVEL", "");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Debug);
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
    }

    #[test]
    fn test_from_env_missing_defaults_to_debug() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("AWS_LAMBDA_LOG_LEVEL");
        let logger = StructuredJsonLogger::from_env();
        assert_eq!(logger.min_level, LogLevel::Debug);
    }
}
