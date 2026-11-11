//! Restart, clock-jump, timezone, permission/channel and retention reconciliation.

mod support;

use gpui_notification_schedule::*;
use std::collections::BTreeSet;
use support::*;

fn repeating(name: &str, policy: MissedFirePolicy) -> ScheduleSubmission {
    ScheduleSubmission::new(
        id(name),
        ScheduleTrigger::RepeatingInterval {
            interval_ms: MIN_REPEATING_INTERVAL_MS,
            max_occurrences: None,
        },
        content(name),
    )
    .with_missed_fire_policy(policy)
}

#[test]
fn startup_resubmits_missing_records_and_cancels_orphans() {
    let directory = TestDirectory::new("reconcile-startup");
    let fixture = Fixture::open(directory.path());
    submit_after_delay(&fixture, "a", 60_000);
    submit_after_delay(&fixture, "b", 60_000);
    assert_eq!(fixture.provider.pending_count(), 2);

    fixture.provider.simulate_os_lost_pending();
    fixture.provider.inject_orphan(
        id("orphan"),
        1,
        fixture.clock.wall_clock_epoch_ms() + 60_000,
    );
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.journal_records, 2);
    assert_eq!(report.active_records, 2);
    assert_eq!(report.resubmitted, 2);
    assert_eq!(report.cancelled_orphans, 1);
    assert_eq!(fixture.provider.pending_count(), 2);
    let pending: BTreeSet<_> = fixture
        .provider
        .pending_schedules()
        .into_iter()
        .map(|pending| pending.schedule_id)
        .collect();
    assert_eq!(pending, BTreeSet::from([id("a"), id("b")]));
}

#[test]
fn startup_replaces_stale_provider_epochs_with_the_durable_epoch() {
    let directory = TestDirectory::new("reconcile-stale-epoch");
    let fixture = Fixture::open(directory.path());
    let receipt = submit_after_delay(&fixture, "stale", 60_000);
    fixture
        .provider
        .force_pending_epoch(&id("stale"), receipt.schedule_epoch + 10);
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.cancelled_orphans, 1);
    assert_eq!(report.resubmitted, 1);
    let pending = fixture.provider.pending_schedules();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].schedule_epoch, receipt.schedule_epoch);
}

#[test]
fn multi_day_clock_jump_coalesces_into_one_catch_up_resubmission() {
    let directory = TestDirectory::new("reconcile-jump-latest");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            repeating("latest", MissedFirePolicy::FireOnceLatest),
        )
        .expect("submitted");
    assert_eq!(fixture.provider.request_count(), 1);

    fixture.clock.advance_wall_clock_ms(5 * 86_400_000);
    let now = fixture.clock.wall_clock_epoch_ms();
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("clock reconcile");
    assert_eq!(report.clock_jumps, 1);
    assert_eq!(report.catch_up_resubmissions, 1);
    assert_eq!(report.expired, 0);
    assert_eq!(fixture.provider.request_count(), 2);

    let record = fixture.service.schedule(&id("latest")).expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(record.catch_up_at_epoch_ms, Some(now));
    assert!(record.next_fire_at_epoch_ms.expect("next") > now);
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        now
    );
    assert_eq!(record.schedule_epoch, receipt.schedule_epoch);
}

#[test]
fn skip_to_next_drops_missed_occurrences_without_firing() {
    let directory = TestDirectory::new("reconcile-jump-skip");
    let fixture = Fixture::open(directory.path());
    fixture
        .service
        .submit(
            &OperationContext::default(),
            repeating("skip", MissedFirePolicy::SkipToNext),
        )
        .expect("submitted");
    fixture.clock.advance_wall_clock_ms(5 * 86_400_000);
    let now = fixture.clock.wall_clock_epoch_ms();
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("clock reconcile");
    assert_eq!(report.clock_jumps, 1);
    assert_eq!(report.catch_up_resubmissions, 0);
    assert_eq!(report.advanced, 1);
    let record = fixture.service.schedule(&id("skip")).expect("record");
    assert_eq!(record.catch_up_at_epoch_ms, None);
    assert!(record.next_fire_at_epoch_ms.expect("next") > now);
    assert_eq!(fixture.provider.request_count(), 2);
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        record.next_fire_at_epoch_ms.expect("next")
    );
}

