//! Integer trigger validation, calendar resolution and DST policies.

mod support;

use gpui_notification_schedule::*;
use support::*;

fn calendar(
    rule: CalendarRule,
    zone: TimeZoneSpec,
    gap: DstGapPolicy,
    fold: DstFoldPolicy,
    name: &str,
    fixture: &Fixture,
) -> ScheduleSubmissionReceipt {
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(id(name), ScheduleTrigger::CalendarRule(rule), content(name))
                .with_time_zone(zone)
                .with_dst_policies(gap, fold),
        )
        .expect("calendar submitted")
}

#[test]
fn one_shot_and_delay_are_absolute_and_reject_the_past() {
    let directory = TestDirectory::new("trigger-one-shot");
    let fixture = Fixture::open(directory.path());
    let now = fixture.clock.wall_clock_epoch_ms();

    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("future"),
                ScheduleTrigger::OneShotAt {
                    fire_at_epoch_ms: now + 60_000,
                },
                content("Future"),
            ),
        )
        .expect("future one-shot");
    assert_eq!(receipt.next_fire_at_epoch_ms, Some(now + 60_000));
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        now + 60_000
    );

    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("past"),
                ScheduleTrigger::OneShotAt {
                    fire_at_epoch_ms: now - 1,
                },
                content("Past"),
            ),
        )
        .expect_err("past one-shot");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("zero-delay"),
                ScheduleTrigger::AfterDelay { delay_ms: 0 },
                content("Zero"),
            ),
        )
        .expect_err("zero delay");
    assert_eq!(error.code, ErrorCode::InvalidInput);

    let delayed = submit_after_delay(&fixture, "delayed", 30_000);
    assert_eq!(delayed.next_fire_at_epoch_ms, Some(now + 30_000));
}

#[test]
fn repeating_interval_floor_and_occurrence_budget_are_enforced() {
    let directory = TestDirectory::new("trigger-interval");
    let fixture = Fixture::open(directory.path());
    let now = fixture.clock.wall_clock_epoch_ms();

    for interval_ms in [0, 1, MIN_REPEATING_INTERVAL_MS - 1] {
        let error = fixture
            .service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("too-fast"),
                    ScheduleTrigger::RepeatingInterval {
                        interval_ms,
                        max_occurrences: None,
                    },
                    content("Fast"),
                ),
            )
            .expect_err("interval floor");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("repeat"),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: Some(2),
                },
                content("Repeat"),
            ),
        )
        .expect("repeating");
    assert_eq!(
        receipt.next_fire_at_epoch_ms,
        Some(now + MIN_REPEATING_INTERVAL_MS)
    );
    assert_eq!(
        fixture
            .service
            .schedule(&id("repeat"))
            .expect("record")
            .max_occurrences(),
        Some(2)
    );
}

#[test]
fn spring_forward_gap_shifts_to_the_transition_instant() {
    let directory = TestDirectory::new("trigger-gap");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 0, 0));
    let rule = CalendarRule::new(2, 30, 0)
        .with_year(2026)
        .with_month(3)
        .with_day(8);
    let receipt = calendar(
        rule,
        TimeZoneSpec::Named(String::from("America/New_York")),
        DstGapPolicy::ShiftForward,
        DstFoldPolicy::FirstOccurrence,
        "gap",
        &fixture,
    );
    // 02:30 does not exist on 2026-03-08 in New York; the transition instant
    // is 07:00 UTC, whose local time is 03:00 EDT.
    assert_eq!(
        receipt.next_fire_at_epoch_ms,
        Some(at_utc(2026, 3, 8, 7, 0))
    );
}

#[test]
fn spring_forward_gap_skip_moves_to_the_next_matching_date() {
    let directory = TestDirectory::new("trigger-gap-skip");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 0, 0));
    // March 8 is only a gap in 2026; in 2027 the same civil date exists.
    let rule = CalendarRule::new(2, 30, 0).with_month(3).with_day(8);
    let receipt = calendar(
        rule,
        TimeZoneSpec::Named(String::from("America/New_York")),
        DstGapPolicy::SkipOccurrence,
        DstFoldPolicy::FirstOccurrence,
        "gap-skip",
        &fixture,
    );
    let next = receipt.next_fire_at_epoch_ms.expect("next");
    let zone = BuiltinTimeZoneResolver::new()
        .resolve(&TimeZoneSpec::Named(String::from("America/New_York")))
        .expect("zone");
    let civil = local_civil(&zone, next).expect("local civil");
    assert_eq!((civil.year, civil.month, civil.day), (2027, 3, 8));
    assert_eq!((civil.hour, civil.minute), (2, 30));
}

