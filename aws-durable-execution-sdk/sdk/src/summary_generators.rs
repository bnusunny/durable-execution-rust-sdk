//! Pre-built summary generators for batch operations.
//!
//! Provides closures suitable for use with `ChildConfig::summary_generator`
//! to produce compact JSON summaries when parallel or map results exceed
//! the 256KB checkpoint size limit.
//!
//! # Example
//!
//! ```
//! use durable_execution_sdk::summary_generators;
//!
//! let gen = summary_generators::parallel_summary();
//! let input = r#"[{"status":"ok"},{"error":"timeout"}]"#;
//! let summary = gen(input);
//! assert!(summary.contains("\"type\":\"parallel\""));
//!
//! let gen = summary_generators::map_summary();
//! let summary = gen(input);
//! assert!(summary.contains("\"type\":\"map\""));
//! ```

/// Creates a summary generator for parallel operation results.
///
/// The returned closure accepts a serialized JSON array of results and
/// produces a JSON string containing:
/// - `type`: `"parallel"`
/// - `totalCount`: total number of elements
/// - `successCount`: number of successful elements
/// - `failureCount`: number of failed elements
/// - `status`: `"completed"` if all succeeded, `"partial"` if some failed,
///   `"failed"` if all failed
///
/// Each array element is considered a failure if it is an object containing
/// an `"error"` field. All other elements are counted as successes.
pub fn parallel_summary() -> impl Fn(&str) -> String {
    build_summary("parallel")
}

/// Creates a summary generator for map operation results.
///
/// The returned closure accepts a serialized JSON array of results and
/// produces a JSON string containing:
/// - `type`: `"map"`
/// - `totalCount`: total number of elements
/// - `successCount`: number of successful elements
/// - `failureCount`: number of failed elements
/// - `status`: `"completed"` if all succeeded, `"partial"` if some failed,
///   `"failed"` if all failed
///
/// Each array element is considered a failure if it is an object containing
/// an `"error"` field. All other elements are counted as successes.
pub fn map_summary() -> impl Fn(&str) -> String {
    build_summary("map")
}