#[test]
fn expire_after_grace_expires_a_stale_one_shot_without_a_storm() {
    let directory = TestDirectory::new("reconcile-grace-expired");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stale-one-shot"),
                ScheduleTrigger::OneShotAt {
                    fire_at_epoch_ms: fixture.clock.wall_clock_epoch_ms() + 60_000,
                },
                content("Stale"),
            )
            .with_missed_fire_policy(MissedFirePolicy::ExpireAfterGrace { grace_ms: 60_000 }),
        )
        .expect("submitted");
    fixture.clock.advance_wall_clock_ms(60_000 * 10);
    let subscription = fixture.service.subscribe();
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("clock reconcile");
    assert_eq!(report.expired, 1);
    assert_eq!(report.catch_up_resubmissions, 0);
    assert_eq!(fixture.provider.request_count(), 1);
    let record = fixture
        .service
        .schedule(&id("stale-one-shot"))
        .expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::Expired);
    assert_eq!(record.next_fire_at_epoch_ms, None);
    assert_eq!(record.schedule_epoch, receipt.schedule_epoch);
    assert_eq!(fixture.provider.pending_count(), 0);
    let events = drain_events(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        NotificationScheduleEvent::Expired { missed_occurrences, .. } if *missed_occurrences == 1
    )));
}

#[test]
fn expire_after_grace_fires_one_catch_up_within_the_grace_window() {
    let directory = TestDirectory::new("reconcile-grace-catch-up");
    let fixture = Fixture::open(directory.path());
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("grace"),
                ScheduleTrigger::OneShotAt {
                    fire_at_epoch_ms: fixture.clock.wall_clock_epoch_ms() + 60_000,
                },
                content("Grace"),
            )
            .with_missed_fire_policy(MissedFirePolicy::ExpireAfterGrace { grace_ms: 120_000 }),
        )
        .expect("submitted");
    fixture.clock.advance_wall_clock_ms(90_000);
    let now = fixture.clock.wall_clock_epoch_ms();
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("clock reconcile");
    assert_eq!(report.catch_up_resubmissions, 1);
    assert_eq!(report.expired, 0);
    let record = fixture.service.schedule(&id("grace")).expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(record.next_fire_at_epoch_ms, Some(now));
    assert_eq!(fixture.provider.request_count(), 2);
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        now
    );
}

#[test]
fn timezone_change_recomputes_a_followed_calendar_rule() {
    let directory = TestDirectory::new("reconcile-timezone");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 8, 0));
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("daily"),
                ScheduleTrigger::CalendarRule(CalendarRule::new(9, 0, 0)),
                content("Daily"),
            )
            .following_active_time_zone(true),
        )
        .expect("submitted");
    assert_eq!(
        receipt.next_fire_at_epoch_ms,
        Some(at_utc(2026, 1, 1, 9, 0))
    );

    fixture
        .clock
        .set_active_time_zone(TimeZoneSpec::Named(String::from("America/New_York")));
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("timezone reconcile");
    assert_eq!(report.timezone_updates, 1);
    assert_eq!(report.advanced, 1);
    let record = fixture.service.schedule(&id("daily")).expect("record");
    assert_eq!(
        record.time_zone,
        TimeZoneSpec::Named(String::from("America/New_York"))
    );
    assert_eq!(
        record.next_fire_at_epoch_ms,
        Some(at_utc(2026, 1, 1, 14, 0))
    );
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        at_utc(2026, 1, 1, 14, 0)
    );
}

