//! Validated timezone identity, deterministic DST transition tables and
//! integer local-civil resolution with explicit gap and fold policies.
//!
//! Named zones are never guessed. A named zone is accepted only when an
//! injected [`TimeZoneResolver`] can supply a deterministic transition table;
//! the built-in resolver contains a bounded table for a small set of IANA zones
//! and fails closed outside its covered range. No floating-point arithmetic is
//! used.

use crate::calendar::{
    CalendarRule, CivilDateTime, Weekday, civil_from_epoch_ms, days_from_civil, days_in_month,
    epoch_ms_from_civil,
};
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Smallest accepted fixed offset (UTC-14:00).
pub const MIN_FIXED_OFFSET_MINUTES: i16 = -840;
/// Largest accepted fixed offset (UTC+14:00).
pub const MAX_FIXED_OFFSET_MINUTES: i16 = 840;
/// Largest accepted IANA zone identifier in bytes.
pub const MAX_TIME_ZONE_ID_BYTES: usize = 64;
/// Largest accepted number of `/`-separated zone identifier segments.
pub const MAX_TIME_ZONE_SEGMENTS: usize = 8;
/// First year in the built-in DST table.
pub const BUILTIN_DST_FIRST_YEAR: i32 = 2024;
/// Last year in the built-in DST table.
pub const BUILTIN_DST_LAST_YEAR: i32 = 2035;

const MINUTE_MS: i64 = 60_000;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn unsupported(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::Unsupported, message)
}

/// A validated timezone for calendar trigger evaluation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeZoneSpec {
    Utc,
    FixedOffsetMinutes(i16),
    Named(String),
}

impl TimeZoneSpec {
    /// UTC is the default; a schedule that does not specify a zone is absolute.
    pub const fn utc() -> Self {
        Self::Utc
    }

    /// Validate a fixed offset or the shape of a named zone. Named-zone support
    /// is resolved separately and fails closed when no table is installed.
    pub fn validate_shape(&self) -> Result<(), SystemError> {
        match self {
            Self::Utc => Ok(()),
            Self::FixedOffsetMinutes(minutes) => {
                if !(MIN_FIXED_OFFSET_MINUTES..=MAX_FIXED_OFFSET_MINUTES).contains(minutes) {
                    return Err(invalid("Fixed timezone offset is outside -840..=840"));
                }
                Ok(())
            }
            Self::Named(identifier) => validate_zone_identifier(identifier),
        }
    }

    pub const fn fixed_offset_minutes(&self) -> Option<i16> {
        match self {
            Self::FixedOffsetMinutes(minutes) => Some(*minutes),
            Self::Utc | Self::Named(_) => None,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Utc => "UTC",
            Self::FixedOffsetMinutes(_) => "fixed-offset",
            Self::Named(identifier) => identifier,
        }
    }
}

impl fmt::Display for TimeZoneSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utc => formatter.write_str("UTC"),
            Self::FixedOffsetMinutes(minutes) => {
                let sign = if *minutes < 0 { '-' } else { '+' };
                let absolute = minutes.unsigned_abs();
                write!(
                    formatter,
                    "UTC{sign}{:02}:{:02}",
                    absolute / 60,
                    absolute % 60
                )
            }
            Self::Named(identifier) => formatter.write_str(identifier),
        }
    }
}

fn validate_zone_identifier(identifier: &str) -> Result<(), SystemError> {
    if identifier.is_empty()
        || identifier.len() > MAX_TIME_ZONE_ID_BYTES
        || !identifier.is_ascii()
        || identifier.starts_with('/')
        || identifier.ends_with('/')
        || identifier.contains("//")
        || identifier.contains("..")
        || identifier.chars().any(char::is_whitespace)
    {
        return Err(invalid(
            "Named timezone identifier is not a bounded IANA id",
        ));
    }
    let segments: Vec<_> = identifier.split('/').collect();
    if segments.is_empty() || segments.len() > MAX_TIME_ZONE_SEGMENTS {
        return Err(invalid("Named timezone has too many path segments"));
    }
    for segment in segments {
        if segment.is_empty()
            || segment.len() > 32
            || segment.starts_with('.')
            || segment.ends_with('.')
            || !segment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'+' | b'-' | b'.')
            })
        {
            return Err(invalid("Named timezone path segment is invalid"));
        }
    }
    Ok(())
}

