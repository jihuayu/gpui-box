//! Integer-only proleptic Gregorian calendar arithmetic and civil calendar
//! rules.
//!
//! The algorithms are the classic era-based civil-date conversions. Every
//! operation is integral; there is no floating-point value anywhere in the
//! trigger, calendar or timezone math.

use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Earliest accepted calendar year.
pub const MIN_CALENDAR_YEAR: i32 = 1970;
/// Latest accepted calendar year.
pub const MAX_CALENDAR_YEAR: i32 = 9999;

const MS_PER_DAY: i64 = 86_400_000;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

/// A proleptic Gregorian weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];

    pub const fn index(self) -> u8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }

    pub const fn from_index(index: u8) -> Option<Self> {
        Some(match index % 7 {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            _ => Self::Sunday,
        })
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Monday => "monday",
            Self::Tuesday => "tuesday",
            Self::Wednesday => "wednesday",
            Self::Thursday => "thursday",
            Self::Friday => "friday",
            Self::Saturday => "saturday",
            Self::Sunday => "sunday",
        }
    }

    /// Weekday of a count of days since 1970-01-01 (which was a Thursday).
    pub const fn from_epoch_days(days: i64) -> Self {
        let index = (days + 3).rem_euclid(7) as u8;
        match index {
            0 => Self::Monday,
            1 => Self::Tuesday,
            2 => Self::Wednesday,
            3 => Self::Thursday,
            4 => Self::Friday,
            5 => Self::Saturday,
            _ => Self::Sunday,
        }
    }
}

impl fmt::Display for Weekday {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// True for a proleptic Gregorian leap year.
pub const fn is_leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days in `month` (`1..=12`) of `year`, or `0` for an invalid month.
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since 1970-01-01 for a valid civil date.
///
/// Callers validate the date first. The conversion is exact for every year in
/// the accepted range and remains correct for the full proleptic calendar.
pub fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Civil date `(year, month, day)` for a count of days since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

/// Validates a civil date, including February 29 in leap years and rejection
/// of impossible dates such as February 30 or April 31.
pub fn validate_civil_date(year: i32, month: u8, day: u8) -> Result<(), SystemError> {
    if !(MIN_CALENDAR_YEAR..=MAX_CALENDAR_YEAR).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
    {
        return Err(invalid("Calendar date is not a valid Gregorian date"));
    }
    Ok(())
}

/// Epoch milliseconds for a valid civil date-time at whole seconds.
pub fn epoch_ms_from_civil(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
) -> Result<u64, SystemError> {
    validate_civil_date(year, month, day)?;
    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid("Civil time is outside 00:00:00..=23:59:59"));
    }
    let days = days_from_civil(year, month, day);
    let seconds = i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    let millis = days
        .checked_mul(MS_PER_DAY)
        .and_then(|millis| millis.checked_add(seconds * 1_000))
        .filter(|millis| *millis >= 0)
        .ok_or_else(|| invalid("Civil date-time is outside the supported epoch range"))?;
    u64::try_from(millis)
        .map_err(|_| invalid("Civil date-time is outside the supported epoch range"))
}

/// Broken-down civil time for an epoch-millisecond instant interpreted as UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CivilDateTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: Weekday,
}

/// Splits an epoch-millisecond instant into civil components (UTC reading).
pub fn civil_from_epoch_ms(epoch_ms: u64) -> CivilDateTime {
    let epoch_ms = i64::try_from(epoch_ms).unwrap_or(i64::MAX);
    let days = epoch_ms.div_euclid(MS_PER_DAY);
    let millis_of_day = epoch_ms.rem_euclid(MS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let seconds_of_day = millis_of_day / 1_000;
    CivilDateTime {
        year,
        month,
        day,
        hour: (seconds_of_day / 3_600) as u8,
        minute: ((seconds_of_day % 3_600) / 60) as u8,
        second: (seconds_of_day % 60) as u8,
        weekday: Weekday::from_epoch_days(days),
    }
}

/// One civil calendar recurrence rule, evaluated in a validated timezone.
///
/// All specified fields must match. `month` and `day` combinations are
/// validated against the Gregorian calendar (February 29 is accepted without a
/// year, February 30 is always rejected). `weekday` is an additional filter.
/// The required `hour`/`minute`/`second` fields select the local civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarRule {
    pub year: Option<i32>,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub weekday: Option<Weekday>,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub max_occurrences: Option<u32>,
}