#[test]
fn permission_and_channel_loss_suspends_and_restoration_resumes() {
    let directory = TestDirectory::new("reconcile-policy");
    let fixture = Fixture::open(directory.path());
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("channelled"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                channelled_content("Channelled", "reminders"),
            ),
        )
        .expect("submitted");
    fixture
        .provider
        .set_channel("reminders", ChannelAvailability::Disabled);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconcile");
    assert_eq!(report.suspended, 1);
    let record = fixture.service.schedule(&id("channelled")).expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::SuspendedByPolicy);
    assert_eq!(record.provider_fire_at_epoch_ms, None);
    assert!(matches!(
        record.suspension,
        Some(SuspensionReason::ChannelDisabled { .. })
    ));
    assert_eq!(fixture.provider.pending_count(), 0);

    fixture
        .provider
        .set_channel("reminders", ChannelAvailability::Enabled);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconcile");
    assert_eq!(report.resumed, 1);
    assert_eq!(
        fixture
            .service
            .schedule(&id("channelled"))
            .expect("record")
            .status,
        NotificationScheduleStatus::Scheduled
    );
    assert_eq!(fixture.provider.pending_count(), 1);

    fixture
        .permissions
        .set_notifications(PermissionStatus::Denied {
            can_request_again: false,
        });
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconcile");
    assert_eq!(report.suspended, 1);
    assert_eq!(fixture.provider.pending_count(), 0);
    assert!(matches!(
        fixture
            .service
            .schedule(&id("channelled"))
            .expect("record")
            .suspension,
        Some(SuspensionReason::NotificationsPermissionDenied)
    ));

    fixture
        .permissions
        .set_notifications(PermissionStatus::Granted);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconcile");
    assert_eq!(report.resumed, 1);
    assert_eq!(fixture.provider.pending_count(), 1);
}

#[test]
fn exact_alarm_loss_downgrades_or_suspends_active_schedules() {
    let directory = TestDirectory::new("reconcile-exact");
    let fixture = Fixture::open(directory.path());
    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    let downgrade = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("downgrade"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Downgrade"),
            )
            .with_precision(
                SchedulePrecision::ExactAlarm,
                ExactAlarmFallback::AllowInexact,
            ),
        )
        .expect("exact");
    assert_eq!(downgrade.precision, SchedulePrecision::ExactAlarm);
    let suspend = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("suspend"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Suspend"),
            )
            .with_precision(SchedulePrecision::ExactAlarm, ExactAlarmFallback::Reject),
        )
        .expect("exact reject");

    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Denied {
            can_request_again: true,
        });
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconcile");
    assert_eq!(report.downgraded, 1);
    assert_eq!(report.suspended, 1);
    let downgraded = fixture.service.schedule(&id("downgrade")).expect("record");
    assert_eq!(downgraded.precision, SchedulePrecision::InexactAdvisory);
    assert_eq!(
        downgraded.downgrade.expect("downgrade").reason,
        InexactDowngradeReason::ExactAlarmPermissionDenied
    );
    assert_eq!(downgraded.schedule_epoch, downgrade.schedule_epoch);
    let suspended = fixture.service.schedule(&id("suspend")).expect("record");
    assert_eq!(
        suspended.status,
        NotificationScheduleStatus::SuspendedByPolicy
    );
    assert_eq!(
        suspended.suspension,
        Some(SuspensionReason::ExactAlarmPermissionRevoked)
    );
    assert_eq!(suspended.schedule_epoch, suspend.schedule_epoch);
    assert_eq!(fixture.provider.pending_count(), 1);

    // Repeating the reconciliation while the permission is still revoked must
    // not downgrade/suspend twice or churn the provider.
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("idempotent reconcile");
    assert_eq!(report.downgraded, 0);
    assert_eq!(report.suspended, 0);

    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Granted);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("restored exact alarm");
    assert_eq!(report.resumed, 1);
    assert_eq!(fixture.provider.pending_count(), 2);
}