/// Policy for a nonexistent local civil time during a spring-forward gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DstGapPolicy {
    /// Fire at the transition instant, whose post-transition local time is the
    /// first valid time after the gap (for 02:30 in a 02:00->03:00 gap this is
    /// the 03:00 post-transition instant).
    ShiftForward,
    /// Skip that occurrence and continue to the next matching civil time.
    SkipOccurrence,
}

/// Policy for an ambiguous local civil time during a fall-back fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DstFoldPolicy {
    /// Select the earlier pre-transition instant (the first occurrence).
    FirstOccurrence,
    /// Select the later post-transition instant (the second occurrence).
    SecondOccurrence,
}

/// One deterministic offset change in a transition table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DstTransitionPoint {
    /// UTC epoch milliseconds at which the new offset takes effect.
    pub at_epoch_ms: u64,
    /// Offset minutes in effect at and after this instant.
    pub offset_minutes: i16,
}

/// An explicit, deterministic civil/UTC transition table for one named zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DstTransitionRule {
    pub zone_id: String,
    pub standard_offset_minutes: i16,
    pub daylight_offset_minutes: i16,
    pub transitions: Vec<DstTransitionPoint>,
    pub covered_from_epoch_ms: u64,
    pub covered_through_epoch_ms: u64,
}

impl DstTransitionRule {
    pub fn validate_shape(&self) -> Result<(), SystemError> {
        validate_zone_identifier(&self.zone_id)?;
        if !(MIN_FIXED_OFFSET_MINUTES..=MAX_FIXED_OFFSET_MINUTES)
            .contains(&self.standard_offset_minutes)
            || !(MIN_FIXED_OFFSET_MINUTES..=MAX_FIXED_OFFSET_MINUTES)
                .contains(&self.daylight_offset_minutes)
        {
            return Err(invalid("DST offsets are outside -840..=840"));
        }
        if self.covered_from_epoch_ms > self.covered_through_epoch_ms {
            return Err(invalid("DST coverage range is inverted"));
        }
        let mut previous = None;
        for transition in &self.transitions {
            if !(MIN_FIXED_OFFSET_MINUTES..=MAX_FIXED_OFFSET_MINUTES)
                .contains(&transition.offset_minutes)
            {
                return Err(invalid("DST transition offset is outside -840..=840"));
            }
            if previous.is_some_and(|value| transition.at_epoch_ms <= value) {
                return Err(invalid("DST transitions must be strictly increasing"));
            }
            if transition.at_epoch_ms < self.covered_from_epoch_ms
                || transition.at_epoch_ms > self.covered_through_epoch_ms
            {
                return Err(invalid("DST transition is outside its covered range"));
            }
            previous = Some(transition.at_epoch_ms);
        }
        Ok(())
    }

    pub fn covers(&self, utc_epoch_ms: u64) -> bool {
        (self.covered_from_epoch_ms..=self.covered_through_epoch_ms).contains(&utc_epoch_ms)
    }

    pub fn offset_at(&self, utc_epoch_ms: u64) -> Option<i16> {
        if !self.covers(utc_epoch_ms) {
            return None;
        }
        let index = self
            .transitions
            .partition_point(|transition| transition.at_epoch_ms <= utc_epoch_ms);
        if index == 0 {
            Some(self.standard_offset_minutes)
        } else {
            Some(self.transitions[index - 1].offset_minutes)
        }
    }

    fn offset_before(&self, index: usize) -> i16 {
        if index == 0 {
            self.standard_offset_minutes
        } else {
            self.transitions[index - 1].offset_minutes
        }
    }

    pub fn offsets(&self) -> Vec<i16> {
        let mut offsets = BTreeSet::new();
        offsets.insert(self.standard_offset_minutes);
        offsets.insert(self.daylight_offset_minutes);
        offsets.extend(self.transitions.iter().map(|point| point.offset_minutes));
        offsets.into_iter().collect()
    }
}

