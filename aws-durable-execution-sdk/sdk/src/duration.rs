//! Duration type for durable execution operations.
//!
//! Provides a Duration type with convenient constructors for specifying
//! time intervals in seconds, minutes, hours, days, weeks, months, and years.

use serde::{Deserialize, Serialize};

use crate::error::DurableError;

/// Duration type representing a time interval in seconds.
///
/// Used for configuring timeouts, wait operations, and other time-based settings.
///
/// # Example
///
/// ```
/// use aws_durable_execution_sdk::Duration;
///
/// let five_seconds = Duration::from_seconds(5);
/// let two_minutes = Duration::from_minutes(2);
/// let one_hour = Duration::from_hours(1);
/// let one_day = Duration::from_days(1);
/// let one_week = Duration::from_weeks(1);
/// let one_month = Duration::from_months(1);
/// let one_year = Duration::from_years(1);
///
/// assert_eq!(five_seconds.to_seconds(), 5);
/// assert_eq!(two_minutes.to_seconds(), 120);
/// assert_eq!(one_hour.to_seconds(), 3600);
/// assert_eq!(one_day.to_seconds(), 86400);
/// assert_eq!(one_week.to_seconds(), 604800);
/// assert_eq!(one_month.to_seconds(), 2592000);  // 30 days
/// assert_eq!(one_year.to_seconds(), 31536000);  // 365 days
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub struct Duration {
    seconds: u64,
}

impl Duration {
    /// Creates a new Duration from the given number of seconds.
    ///
    /// # Arguments
    ///
    /// * `seconds` - The number of seconds for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_seconds(30);
    /// assert_eq!(duration.to_seconds(), 30);
    /// ```
    pub fn from_seconds(seconds: u64) -> Self {
        Self { seconds }
    }

    /// Creates a new Duration from the given number of minutes.
    ///
    /// # Arguments
    ///
    /// * `minutes` - The number of minutes for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_minutes(5);
    /// assert_eq!(duration.to_seconds(), 300);
    /// ```
    pub fn from_minutes(minutes: u64) -> Self {
        Self {
            seconds: minutes * 60,
        }
    }

    /// Creates a new Duration from the given number of hours.
    ///
    /// # Arguments
    ///
    /// * `hours` - The number of hours for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_hours(2);
    /// assert_eq!(duration.to_seconds(), 7200);
    /// ```
    pub fn from_hours(hours: u64) -> Self {
        Self {
            seconds: hours * 3600,
        }
    }

    /// Creates a new Duration from the given number of days.
    ///
    /// # Arguments
    ///
    /// * `days` - The number of days for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_days(1);
    /// assert_eq!(duration.to_seconds(), 86400);
    /// ```
    pub fn from_days(days: u64) -> Self {
        Self {
            seconds: days * 86400,
        }
    }

    /// Creates a new Duration from the given number of weeks.
    ///
    /// # Arguments
    ///
    /// * `weeks` - The number of weeks for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_weeks(1);
    /// assert_eq!(duration.to_seconds(), 604800);
    /// ```
    pub fn from_weeks(weeks: u64) -> Self {
        Self {
            seconds: weeks * 604800, // 7 days * 86400 seconds/day
        }
    }

    /// Creates a new Duration from the given number of months.
    ///
    /// A month is defined as 30 days for consistency.
    ///
    /// # Arguments
    ///
    /// * `months` - The number of months for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_months(1);
    /// assert_eq!(duration.to_seconds(), 2592000); // 30 days
    /// ```
    pub fn from_months(months: u64) -> Self {
        Self {
            seconds: months * 2592000, // 30 days * 86400 seconds/day
        }
    }

    /// Creates a new Duration from the given number of years.
    ///
    /// A year is defined as 365 days for consistency.
    ///
    /// # Arguments
    ///
    /// * `years` - The number of years for this duration
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_years(1);
    /// assert_eq!(duration.to_seconds(), 31536000); // 365 days
    /// ```
    pub fn from_years(years: u64) -> Self {
        Self {
            seconds: years * 31536000, // 365 days * 86400 seconds/day
        }
    }

    /// Returns the total number of seconds in this duration.
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let duration = Duration::from_minutes(2);
    /// assert_eq!(duration.to_seconds(), 120);
    /// ```
    pub fn to_seconds(&self) -> u64 {
        self.seconds
    }

    /// Validates that this duration is at least the minimum required for wait operations.
    ///
    /// Wait operations require a minimum duration of 1 second.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the duration is valid (>= 1 second)
    /// * `Err(DurableError::Validation)` if the duration is less than 1 second
    ///
    /// # Example
    ///
    /// ```
    /// use aws_durable_execution_sdk::Duration;
    ///
    /// let valid = Duration::from_seconds(1);
    /// assert!(valid.validate_for_wait().is_ok());
    ///
    /// let invalid = Duration::from_seconds(0);
    /// assert!(invalid.validate_for_wait().is_err());
    /// ```
    pub fn validate_for_wait(&self) -> Result<(), DurableError> {
        if self.seconds < 1 {
            return Err(DurableError::Validation {
                message: "Wait duration must be at least 1 second".to_string(),
            });
        }
        Ok(())
    }
}

impl From<std::time::Duration> for Duration {
    fn from(duration: std::time::Duration) -> Self {
        Self {
            seconds: duration.as_secs(),
        }
    }
}