#[test]
fn exact_alarm_capability_loss_downgrades_or_suspends_active_schedules() {
    let directory = TestDirectory::new("reconcile-exact-capability");
    let fixture = Fixture::open(directory.path());
    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("downgrade-capability"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Downgrade capability"),
            )
            .with_precision(
                SchedulePrecision::ExactAlarm,
                ExactAlarmFallback::AllowInexact,
            ),
        )
        .expect("exact downgrade");
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("suspend-capability"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Suspend capability"),
            )
            .with_precision(SchedulePrecision::ExactAlarm, ExactAlarmFallback::Reject),
        )
        .expect("exact reject");
    assert_eq!(fixture.provider.pending_count(), 2);

    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Unsupported);
    let subscription = fixture.service.subscribe();
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("capability reconcile");
    assert_eq!(report.downgraded, 1);
    assert_eq!(report.suspended, 1);
    assert_eq!(report.provider_failures, 0);

    let downgraded = fixture
        .service
        .schedule(&id("downgrade-capability"))
        .expect("downgraded record");
    assert_eq!(downgraded.precision, SchedulePrecision::InexactAdvisory);
    assert_eq!(downgraded.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(
        downgraded.downgrade.expect("downgrade").reason,
        InexactDowngradeReason::ExactAlarmCapabilityUnavailable
    );
    let suspended = fixture
        .service
        .schedule(&id("suspend-capability"))
        .expect("suspended record");
    assert_eq!(
        suspended.status,
        NotificationScheduleStatus::SuspendedByPolicy
    );
    assert_eq!(
        suspended.suspension,
        Some(SuspensionReason::ProviderUnavailable)
    );
    assert_eq!(fixture.provider.pending_count(), 1);
    let events = drain_events(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        NotificationScheduleEvent::DowngradedToInexact {
            reason: InexactDowngradeReason::ExactAlarmCapabilityUnavailable,
            ..
        }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        NotificationScheduleEvent::Suspended {
            reason: SuspensionReason::ProviderUnavailable,
            ..
        }
    )));

    // A second pass with the same capability state is a no-op.
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("idempotent capability reconcile");
    assert_eq!(report.downgraded, 0);
    assert_eq!(report.suspended, 0);

    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("restored capability reconcile");
    assert_eq!(report.resumed, 1);
    assert_eq!(fixture.provider.pending_count(), 2);
}

#[test]
fn suspended_exact_alarm_is_not_resumed_while_capability_is_unavailable() {
    let directory = TestDirectory::new("reconcile-exact-resume-capability");
    let fixture = Fixture::open(directory.path());
    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("suspended"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Suspended"),
            )
            .with_precision(SchedulePrecision::ExactAlarm, ExactAlarmFallback::Reject),
        )
        .expect("exact reject");
    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Denied {
            can_request_again: true,
        });
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("permission reconcile");
    assert_eq!(report.suspended, 1);
    assert_eq!(
        fixture
            .service
            .schedule(&id("suspended"))
            .expect("suspended record")
            .suspension,
        Some(SuspensionReason::ExactAlarmPermissionRevoked)
    );

    // Permission is restored, but the provider cannot schedule exact alarms.
    fixture
        .permissions
        .set_exact_alarm(PermissionStatus::Granted);
    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Unsupported);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("capability reconcile");
    assert_eq!(report.resumed, 0);
    assert_eq!(report.suspended, 1);
    let record = fixture.service.schedule(&id("suspended")).expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::SuspendedByPolicy);
    assert_eq!(
        record.suspension,
        Some(SuspensionReason::ProviderUnavailable)
    );
    assert_eq!(fixture.provider.pending_count(), 0);

    fixture
        .provider
        .set_support(ScheduleOperation::ScheduleExactAlarm, Support::Native);
    let report = fixture
        .service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("restored capability reconcile");
    assert_eq!(report.resumed, 1);
    assert_eq!(
        fixture
            .service
            .schedule(&id("suspended"))
            .expect("resumed record")
            .status,
        NotificationScheduleStatus::Scheduled
    );
    assert_eq!(fixture.provider.pending_count(), 1);
}