/// A resolved timezone with an optional deterministic transition table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTimeZone {
    pub spec: TimeZoneSpec,
    pub rule: Option<DstTransitionRule>,
}

impl ResolvedTimeZone {
    pub const fn utc() -> Self {
        Self {
            spec: TimeZoneSpec::Utc,
            rule: None,
        }
    }

    pub const fn fixed(minutes: i16) -> Self {
        Self {
            spec: TimeZoneSpec::FixedOffsetMinutes(minutes),
            rule: None,
        }
    }

    pub fn offset_at(&self, utc_epoch_ms: u64) -> Result<i16, SystemError> {
        match &self.spec {
            TimeZoneSpec::Utc => Ok(0),
            TimeZoneSpec::FixedOffsetMinutes(minutes) => Ok(*minutes),
            TimeZoneSpec::Named(_) => self
                .rule
                .as_ref()
                .and_then(|rule| rule.offset_at(utc_epoch_ms))
                .ok_or_else(|| {
                    unsupported("Named timezone transition table does not cover this instant")
                }),
        }
    }

    pub fn offsets(&self) -> Vec<i16> {
        match &self.spec {
            TimeZoneSpec::Utc => vec![0],
            TimeZoneSpec::FixedOffsetMinutes(minutes) => vec![*minutes],
            TimeZoneSpec::Named(_) => self
                .rule
                .as_ref()
                .map(DstTransitionRule::offsets)
                .unwrap_or_default(),
        }
    }

    /// Smallest UTC offset this zone can be in. A local civil instant minus the
    /// smallest offset is the latest UTC instant that could map to it.
    fn min_offset_minutes(&self) -> i16 {
        match &self.spec {
            TimeZoneSpec::Utc => 0,
            TimeZoneSpec::FixedOffsetMinutes(minutes) => *minutes,
            TimeZoneSpec::Named(_) => self.rule.as_ref().map_or(0, |rule| {
                rule.standard_offset_minutes
                    .min(rule.daylight_offset_minutes)
            }),
        }
    }

    /// Resolve one local civil instant, applying the explicit gap/fold policy.
    pub fn resolve_local(
        &self,
        local_epoch_ms: u64,
        gap: DstGapPolicy,
        fold: DstFoldPolicy,
    ) -> Result<LocalResolution, SystemError> {
        let local = i128::from(local_epoch_ms);
        let mut candidates = BTreeSet::new();
        let mut saw_uncovered = false;
        for offset in self.offsets() {
            let utc = local - i128::from(offset) * i128::from(MINUTE_MS);
            if utc < 0 || utc > i128::from(u64::MAX) {
                continue;
            }
            let utc = utc as u64;
            if self.rule.as_ref().is_some_and(|rule| !rule.covers(utc)) {
                saw_uncovered = true;
                continue;
            }
            if self.offset_at(utc)? == offset {
                candidates.insert(utc);
            }
        }
        if candidates.len() == 1 {
            return Ok(LocalResolution::Unique(
                *candidates.first().expect("one candidate"),
            ));
        }
        if candidates.len() > 1 {
            let chosen = match fold {
                DstFoldPolicy::FirstOccurrence => candidates.first(),
                DstFoldPolicy::SecondOccurrence => candidates.last(),
            };
            return Ok(LocalResolution::Unique(
                *chosen.expect("more than one candidate"),
            ));
        }
        if let Some(rule) = &self.rule {
            for (index, transition) in rule.transitions.iter().enumerate() {
                let before = rule.offset_before(index);
                if transition.offset_minutes <= before {
                    continue;
                }
                let local_pre =
                    i128::from(transition.at_epoch_ms) + i128::from(before) * i128::from(MINUTE_MS);
                let local_post = i128::from(transition.at_epoch_ms)
                    + i128::from(transition.offset_minutes) * i128::from(MINUTE_MS);
                if local >= local_pre && local < local_post {
                    return Ok(match gap {
                        DstGapPolicy::ShiftForward => {
                            LocalResolution::GapShifted(transition.at_epoch_ms)
                        }
                        DstGapPolicy::SkipOccurrence => LocalResolution::Skipped,
                    });
                }
            }
        }
        if saw_uncovered {
            return Err(unsupported(
                "Named timezone transition table does not cover this instant",
            ));
        }
        Err(invalid(
            "Local civil time cannot be resolved in the selected timezone",
        ))
    }
}