#[test]
fn fall_back_fold_selects_first_or_second_occurrence() {
    let directory = TestDirectory::new("trigger-fold");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 0, 0));
    let zone = TimeZoneSpec::Named(String::from("America/New_York"));
    let rule = CalendarRule::new(1, 30, 0)
        .with_year(2026)
        .with_month(11)
        .with_day(1);
    let first = calendar(
        rule,
        zone.clone(),
        DstGapPolicy::ShiftForward,
        DstFoldPolicy::FirstOccurrence,
        "fold-first",
        &fixture,
    );
    let second = calendar(
        rule,
        zone,
        DstGapPolicy::ShiftForward,
        DstFoldPolicy::SecondOccurrence,
        "fold-second",
        &fixture,
    );
    // 01:30 EDT is 05:30 UTC; 01:30 EST is 06:30 UTC.
    assert_eq!(
        first.next_fire_at_epoch_ms,
        Some(at_utc(2026, 11, 1, 5, 30))
    );
    assert_eq!(
        second.next_fire_at_epoch_ms,
        Some(at_utc(2026, 11, 1, 6, 30))
    );
}

#[test]
fn impossible_calendar_dates_are_rejected_and_leap_day_is_honoured() {
    let directory = TestDirectory::new("trigger-calendar-validation");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 0, 0));

    for (month, day) in [(2, 30), (4, 31), (13, 1), (0, 1)] {
        let error = fixture
            .service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("impossible"),
                    ScheduleTrigger::CalendarRule(
                        CalendarRule::new(9, 0, 0).with_month(month).with_day(day),
                    ),
                    content("Impossible"),
                ),
            )
            .expect_err("impossible date");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    let receipt = calendar(
        CalendarRule::new(9, 0, 0).with_month(2).with_day(29),
        TimeZoneSpec::Utc,
        DstGapPolicy::ShiftForward,
        DstFoldPolicy::FirstOccurrence,
        "leap",
        &fixture,
    );
    let civil = civil_from_epoch_ms(receipt.next_fire_at_epoch_ms.expect("next"));
    assert_eq!((civil.year, civil.month, civil.day), (2028, 2, 29));

    // A specific non-leap year with February 29 has no future occurrence.
    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("bad-leap"),
                ScheduleTrigger::CalendarRule(
                    CalendarRule::new(9, 0, 0)
                        .with_year(2026)
                        .with_month(2)
                        .with_day(29),
                ),
                content("Bad leap"),
            ),
        )
        .expect_err("no occurrence");
    assert_eq!(error.code, ErrorCode::InvalidInput);
}

#[test]
fn occurrence_budget_completes_after_the_configured_presentations() {
    let directory = TestDirectory::new("trigger-budget");
    let fixture = Fixture::open(directory.path());
    let schedule_id = id("twice");
    let submitted = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                schedule_id.clone(),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: Some(2),
                },
                content("Twice"),
            ),
        )
        .expect("submitted");
    let first_fire = submitted.next_fire_at_epoch_ms.expect("first");
    let first = fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(schedule_id.clone(), submitted.schedule_epoch, first_fire),
        )
        .expect("first presentation");
    assert_eq!(first.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(first.presented_count, 1);
    let second_fire = first.next_fire_at_epoch_ms.expect("second");
    let second = fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(schedule_id.clone(), submitted.schedule_epoch, second_fire),
        )
        .expect("second presentation");
    assert_eq!(second.status, NotificationScheduleStatus::Completed);
    assert_eq!(second.presented_count, 2);
    assert_eq!(second.next_fire_at_epoch_ms, None);
    assert_eq!(fixture.provider.pending_count(), 0);
}

#[test]
fn weekday_calendar_rule_uses_the_local_civil_weekday() {
    let directory = TestDirectory::new("trigger-weekday");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 0, 0));
    let receipt = calendar(
        CalendarRule::new(9, 0, 0).with_weekday(Weekday::Monday),
        TimeZoneSpec::Named(String::from("America/New_York")),
        DstGapPolicy::ShiftForward,
        DstFoldPolicy::FirstOccurrence,
        "monday",
        &fixture,
    );
    let next = receipt.next_fire_at_epoch_ms.expect("next");
    let zone = BuiltinTimeZoneResolver::new()
        .resolve(&TimeZoneSpec::Named(String::from("America/New_York")))
        .expect("zone");
    let civil = local_civil(&zone, next).expect("local civil");
    assert_eq!(civil.weekday, Weekday::Monday);
    assert_eq!(civil.hour, 9);
    assert_eq!(civil.minute, 0);
}

#[test]
fn unknown_named_timezone_fails_closed() {
    let directory = TestDirectory::new("trigger-unknown-zone");
    let fixture = Fixture::open(directory.path());
    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("mars"),
                ScheduleTrigger::CalendarRule(CalendarRule::new(9, 0, 0)),
                content("Mars"),
            )
            .with_time_zone(TimeZoneSpec::Named(String::from("Mars/Olympus"))),
        )
        .expect_err("unknown zone");
    assert_eq!(error.code, ErrorCode::Unsupported);
}