impl From<Duration> for std::time::Duration {
    fn from(duration: Duration) -> Self {
        std::time::Duration::from_secs(duration.seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_from_seconds() {
        let duration = Duration::from_seconds(42);
        assert_eq!(duration.to_seconds(), 42);
    }

    #[test]
    fn test_from_minutes() {
        let duration = Duration::from_minutes(5);
        assert_eq!(duration.to_seconds(), 300);
    }

    #[test]
    fn test_from_hours() {
        let duration = Duration::from_hours(2);
        assert_eq!(duration.to_seconds(), 7200);
    }

    #[test]
    fn test_from_days() {
        let duration = Duration::from_days(1);
        assert_eq!(duration.to_seconds(), 86400);
    }

    #[test]
    fn test_from_weeks() {
        let duration = Duration::from_weeks(1);
        assert_eq!(duration.to_seconds(), 604800);
    }

    #[test]
    fn test_from_weeks_multiple() {
        let duration = Duration::from_weeks(2);
        assert_eq!(duration.to_seconds(), 1209600);
    }

    #[test]
    fn test_from_months() {
        let duration = Duration::from_months(1);
        assert_eq!(duration.to_seconds(), 2592000); // 30 days
    }

    #[test]
    fn test_from_months_multiple() {
        let duration = Duration::from_months(3);
        assert_eq!(duration.to_seconds(), 7776000); // 90 days
    }

    #[test]
    fn test_from_years() {
        let duration = Duration::from_years(1);
        assert_eq!(duration.to_seconds(), 31536000); // 365 days
    }

    #[test]
    fn test_from_years_multiple() {
        let duration = Duration::from_years(2);
        assert_eq!(duration.to_seconds(), 63072000); // 730 days
    }

    #[test]
    fn test_validate_for_wait_valid() {
        let duration = Duration::from_seconds(1);
        assert!(duration.validate_for_wait().is_ok());

        let duration = Duration::from_seconds(100);
        assert!(duration.validate_for_wait().is_ok());
    }

    #[test]
    fn test_validate_for_wait_invalid() {
        let duration = Duration::from_seconds(0);
        assert!(duration.validate_for_wait().is_err());
    }

    #[test]
    fn test_std_duration_conversion() {
        let std_duration = std::time::Duration::from_secs(60);
        let duration: Duration = std_duration.into();
        assert_eq!(duration.to_seconds(), 60);

        let back: std::time::Duration = duration.into();
        assert_eq!(back.as_secs(), 60);
    }

    // Property-based tests
    // **Feature: durable-execution-rust-sdk, Property 8: Duration Validation**
    // **Validates: Requirements 5.4, 12.7**
    proptest! {
        /// Property: For any Duration value, constructing from seconds/minutes/hours/days
        /// SHALL produce the correct total seconds.
        #[test]
        fn prop_duration_from_seconds_produces_correct_total(seconds in 0u64..=u64::MAX / 86400) {
            let duration = Duration::from_seconds(seconds);
            prop_assert_eq!(duration.to_seconds(), seconds);
        }

        #[test]
        fn prop_duration_from_minutes_produces_correct_total(minutes in 0u64..=u64::MAX / 86400 / 60) {
            let duration = Duration::from_minutes(minutes);
            prop_assert_eq!(duration.to_seconds(), minutes * 60);
        }

        #[test]
        fn prop_duration_from_hours_produces_correct_total(hours in 0u64..=u64::MAX / 86400 / 3600) {
            let duration = Duration::from_hours(hours);
            prop_assert_eq!(duration.to_seconds(), hours * 3600);
        }

        #[test]
        fn prop_duration_from_days_produces_correct_total(days in 0u64..=u64::MAX / 86400 / 86400) {
            let duration = Duration::from_days(days);
            prop_assert_eq!(duration.to_seconds(), days * 86400);
        }

        #[test]
        fn prop_duration_from_weeks_produces_correct_total(weeks in 0u64..=u64::MAX / 604800 / 604800) {
            let duration = Duration::from_weeks(weeks);
            prop_assert_eq!(duration.to_seconds(), weeks * 604800);
        }

        #[test]
        fn prop_duration_from_months_produces_correct_total(months in 0u64..=u64::MAX / 2592000 / 2592000) {
            let duration = Duration::from_months(months);
            prop_assert_eq!(duration.to_seconds(), months * 2592000);
        }

        #[test]
        fn prop_duration_from_years_produces_correct_total(years in 0u64..=u64::MAX / 31536000 / 31536000) {
            let duration = Duration::from_years(years);
            prop_assert_eq!(duration.to_seconds(), years * 31536000);
        }

        /// Property: Wait operations SHALL reject durations less than 1 second.
        #[test]
        fn prop_duration_validate_for_wait_rejects_zero(seconds in 0u64..1) {
            let duration = Duration::from_seconds(seconds);
            prop_assert!(duration.validate_for_wait().is_err());
        }

        /// Property: Wait operations SHALL accept durations >= 1 second.
        #[test]
        fn prop_duration_validate_for_wait_accepts_valid(seconds in 1u64..=1_000_000) {
            let duration = Duration::from_seconds(seconds);
            prop_assert!(duration.validate_for_wait().is_ok());
        }

        /// Property: Duration round-trip through std::time::Duration preserves value.
        #[test]
        fn prop_duration_std_roundtrip(seconds in 0u64..=u64::MAX / 2) {
            let duration = Duration::from_seconds(seconds);
            let std_duration: std::time::Duration = duration.into();
            let back: Duration = std_duration.into();
            prop_assert_eq!(duration, back);
        }
    }
}