/// The result of resolving a local civil time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalResolution {
    /// Exactly one UTC instant maps to this local time.
    Unique(u64),
    /// The local time did not exist; the transition instant was selected.
    GapShifted(u64),
    /// The local occurrence does not exist and the gap policy says to skip it.
    Skipped,
}

impl LocalResolution {
    pub const fn instant(self) -> Option<u64> {
        match self {
            Self::Unique(value) | Self::GapShifted(value) => Some(value),
            Self::Skipped => None,
        }
    }
}

/// Injectable deterministic timezone resolver.
pub trait TimeZoneResolver: Send + Sync + 'static {
    fn resolve(&self, spec: &TimeZoneSpec) -> Result<ResolvedTimeZone, SystemError>;
}

/// A built-in resolver backed by an explicit transition table for a bounded
/// set of IANA zones. Unknown zones fail closed; they are never approximated.
#[derive(Debug, Clone, Default)]
pub struct BuiltinTimeZoneResolver {
    zones: BTreeMap<String, DstTransitionRule>,
}

impl BuiltinTimeZoneResolver {
    pub fn new() -> Self {
        let mut zones = BTreeMap::new();
        zones.insert(
            String::from("UTC"),
            fixed_rule("UTC", 0).expect("built-in UTC rule"),
        );
        zones.insert(
            String::from("Etc/UTC"),
            fixed_rule("Etc/UTC", 0).expect("built-in Etc/UTC rule"),
        );
        zones.insert(
            String::from("Asia/Tokyo"),
            fixed_rule("Asia/Tokyo", 540).expect("built-in Tokyo rule"),
        );
        zones.insert(
            String::from("Asia/Kathmandu"),
            fixed_rule("Asia/Kathmandu", 345).expect("built-in Kathmandu rule"),
        );
        zones.insert(
            String::from("America/New_York"),
            us_rule("America/New_York", -300, -240).expect("built-in New York rule"),
        );
        zones.insert(
            String::from("Europe/London"),
            eu_rule("Europe/London", 0, 60).expect("built-in London rule"),
        );
        zones.insert(
            String::from("Europe/Berlin"),
            eu_rule("Europe/Berlin", 60, 120).expect("built-in Berlin rule"),
        );
        zones.insert(
            String::from("Australia/Sydney"),
            sydney_rule("Australia/Sydney", 600, 660).expect("built-in Sydney rule"),
        );
        Self { zones }
    }

    pub fn zone_ids(&self) -> Vec<&str> {
        self.zones.keys().map(String::as_str).collect()
    }
}

impl TimeZoneResolver for BuiltinTimeZoneResolver {
    fn resolve(&self, spec: &TimeZoneSpec) -> Result<ResolvedTimeZone, SystemError> {
        spec.validate_shape()?;
        match spec {
            TimeZoneSpec::Utc => Ok(ResolvedTimeZone::utc()),
            TimeZoneSpec::FixedOffsetMinutes(minutes) => Ok(ResolvedTimeZone::fixed(*minutes)),
            TimeZoneSpec::Named(identifier) => {
                let rule = self.zones.get(identifier).cloned().ok_or_else(|| {
                    unsupported("Named timezone is not present in the deterministic table")
                })?;
                Ok(ResolvedTimeZone {
                    spec: spec.clone(),
                    rule: Some(rule),
                })
            }
        }
    }
}

fn civil_local_ms(
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
) -> Result<i128, SystemError> {
    Ok(i128::from(epoch_ms_from_civil(
        year, month, day, hour, minute, 0,
    )?))
}