fn build_summary(summary_type: &str) -> impl Fn(&str) -> String {
    let summary_type = summary_type.to_string();
    move |serialized: &str| {
        let items: Vec<serde_json::Value> = serde_json::from_str(serialized).unwrap_or_default();

        let total = items.len();
        let failure_count = items
            .iter()
            .filter(|item| item.is_object() && item.get("error").is_some())
            .count();
        let success_count = total - failure_count;

        let status = if total == 0 || failure_count == total {
            "failed"
        } else if failure_count == 0 {
            "completed"
        } else {
            "partial"
        };

        serde_json::json!({
            "type": summary_type,
            "totalCount": total,
            "successCount": success_count,
            "failureCount": failure_count,
            "status": status,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parallel_summary tests ---

    #[test]
    fn parallel_summary_all_success() {
        let gen = parallel_summary();
        let input = r#"[{"status":"ok"},{"status":"done"},{"status":"ok"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "parallel");
        assert_eq!(parsed["totalCount"], 3);
        assert_eq!(parsed["successCount"], 3);
        assert_eq!(parsed["failureCount"], 0);
        assert_eq!(parsed["status"], "completed");
    }

    #[test]
    fn parallel_summary_all_failure() {
        let gen = parallel_summary();
        let input = r#"[{"error":"timeout"},{"error":"connection refused"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "parallel");
        assert_eq!(parsed["totalCount"], 2);
        assert_eq!(parsed["successCount"], 0);
        assert_eq!(parsed["failureCount"], 2);
        assert_eq!(parsed["status"], "failed");
    }

    #[test]
    fn parallel_summary_partial() {
        let gen = parallel_summary();
        let input = r#"[{"status":"ok"},{"error":"timeout"},{"status":"done"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "parallel");
        assert_eq!(parsed["totalCount"], 3);
        assert_eq!(parsed["successCount"], 2);
        assert_eq!(parsed["failureCount"], 1);
        assert_eq!(parsed["status"], "partial");
    }

    #[test]
    fn parallel_summary_empty_array() {
        let gen = parallel_summary();
        let output = gen("[]");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "parallel");
        assert_eq!(parsed["totalCount"], 0);
        assert_eq!(parsed["successCount"], 0);
        assert_eq!(parsed["failureCount"], 0);
        assert_eq!(parsed["status"], "failed");
    }

    #[test]
    fn parallel_summary_invalid_json_returns_empty() {
        let gen = parallel_summary();
        let output = gen("not json");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "parallel");
        assert_eq!(parsed["totalCount"], 0);
        assert_eq!(parsed["status"], "failed");
    }

    #[test]
    fn parallel_summary_primitive_values_are_successes() {
        let gen = parallel_summary();
        let input = r#"[42, "hello", true, null]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["totalCount"], 4);
        assert_eq!(parsed["successCount"], 4);
        assert_eq!(parsed["failureCount"], 0);
        assert_eq!(parsed["status"], "completed");
    }

    #[test]
    fn parallel_summary_object_without_error_is_success() {
        let gen = parallel_summary();
        let input = r#"[{"result":"ok"},{"value":123}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["successCount"], 2);
        assert_eq!(parsed["failureCount"], 0);
    }

    // --- map_summary tests ---

    #[test]
    fn map_summary_all_success() {
        let gen = map_summary();
        let input = r#"[{"status":"ok"},{"status":"done"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "map");
        assert_eq!(parsed["totalCount"], 2);
        assert_eq!(parsed["successCount"], 2);
        assert_eq!(parsed["failureCount"], 0);
        assert_eq!(parsed["status"], "completed");
    }

    #[test]
    fn map_summary_all_failure() {
        let gen = map_summary();
        let input = r#"[{"error":"e1"},{"error":"e2"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "map");
        assert_eq!(parsed["totalCount"], 2);
        assert_eq!(parsed["successCount"], 0);
        assert_eq!(parsed["failureCount"], 2);
        assert_eq!(parsed["status"], "failed");
    }

    #[test]
    fn map_summary_partial() {
        let gen = map_summary();
        let input = r#"[{"status":"ok"},{"error":"timeout"}]"#;
        let output = gen(input);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "map");
        assert_eq!(parsed["totalCount"], 2);
        assert_eq!(parsed["successCount"], 1);
        assert_eq!(parsed["failureCount"], 1);
        assert_eq!(parsed["status"], "partial");
    }

    #[test]
    fn map_summary_empty_array() {
        let gen = map_summary();
        let output = gen("[]");
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["type"], "map");
        assert_eq!(parsed["totalCount"], 0);
        assert_eq!(parsed["status"], "failed");
    }

    // --- JSON structure tests ---

    #[test]
    fn parallel_summary_output_is_valid_json() {
        let gen = parallel_summary();
        let output = gen(r#"[1,2,3]"#);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&output);
        assert!(parsed.is_ok());
    }

    #[test]
    fn map_summary_output_is_valid_json() {
        let gen = map_summary();
        let output = gen(r#"[1,2,3]"#);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&output);
        assert!(parsed.is_ok());
    }

    #[test]
    fn parallel_summary_contains_all_required_fields() {
        let gen = parallel_summary();
        let output = gen(r#"[{"status":"ok"}]"#);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert!(parsed.get("type").is_some(), "missing 'type' field");
        assert!(parsed.get("totalCount").is_some(), "missing 'totalCount'");
        assert!(
            parsed.get("successCount").is_some(),
            "missing 'successCount'"
        );
        assert!(
            parsed.get("failureCount").is_some(),
            "missing 'failureCount'"
        );
        assert!(parsed.get("status").is_some(), "missing 'status' field");
    }

    #[test]
    fn map_summary_contains_all_required_fields() {
        let gen = map_summary();
        let output = gen(r#"[{"status":"ok"}]"#);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert!(parsed.get("type").is_some(), "missing 'type' field");
        assert!(parsed.get("totalCount").is_some(), "missing 'totalCount'");
        assert!(
            parsed.get("successCount").is_some(),
            "missing 'successCount'"
        );
        assert!(
            parsed.get("failureCount").is_some(),
            "missing 'failureCount'"
        );
        assert!(parsed.get("status").is_some(), "missing 'status' field");
    }
}