impl CalendarRule {
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            year: None,
            month: None,
            day: None,
            weekday: None,
            hour,
            minute,
            second,
            max_occurrences: None,
        }
    }

    pub const fn with_year(mut self, year: i32) -> Self {
        self.year = Some(year);
        self
    }

    pub const fn with_month(mut self, month: u8) -> Self {
        self.month = Some(month);
        self
    }

    pub const fn with_day(mut self, day: u8) -> Self {
        self.day = Some(day);
        self
    }

    pub const fn with_weekday(mut self, weekday: Weekday) -> Self {
        self.weekday = Some(weekday);
        self
    }

    pub const fn with_max_occurrences(mut self, max_occurrences: u32) -> Self {
        self.max_occurrences = Some(max_occurrences);
        self
    }

    /// Structural validation. Resolvability in a concrete timezone is checked
    /// by the service at submission time.
    pub fn validate(&self) -> Result<(), SystemError> {
        if let Some(year) = self.year
            && !(MIN_CALENDAR_YEAR..=MAX_CALENDAR_YEAR).contains(&year)
        {
            return Err(invalid("Calendar year is outside the supported range"));
        }
        if let Some(month) = self.month
            && !(1..=12).contains(&month)
        {
            return Err(invalid("Calendar month must be in 1..=12"));
        }
        if let Some(day) = self.day {
            if day == 0 {
                return Err(invalid("Calendar day must be at least 1"));
            }
            match self.month {
                Some(month) => {
                    // Without a year, a leap-year upper bound keeps February 29
                    // legal while rejecting February 30.
                    let reference_year = self.year.unwrap_or(2000);
                    if day > days_in_month(reference_year, month) {
                        return Err(invalid("Calendar day does not exist in that month"));
                    }
                }
                None => {
                    if day > 31 {
                        return Err(invalid("Calendar day must be at most 31"));
                    }
                }
            }
        }
        if self.hour > 23 || self.minute > 59 || self.second > 59 {
            return Err(invalid("Calendar time-of-day is outside supported bounds"));
        }
        if self.max_occurrences == Some(0) {
            return Err(invalid("Calendar max_occurrences must be at least 1"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_round_trips_and_weekdays() {
        for (year, month, day, weekday) in [
            (1970, 1, 1, Weekday::Thursday),
            (2000, 2, 29, Weekday::Tuesday),
            (2024, 2, 29, Weekday::Thursday),
            (2026, 11, 15, Weekday::Sunday),
            (9999, 12, 31, Weekday::Friday),
        ] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day));
            assert_eq!(Weekday::from_epoch_days(days), weekday);
            let millis = epoch_ms_from_civil(year, month, day, 12, 34, 56).expect("epoch");
            let civil = civil_from_epoch_ms(millis);
            assert_eq!(
                (
                    civil.year,
                    civil.month,
                    civil.day,
                    civil.hour,
                    civil.minute,
                    civil.second
                ),
                (year, month, day, 12, 34, 56)
            );
            assert_eq!(civil.weekday, weekday);
        }
    }

    #[test]
    fn impossible_dates_are_rejected() {
        assert!(validate_civil_date(2026, 2, 29).is_err());
        assert!(validate_civil_date(2026, 2, 30).is_err());
        assert!(validate_civil_date(2026, 4, 31).is_err());
        assert!(validate_civil_date(2024, 2, 29).is_ok());
        assert!(validate_civil_date(1969, 12, 31).is_err());
        assert!(validate_civil_date(10_000, 1, 1).is_err());
    }

    #[test]
    fn calendar_rule_validation_distinguishes_february_29() {
        assert!(
            CalendarRule::new(9, 0, 0)
                .with_month(2)
                .with_day(29)
                .validate()
                .is_ok()
        );
        assert!(
            CalendarRule::new(9, 0, 0)
                .with_month(2)
                .with_day(30)
                .validate()
                .is_err()
        );
        assert!(
            CalendarRule::new(9, 0, 0)
                .with_month(4)
                .with_day(31)
                .validate()
                .is_err()
        );
        assert!(CalendarRule::new(24, 0, 0).validate().is_err());
        assert!(CalendarRule::new(9, 60, 0).validate().is_err());
        assert!(
            CalendarRule::new(9, 0, 0)
                .with_year(1969)
                .validate()
                .is_err()
        );
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
    }
}
