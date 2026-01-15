//! Extended Duration Wait Example
//!
//! Using extended duration helpers for longer wait periods.

use aws_durable_execution_sdk::{durable_execution, Duration, DurableError};

/// Demonstrate various duration helpers.
///
/// The Duration type supports extended time periods including
/// weeks, months, and years.
#[durable_execution]
pub async fn handler(_event: serde_json::Value, ctx: DurableContext) -> Result<String, DurableError> {
    // Standard durations
    ctx.wait(Duration::from_seconds(30), Some("seconds_wait")).await?;
    ctx.wait(Duration::from_minutes(1), Some("minutes_wait")).await?;

    // For demonstration - in practice you'd use appropriate durations
    // ctx.wait(Duration::from_hours(1), Some("hours_wait")).await?;
    // ctx.wait(Duration::from_days(1), Some("days_wait")).await?;
    // ctx.wait(Duration::from_weeks(1), Some("weeks_wait")).await?;

    Ok("Extended duration waits completed".to_string())
}

/// Show duration conversion examples.
pub fn duration_examples() {
    // Standard durations
    let _seconds = Duration::from_seconds(30);
    let _minutes = Duration::from_minutes(5);
    let _hours = Duration::from_hours(2);
    let _days = Duration::from_days(7);

    // Extended durations
    let weeks = Duration::from_weeks(2);      // 14 days
    let months = Duration::from_months(3);    // 90 days (30 days per month)
    let years = Duration::from_years(1);      // 365 days

    assert_eq!(weeks.to_seconds(), 14 * 24 * 60 * 60);
    assert_eq!(months.to_seconds(), 90 * 24 * 60 * 60);
    assert_eq!(years.to_seconds(), 365 * 24 * 60 * 60);
}

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Initialize the default subscriber
    lambda_runtime::tracing::init_default_subscriber();

    lambda_runtime::run(lambda_runtime::service_fn(handler)).await
}