#[test]
fn backward_clock_shift_recomputes_a_calendar_occurrence() {
    let directory = TestDirectory::new("reconcile-backward");
    let fixture = Fixture::open(directory.path());
    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 10, 0));
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("daily"),
                ScheduleTrigger::CalendarRule(CalendarRule::new(9, 0, 0)),
                content("Daily"),
            ),
        )
        .expect("submitted");
    assert_eq!(
        fixture
            .service
            .schedule(&id("daily"))
            .expect("record")
            .next_fire_at_epoch_ms,
        Some(at_utc(2026, 1, 2, 9, 0))
    );

    fixture
        .clock
        .set_wall_clock_epoch_ms(at_utc(2026, 1, 1, 8, 0));
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("backward reconcile");
    assert_eq!(report.timezone_updates, 0);
    assert_eq!(report.advanced, 1);
    let record = fixture.service.schedule(&id("daily")).expect("record");
    assert_eq!(record.next_fire_at_epoch_ms, Some(at_utc(2026, 1, 1, 9, 0)));
    assert_eq!(
        fixture
            .provider
            .latest_request()
            .expect("request")
            .fire_at_epoch_ms,
        at_utc(2026, 1, 1, 9, 0)
    );
}

#[test]
fn retention_prunes_terminal_records_by_budget_and_ttl_on_disk_and_memory() {
    let directory = TestDirectory::new("reconcile-retention");
    let retention = RetentionPolicy {
        max_terminal_records: 1,
        terminal_ttl_ms: 86_400_000,
        ..RetentionPolicy::default()
    };
    let fixture = Fixture::open_with_policy(directory.path(), retention);
    for name in ["one", "two", "three"] {
        submit_after_delay(&fixture, name, 60_000);
        fixture
            .service
            .cancel(&OperationContext::default(), &id(name))
            .expect("cancel");
    }
    let report = fixture
        .service
        .prune(&OperationContext::default())
        .expect("prune");
    assert_eq!(report.removed, 2);
    assert_eq!(report.remaining_terminal, 1);
    assert_eq!(fixture.service.schedule_count(), 1);
    let files = std::fs::read_dir(fixture.service.namespace_path().expect("namespace"))
        .expect("namespace directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("schedule-"))
        .count();
    assert_eq!(files, 1);

    fixture.clock.advance_wall_clock_ms(86_400_000 * 2);
    let report = fixture
        .service
        .prune(&OperationContext::default())
        .expect("prune ttl");
    assert_eq!(report.removed, 1);
    assert_eq!(fixture.service.schedule_count(), 0);
    let files = std::fs::read_dir(fixture.service.namespace_path().expect("namespace"))
        .expect("namespace directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("schedule-"))
        .count();
    assert_eq!(files, 0);
}

#[test]
fn a_multi_day_jump_over_sixty_four_schedules_never_storms_the_provider() {
    let directory = TestDirectory::new("reconcile-storm");
    let fixture = Fixture::open(directory.path());
    for index in 0..DEFAULT_MAX_ACTIVE_SCHEDULES {
        fixture
            .service
            .submit(
                &OperationContext::default(),
                repeating(&format!("storm-{index}"), MissedFirePolicy::FireOnceLatest),
            )
            .expect("submitted");
    }
    assert_eq!(
        fixture.provider.request_count(),
        DEFAULT_MAX_ACTIVE_SCHEDULES
    );
    fixture.clock.advance_wall_clock_ms(7 * 86_400_000);
    let now = fixture.clock.wall_clock_epoch_ms();
    let report = fixture
        .service
        .reconcile_clock_or_timezone_change(&OperationContext::default())
        .expect("clock reconcile");
    assert_eq!(report.catch_up_resubmissions, DEFAULT_MAX_ACTIVE_SCHEDULES);
    // Exactly one new provider registration per active schedule, never one per
    // missed minute.
    assert_eq!(
        fixture.provider.request_count(),
        DEFAULT_MAX_ACTIVE_SCHEDULES * 2
    );
    assert!(
        fixture
            .service
            .active_schedules()
            .iter()
            .all(|record| record.catch_up_at_epoch_ms == Some(now)
                && record.next_fire_at_epoch_ms.expect("next") > now)
    );
}