fn utc_from_local(local: i128, offset_minutes: i16) -> Result<u64, SystemError> {
    let utc = local - i128::from(offset_minutes) * i128::from(MINUTE_MS);
    if utc < 0 || utc > i128::from(u64::MAX) {
        return Err(invalid("Transition instant is outside the epoch range"));
    }
    Ok(utc as u64)
}

fn coverage() -> Result<(u64, u64), SystemError> {
    Ok((
        epoch_ms_from_civil(BUILTIN_DST_FIRST_YEAR, 1, 1, 0, 0, 0)?,
        epoch_ms_from_civil(BUILTIN_DST_LAST_YEAR + 1, 1, 1, 0, 0, 0)? - 1,
    ))
}

fn fixed_rule(zone_id: &str, offset: i16) -> Result<DstTransitionRule, SystemError> {
    let rule = DstTransitionRule {
        zone_id: zone_id.to_owned(),
        standard_offset_minutes: offset,
        daylight_offset_minutes: offset,
        transitions: Vec::new(),
        covered_from_epoch_ms: 0,
        covered_through_epoch_ms: u64::MAX,
    };
    rule.validate_shape()?;
    Ok(rule)
}

fn first_weekday_day(year: i32, month: u8, weekday: Weekday) -> u8 {
    let first = Weekday::from_epoch_days(days_from_civil(year, month, 1)).index();
    let delta = (weekday.index() + 7 - first) % 7;
    1 + delta
}

fn nth_weekday_day(year: i32, month: u8, weekday: Weekday, nth: u8) -> u8 {
    first_weekday_day(year, month, weekday) + 7 * (nth.saturating_sub(1))
}

fn last_weekday_day(year: i32, month: u8, weekday: Weekday) -> u8 {
    let last = days_in_month(year, month);
    let weekday_first = Weekday::from_epoch_days(days_from_civil(year, month, 1));
    let last_weekday = Weekday::from_epoch_days(days_from_civil(year, month, last)).index();
    let delta = (last_weekday + 7 - weekday.index()) % 7;
    let _ = weekday_first;
    last - delta
}

fn us_rule(zone_id: &str, standard: i16, daylight: i16) -> Result<DstTransitionRule, SystemError> {
    let mut transitions = Vec::new();
    for year in BUILTIN_DST_FIRST_YEAR..=BUILTIN_DST_LAST_YEAR {
        let start_day = nth_weekday_day(year, 3, Weekday::Sunday, 2);
        let end_day = first_weekday_day(year, 11, Weekday::Sunday);
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 3, start_day, 2, 0)?, standard)?,
            offset_minutes: daylight,
        });
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 11, end_day, 2, 0)?, daylight)?,
            offset_minutes: standard,
        });
    }
    transitions.sort_by_key(|point| point.at_epoch_ms);
    transition_rule(zone_id, standard, daylight, transitions)
}

fn eu_rule(zone_id: &str, standard: i16, daylight: i16) -> Result<DstTransitionRule, SystemError> {
    let mut transitions = Vec::new();
    for year in BUILTIN_DST_FIRST_YEAR..=BUILTIN_DST_LAST_YEAR {
        let start_day = last_weekday_day(year, 3, Weekday::Sunday);
        let end_day = last_weekday_day(year, 10, Weekday::Sunday);
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 3, start_day, 1, 0)?, 0)?,
            offset_minutes: daylight,
        });
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 10, end_day, 1, 0)?, 0)?,
            offset_minutes: standard,
        });
    }
    transitions.sort_by_key(|point| point.at_epoch_ms);
    transition_rule(zone_id, standard, daylight, transitions)
}

fn sydney_rule(
    zone_id: &str,
    standard: i16,
    daylight: i16,
) -> Result<DstTransitionRule, SystemError> {
    let mut transitions = Vec::new();
    for year in BUILTIN_DST_FIRST_YEAR..=BUILTIN_DST_LAST_YEAR {
        let start_day = first_weekday_day(year, 10, Weekday::Sunday);
        let end_day = first_weekday_day(year, 4, Weekday::Sunday);
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 10, start_day, 2, 0)?, standard)?,
            offset_minutes: daylight,
        });
        transitions.push(DstTransitionPoint {
            at_epoch_ms: utc_from_local(civil_local_ms(year, 4, end_day, 3, 0)?, daylight)?,
            offset_minutes: standard,
        });
    }
    transitions.sort_by_key(|point| point.at_epoch_ms);
    transition_rule(zone_id, standard, daylight, transitions)
}

fn transition_rule(
    zone_id: &str,
    standard: i16,
    daylight: i16,
    transitions: Vec<DstTransitionPoint>,
) -> Result<DstTransitionRule, SystemError> {
    let (covered_from_epoch_ms, covered_through_epoch_ms) = coverage()?;
    let rule = DstTransitionRule {
        zone_id: zone_id.to_owned(),
        standard_offset_minutes: standard,
        daylight_offset_minutes: daylight,
        transitions,
        covered_from_epoch_ms,
        covered_through_epoch_ms,
    };
    rule.validate_shape()?;
    Ok(rule)
}

/// Returns the next matching calendar occurrence strictly after
/// `after_epoch_ms`, or `None` when the rule is exhausted.
pub fn next_calendar_occurrence(
    rule: &CalendarRule,
    zone: &ResolvedTimeZone,
    after_epoch_ms: u64,
    gap: DstGapPolicy,
    fold: DstFoldPolicy,
) -> Result<Option<u64>, SystemError> {
    rule.validate()?;
    let after = civil_from_epoch_ms(after_epoch_ms);
    let (first_year, last_year) = match rule.year {
        Some(year) => (year, year),
        None => {
            // `after` is UTC, so a rule close to midnight can still match the
            // previous local year. Start one year earlier and use a horizon
            // wide enough for Feb 29 + weekday patterns across non-leap
            // century boundaries (up to 40 years).
            let start = after
                .year
                .saturating_sub(1)
                .max(crate::calendar::MIN_CALENDAR_YEAR);
            let end = start
                .saturating_add(48)
                .min(crate::calendar::MAX_CALENDAR_YEAR);
            (start, end)
        }
    };
    for year in first_year..=last_year {
        let months: Vec<u8> = match rule.month {
            Some(month) => vec![month],
            None => (1..=12).collect(),
        };
        for month in months {
            let max_day = days_in_month(year, month);
            let days: Vec<u8> = match rule.day {
                Some(day) => {
                    if day > max_day {
                        Vec::new()
                    } else {
                        vec![day]
                    }
                }
                None => (1..=max_day).collect(),
            };
            for day in days {
                if let Some(weekday) = rule.weekday
                    && Weekday::from_epoch_days(days_from_civil(year, month, day)) != weekday
                {
                    continue;
                }
                let local =
                    epoch_ms_from_civil(year, month, day, rule.hour, rule.minute, rule.second)?;
                // A local instant can map to UTC only within
                // `[local - max_offset, local - min_offset]`. Skip candidates
                // whose latest possible instant is not strictly after the
                // cursor, so an uncovered year at the start of the scan (for
                // example 2023 against a table covering 2024 onward) cannot
                // abort resolution before a covered occurrence is reached.
                let max_possible_utc = i128::from(local)
                    - i128::from(zone.min_offset_minutes()) * i128::from(MINUTE_MS);
                if max_possible_utc <= i128::from(after_epoch_ms) {
                    continue;
                }
                match zone.resolve_local(local, gap, fold)? {
                    LocalResolution::Skipped => continue,
                    LocalResolution::Unique(instant) | LocalResolution::GapShifted(instant) => {
                        if instant > after_epoch_ms {
                            return Ok(Some(instant));
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Splits a local civil instant into components for diagnostics and tests.
pub fn local_civil(
    zone: &ResolvedTimeZone,
    utc_epoch_ms: u64,
) -> Result<CivilDateTime, SystemError> {
    let offset = zone.offset_at(utc_epoch_ms)?;
    let local = i128::from(utc_epoch_ms) + i128::from(offset) * i128::from(MINUTE_MS);
    if local < 0 || local > i128::from(u64::MAX) {
        return Err(invalid("Local instant is outside the epoch range"));
    }
    Ok(civil_from_epoch_ms(local as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::epoch_ms_from_civil;

    fn new_york() -> ResolvedTimeZone {
        BuiltinTimeZoneResolver::new()
            .resolve(&TimeZoneSpec::Named(String::from("America/New_York")))
            .expect("New York")
    }

    #[test]
    fn fixed_and_named_specs_validate() {
        assert!(TimeZoneSpec::Utc.validate_shape().is_ok());
        assert!(
            TimeZoneSpec::FixedOffsetMinutes(840)
                .validate_shape()
                .is_ok()
        );
        assert!(
            TimeZoneSpec::FixedOffsetMinutes(-840)
                .validate_shape()
                .is_ok()
        );
        assert!(
            TimeZoneSpec::FixedOffsetMinutes(841)
                .validate_shape()
                .is_err()
        );
        assert!(
            TimeZoneSpec::Named(String::from("America/New_York"))
                .validate_shape()
                .is_ok()
        );
        assert!(
            TimeZoneSpec::Named(String::from("../etc/passwd"))
                .validate_shape()
                .is_err()
        );
        assert!(
            TimeZoneSpec::Named(String::from(""))
                .validate_shape()
                .is_err()
        );
        let resolver = BuiltinTimeZoneResolver::new();
        assert!(
            resolver
                .resolve(&TimeZoneSpec::Named(String::from("Mars/Olympus")))
                .is_err()
        );
    }

    #[test]
    fn spring_forward_gap_shifts_or_skips() {
        let zone = new_york();
        let local = epoch_ms_from_civil(2026, 3, 8, 2, 30, 0).expect("local");
        let shifted = zone
            .resolve_local(
                local,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("resolved");
        let skipped = zone
            .resolve_local(
                local,
                DstGapPolicy::SkipOccurrence,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("resolved");
        assert_eq!(skipped, LocalResolution::Skipped);
        let LocalResolution::GapShifted(instant) = shifted else {
            panic!("expected shifted gap resolution");
        };
        let civil = local_civil(&zone, instant).expect("civil");
        assert_eq!((civil.hour, civil.minute), (3, 0));
    }

    #[test]
    fn fall_back_fold_selects_first_or_second_occurrence() {
        let zone = new_york();
        let local = epoch_ms_from_civil(2026, 11, 1, 1, 30, 0).expect("local");
        let first = zone
            .resolve_local(
                local,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("first");
        let second = zone
            .resolve_local(
                local,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::SecondOccurrence,
            )
            .expect("second");
        let Some(first) = first.instant() else {
            panic!("first fold");
        };
        let Some(second) = second.instant() else {
            panic!("second fold");
        };
        assert!(first < second);
        let first_civil = local_civil(&zone, first).expect("civil");
        let second_civil = local_civil(&zone, second).expect("civil");
        assert_eq!((first_civil.hour, first_civil.minute), (1, 30));
        assert_eq!((second_civil.hour, second_civil.minute), (1, 30));
    }

    #[test]
    fn calendar_scan_handles_leap_days_and_weekdays() {
        let zone = ResolvedTimeZone::utc();
        let rule = CalendarRule::new(9, 0, 0).with_month(2).with_day(29);
        let next = next_calendar_occurrence(
            &rule,
            &zone,
            epoch_ms_from_civil(2026, 1, 1, 0, 0, 0).expect("after"),
            DstGapPolicy::ShiftForward,
            DstFoldPolicy::FirstOccurrence,
        )
        .expect("scan")
        .expect("leap day");
        assert_eq!(local_civil(&zone, next).expect("civil").year, 2028);

        let rule = CalendarRule::new(9, 0, 0).with_weekday(Weekday::Monday);
        let next = next_calendar_occurrence(
            &rule,
            &zone,
            epoch_ms_from_civil(2026, 11, 15, 0, 0, 0).expect("after"),
            DstGapPolicy::ShiftForward,
            DstFoldPolicy::FirstOccurrence,
        )
        .expect("scan")
        .expect("monday");
        assert_eq!(
            local_civil(&zone, next).expect("civil").weekday,
            Weekday::Monday
        );
    }

    #[test]
    fn calendar_scan_rolls_back_to_the_previous_local_year_at_the_utc_boundary() {
        let zone = new_york();
        let after = epoch_ms_from_civil(2027, 1, 1, 2, 0, 0).expect("after");
        let rule = CalendarRule::new(23, 0, 0).with_month(12).with_day(31);
        let next = next_calendar_occurrence(
            &rule,
            &zone,
            after,
            DstGapPolicy::ShiftForward,
            DstFoldPolicy::FirstOccurrence,
        )
        .expect("scan")
        .expect("previous local year occurrence");
        assert_eq!(next, epoch_ms_from_civil(2027, 1, 1, 4, 0, 0).expect("utc"));
        let civil = local_civil(&zone, next).expect("local civil");
        assert_eq!((civil.year, civil.month, civil.day), (2026, 12, 31));
        assert_eq!((civil.hour, civil.minute), (23, 0));
    }

    #[test]
    fn calendar_scan_finds_feb_29_weekday_beyond_the_old_eight_year_horizon() {
        let after = epoch_ms_from_civil(2072, 3, 1, 0, 0, 0).expect("after");
        let rule = CalendarRule::new(9, 0, 0)
            .with_month(2)
            .with_day(29)
            .with_weekday(Weekday::Monday);
        for zone in [ResolvedTimeZone::utc(), ResolvedTimeZone::fixed(-300)] {
            let next = next_calendar_occurrence(
                &rule,
                &zone,
                after,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("scan")
            .expect("next Monday leap day");
            let civil = local_civil(&zone, next).expect("civil");
            assert_eq!((civil.year, civil.month, civil.day), (2112, 2, 29));
            assert_eq!(civil.weekday, Weekday::Monday);
        }
    }

    #[test]
    fn calendar_scan_skips_uncovered_candidates_before_named_zone_resolution() {
        let resolver = BuiltinTimeZoneResolver::new();
        let zones = ["America/New_York", "Europe/Berlin"].map(|identifier| {
            resolver
                .resolve(&TimeZoneSpec::Named(String::from(identifier)))
                .expect("named zone")
        });
        let after_instants = [
            epoch_ms_from_civil(2024, 1, 1, 0, 30, 0).expect("2024-01-01T00:30:00Z"),
            epoch_ms_from_civil(2024, 6, 15, 12, 0, 0).expect("2024-06-15T12:00:00Z"),
        ];
        for zone in &zones {
            for after in after_instants {
                let next = next_calendar_occurrence(
                    &CalendarRule::new(9, 0, 0),
                    zone,
                    after,
                    DstGapPolicy::ShiftForward,
                    DstFoldPolicy::FirstOccurrence,
                )
                .expect("scan below the 2024 coverage start")
                .expect("next occurrence");
                assert!(next > after);
                let civil = local_civil(zone, next).expect("local civil");
                assert_eq!((civil.hour, civil.minute), (9, 0));
            }
        }
    }

    #[test]
    fn local_resolution_ignores_trial_offsets_outside_the_coverage_window() {
        let zone = BuiltinTimeZoneResolver::new()
            .resolve(&TimeZoneSpec::Named(String::from("Europe/Berlin")))
            .expect("Berlin");
        let local = epoch_ms_from_civil(2024, 1, 1, 1, 30, 0).expect("local");
        let resolved = zone
            .resolve_local(
                local,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect("covered candidate");
        assert_eq!(
            resolved,
            LocalResolution::Unique(epoch_ms_from_civil(2024, 1, 1, 0, 30, 0).expect("utc"))
        );

        // Both trial offsets fall before coverage and there is no DST gap, so
        // the resolver must report a coverage error rather than a generic
        // unresolvable-local-time error.
        let before_coverage = epoch_ms_from_civil(2024, 1, 1, 0, 0, 0).expect("local");
        let error = zone
            .resolve_local(
                before_coverage,
                DstGapPolicy::ShiftForward,
                DstFoldPolicy::FirstOccurrence,
            )
            .expect_err("outside coverage");
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
}
