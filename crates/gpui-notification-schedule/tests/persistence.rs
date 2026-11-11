//! Durable envelope, semantic validation, quarantine and rollback behaviour.

mod support;

use gpui_notification_schedule::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use support::*;

/// Fails exactly the `fail_at`-th rename and, optionally, asks the fake
/// provider to reject the next schedule call at the same point. This makes the
/// post-provider commit failure and the rollback re-submission deterministic.
struct FailRenameAt {
    calls: AtomicU64,
    fail_at: u64,
    fail_provider: OnceLock<Arc<FakeNotificationScheduleProvider>>,
}

impl FailRenameAt {
    fn new(fail_at: u64) -> Self {
        Self {
            calls: AtomicU64::new(0),
            fail_at,
            fail_provider: OnceLock::new(),
        }
    }

    fn arm_provider(&self, provider: Arc<FakeNotificationScheduleProvider>) {
        let _ = self.fail_provider.set(provider);
    }
}

impl ScheduleStorageHook for FailRenameAt {
    fn before_rename(&self, _file: &str) -> io::Result<()> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_at {
            if let Some(provider) = self.fail_provider.get() {
                provider.fail_next_schedule();
            }
            return Err(io::Error::other("injected post-provider rename failure"));
        }
        Ok(())
    }
}

fn record_path(service: &NotificationScheduleService, name: &str) -> std::path::PathBuf {
    let directory = service.namespace_path().expect("durable namespace");
    directory.join(format!("schedule-{}.json", id(name).storage_id()))
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).expect("read record")).expect("record json")
}

fn write_json(path: &std::path::Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("encode")).expect("write record");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-notification-schedule-envelope\0");
    hasher.update(NOTIFICATION_SCHEDULE_FORMAT_VERSION.to_le_bytes());
    hasher.update(payload);
    hex(&hasher.finalize())
}

fn storage_id_for(namespace: &str, name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-notification-schedule-id\0");
    hasher.update(namespace.as_bytes());
    hasher.update([0]);
    hasher.update(name.as_bytes());
    hex(&hasher.finalize())
}

fn rewrite_payload(path: &std::path::Path, mutate: impl FnOnce(&mut Value)) {
    let mut value = read_json(path);
    mutate(&mut value["payload"]);
    let payload = serde_json::to_vec(&value["payload"]).expect("payload bytes");
    value["checksum"] = Value::String(digest(&payload));
    write_json(path, &value);
}

#[test]
fn durable_record_round_trips_across_restart_without_epoch_drift() {
    let directory = TestDirectory::new("persistence-roundtrip");
    let (epoch, token, next_fire) = {
        let fixture = Fixture::open(directory.path());
        let receipt = submit_after_delay(&fixture, "roundtrip", 60_000);
        assert!(
            record_path(&fixture.service, "roundtrip")
                .file_name()
                .expect("file")
                .to_str()
                .expect("name")
                .starts_with("schedule-")
        );
        assert_eq!(
            record_path(&fixture.service, "roundtrip")
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("json")
        );
        fixture.provider.simulate_restart();
        (
            receipt.schedule_epoch,
            receipt.provider_token,
            receipt.next_fire_at_epoch_ms,
        )
    };

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.journal_records, 1);
    assert_eq!(report.active_records, 1);
    // The restarted fake provider starts with no OS registrations, so the
    // journal record is re-submitted with the same durable epoch.
    assert_eq!(report.in_sync, 0);
    assert_eq!(report.resubmitted, 1);
    assert_eq!(report.quarantined, 0);
    let record = fixture.service.schedule(&id("roundtrip")).expect("record");
    assert_eq!(record.schedule_epoch, epoch);
    assert_eq!(record.provider_token, token);
    assert_eq!(record.next_fire_at_epoch_ms, next_fire);
    assert_eq!(record.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(fixture.provider.pending_count(), 1);
}

#[test]
fn checksum_and_version_mismatches_are_quarantined_without_aborting_healthy_records() {
    let directory = TestDirectory::new("persistence-quarantine");
    let fixture = Fixture::open(directory.path());
    submit_after_delay(&fixture, "healthy", 60_000);
    submit_after_delay(&fixture, "checksum", 60_000);
    submit_after_delay(&fixture, "version", 60_000);

    let checksum_path = record_path(&fixture.service, "checksum");
    let mut checksum = read_json(&checksum_path);
    checksum["checksum"] = Value::String("0".repeat(64));
    write_json(&checksum_path, &checksum);

    let version_path = record_path(&fixture.service, "version");
    let mut version = read_json(&version_path);
    version["schema_version"] = Value::from(2);
    write_json(&version_path, &version);

    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let subscription = fixture.service.subscribe();
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 2);
    assert_eq!(report.journal_records, 1);
    assert!(fixture.service.schedule(&id("healthy")).is_some());
    assert!(fixture.service.schedule(&id("checksum")).is_none());
    assert!(fixture.service.schedule(&id("version")).is_none());

    let quarantine = fixture
        .service
        .namespace_path()
        .expect("namespace")
        .join(QUARANTINE_DIR);
    let entries: Vec<_> = fs::read_dir(&quarantine)
        .expect("quarantine directory")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .all(|entry| entry.file_name().to_string_lossy().ends_with(".corrupt"))
    );

    let events = drain_events(&subscription);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                NotificationScheduleEvent::CorruptRecordQuarantined { .. }
            ))
            .count(),
        2
    );
}

#[test]
fn truncated_and_semantically_invalid_records_are_quarantined() {
    let directory = TestDirectory::new("persistence-semantic");
    let fixture = Fixture::open(directory.path());
    submit_after_delay(&fixture, "truncated", 60_000);
    submit_after_delay(&fixture, "semantic", 60_000);

    let truncated = record_path(&fixture.service, "truncated");
    fs::write(&truncated, b"{\"schema_version\"").expect("truncate");

    let semantic = record_path(&fixture.service, "semantic");
    let mut value = read_json(&semantic);
    value["payload"]["schedule_epoch"] = Value::from(0);
    let payload = serde_json::to_vec(&value["payload"]).expect("payload bytes");
    value["checksum"] = Value::String(digest(&payload));
    write_json(&semantic, &value);

    drop(fixture);
    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 2);
    assert_eq!(report.journal_records, 0);
    assert_eq!(fixture.service.schedule_count(), 0);
}

#[test]
fn write_and_rename_failures_do_not_mutate_memory_or_replace_prior_records() {
    let directory = TestDirectory::new("persistence-rollback");
    let faults = Arc::new(ScheduleStorageFaultInjection::new());
    let fixture = Fixture::open_with_hook(directory.path(), faults.clone());
    let original = submit_after_delay(&fixture, "stable", 60_000);
    assert_eq!(fixture.provider.request_count(), 1);

    faults.fail_next_rename();
    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stable"),
                ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                content("Replacement"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect_err("rename failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert_eq!(fixture.provider.request_count(), 1);
    let record = fixture.service.schedule(&id("stable")).expect("record");
    assert_eq!(record.schedule_epoch, original.schedule_epoch);
    assert_eq!(
        record.trigger,
        ScheduleTrigger::AfterDelay { delay_ms: 60_000 }
    );

    faults.fail_next_write();
    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("new"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("New"),
            ),
        )
        .expect_err("write failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert_eq!(fixture.provider.request_count(), 1);
    assert!(fixture.service.schedule(&id("new")).is_none());

    faults.clear();
    let replaced = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stable"),
                ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                content("Replacement"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect("retry");
    assert!(replaced.schedule_epoch > original.schedule_epoch);
    assert_eq!(fixture.provider.pending_count(), 1);
}

#[test]
fn provider_failure_rolls_back_the_durable_intent() {
    let directory = TestDirectory::new("persistence-provider-failure");
    let fixture = Fixture::open(directory.path());
    fixture.provider.fail_next_schedule_with(SystemError::new(
        ErrorCode::Unavailable,
        "simulated provider outage",
    ));
    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("rollback"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                content("Rollback"),
            ),
        )
        .expect_err("provider failure");
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(fixture.service.schedule(&id("rollback")).is_none());
    assert!(!record_path(&fixture.service, "rollback").exists());
    assert_eq!(fixture.provider.pending_count(), 0);
    assert_eq!(fixture.service.schedule_count(), 0);
}

#[test]
fn repeating_delivery_provider_failure_persists_submitted_and_startup_repairs() {
    let directory = TestDirectory::new("persistence-delivery-reschedule-failure");
    let fixture = Fixture::open(directory.path());
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("delivery-retry"),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: None,
                },
                content("Delivery retry"),
            ),
        )
        .expect("submitted");
    let first_fire = receipt.next_fire_at_epoch_ms.expect("first fire");
    fixture.provider.fail_next_schedule_with(SystemError::new(
        ErrorCode::Unavailable,
        "simulated reschedule outage",
    ));
    let observed = fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(id("delivery-retry"), receipt.schedule_epoch, first_fire),
        )
        .expect("presentation observation");
    assert!(!observed.provider_rescheduled);
    assert_eq!(observed.status, NotificationScheduleStatus::Submitted);
    let second_fire = observed.next_fire_at_epoch_ms.expect("second fire");
    let record = fixture
        .service
        .schedule(&id("delivery-retry"))
        .expect("record");
    assert_eq!(record.status, NotificationScheduleStatus::Submitted);
    assert_eq!(record.provider_token, None);
    assert_eq!(record.next_fire_at_epoch_ms, Some(second_fire));
    assert_eq!(record.provider_fire_at_epoch_ms, Some(second_fire));

    let durable = read_json(&record_path(&fixture.service, "delivery-retry"));
    assert_eq!(
        durable["payload"]["status"],
        Value::String("submitted".into())
    );
    assert_eq!(durable["payload"]["provider_token"], Value::Null);

    // The failed reschedule never removed the previous provider registration.
    let pending = fixture.provider.pending_schedules();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].schedule_epoch, receipt.schedule_epoch);
    assert_eq!(pending[0].fire_at_epoch_ms, first_fire);

    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("startup repair");
    assert_eq!(report.resubmitted, 1);
    assert_eq!(report.cancelled_orphans, 1);
    let repaired = fixture
        .service
        .schedule(&id("delivery-retry"))
        .expect("repaired record");
    assert_eq!(repaired.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(repaired.next_fire_at_epoch_ms, Some(second_fire));
    assert!(repaired.provider_token.is_some());
    let pending = fixture.provider.pending_schedules();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].schedule_epoch, receipt.schedule_epoch);
    assert_eq!(pending[0].fire_at_epoch_ms, second_fire);
    assert_eq!(
        pending[0].provider_token,
        repaired.provider_token.expect("token")
    );
}

#[test]
fn replace_rollback_resubmits_existing_after_post_provider_commit_failure() {
    let directory = TestDirectory::new("persistence-replace-resubmit");
    let hook = Arc::new(FailRenameAt::new(4));
    let fixture = Fixture::open_with_hook(directory.path(), hook.clone());
    let original = submit_after_delay(&fixture, "stable", 60_000);
    let original_fire = original.next_fire_at_epoch_ms.expect("original fire");
    assert_eq!(fixture.provider.request_count(), 1);

    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stable"),
                ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                content("Replacement"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect_err("post-provider commit failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);

    let restored = fixture.service.schedule(&id("stable")).expect("record");
    assert_eq!(restored.schedule_epoch, original.schedule_epoch);
    assert_eq!(restored.status, NotificationScheduleStatus::Scheduled);
    assert_eq!(restored.next_fire_at_epoch_ms, Some(original_fire));
    assert_eq!(
        restored.trigger,
        ScheduleTrigger::AfterDelay { delay_ms: 60_000 }
    );
    let pending = fixture.provider.pending_schedules();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].schedule_epoch, original.schedule_epoch);
    assert_eq!(pending[0].fire_at_epoch_ms, original_fire);
    assert_eq!(
        pending[0].provider_token,
        restored.provider_token.expect("token")
    );

    let durable = read_json(&record_path(&fixture.service, "stable"));
    assert_eq!(
        durable["payload"]["status"],
        Value::String("scheduled".into())
    );
    assert_eq!(
        durable["payload"]["provider_token"],
        Value::from(restored.provider_token.expect("token"))
    );
}

#[test]
fn replace_rollback_persists_submitted_when_existing_resubmission_fails() {
    let directory = TestDirectory::new("persistence-replace-resubmit-failure");
    let hook = Arc::new(FailRenameAt::new(4));
    let fixture = Fixture::open_with_hook(directory.path(), hook.clone());
    hook.arm_provider(fixture.provider.clone());
    let original = submit_after_delay(&fixture, "stable", 60_000);
    let original_fire = original.next_fire_at_epoch_ms.expect("original fire");

    let error = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("stable"),
                ScheduleTrigger::AfterDelay { delay_ms: 90_000 },
                content("Replacement"),
            )
            .with_conflict_policy(ScheduleConflictPolicy::Replace),
        )
        .expect_err("rollback resubmission failure");
    assert_eq!(error.code, ErrorCode::PlatformFailure);

    let failed = fixture.service.schedule(&id("stable")).expect("record");
    assert_eq!(failed.status, NotificationScheduleStatus::Submitted);
    assert_eq!(failed.provider_token, None);
    assert_eq!(failed.schedule_epoch, original.schedule_epoch);
    assert_eq!(failed.next_fire_at_epoch_ms, Some(original_fire));
    assert_eq!(fixture.provider.pending_count(), 0);
    let durable = read_json(&record_path(&fixture.service, "stable"));
    assert_eq!(
        durable["payload"]["status"],
        Value::String("submitted".into())
    );
    assert_eq!(durable["payload"]["provider_token"], Value::Null);

    // The durable record is honestly `Submitted`, so startup reconciliation
    // repairs the missing provider registration instead of silently diverging.
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("startup repair");
    assert_eq!(report.resubmitted, 1);
    let repaired = fixture.service.schedule(&id("stable")).expect("repaired");
    assert_eq!(repaired.status, NotificationScheduleStatus::Scheduled);
    assert!(repaired.provider_token.is_some());
    assert_eq!(fixture.provider.pending_count(), 1);
    assert_eq!(
        fixture.provider.pending_schedules()[0].fire_at_epoch_ms,
        original_fire
    );
}

#[test]
fn persisted_epochs_are_monotonic_across_restart_and_cancellation() {
    let directory = TestDirectory::new("persistence-epoch");
    let (max_epoch, cancelled_epoch) = {
        let fixture = Fixture::open(directory.path());
        let first = submit_after_delay(&fixture, "first", 60_000);
        let cancelled = fixture
            .service
            .cancel(&OperationContext::default(), &id("first"))
            .expect("cancel");
        let second = submit_after_delay(&fixture, "second", 60_000);
        assert!(second.schedule_epoch > first.schedule_epoch);
        assert!(cancelled.schedule_epoch > first.schedule_epoch);
        (second.schedule_epoch, cancelled.schedule_epoch)
    };

    let fixture = Fixture::open(directory.path());
    // Allocation is seeded from the durable watermark even before the first
    // explicit reconciliation, so a fresh process cannot reuse an old epoch.
    let third = submit_after_delay(&fixture, "third", 60_000);
    assert!(third.schedule_epoch > max_epoch);
    assert!(third.schedule_epoch > cancelled_epoch);
    fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("rehydrate");
    assert_eq!(
        fixture
            .service
            .schedule(&id("first"))
            .expect("cancelled record")
            .schedule_epoch,
        cancelled_epoch
    );
    assert_eq!(
        fixture
            .service
            .schedule(&id("third"))
            .expect("third record")
            .schedule_epoch,
        third.schedule_epoch
    );
}

#[test]
fn quarantined_bytes_are_preserved_and_live_file_can_be_recreated() {
    let directory = TestDirectory::new("persistence-bytes");
    let fixture = Fixture::open(directory.path());
    submit_after_delay(&fixture, "preserve", 60_000);
    let path = record_path(&fixture.service, "preserve");
    let tampered = b"{\"payload\":\"tampered\"}";
    fs::write(&path, tampered).expect("tamper");
    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 1);
    assert!(!path.exists());
    let quarantined = fs::read_dir(
        fixture
            .service
            .namespace_path()
            .expect("namespace")
            .join(QUARANTINE_DIR),
    )
    .expect("quarantine")
    .filter_map(Result::ok)
    .next()
    .expect("quarantined file");
    assert_eq!(fs::read(quarantined.path()).expect("bytes"), tampered);

    let recreated = submit_after_delay(&fixture, "preserve", 60_000);
    assert!(recreated.is_scheduled());
    assert!(path.exists());
}

#[test]
fn format_constants_and_namespace_are_stable() {
    assert_eq!(NOTIFICATION_SCHEDULE_FORMAT_VERSION, 1);
    assert_eq!(NOTIFICATION_SCHEDULE_NAMESPACE, "notification-schedule");
    assert_eq!(MAX_SCHEDULE_RECORD_BYTES, 256 * 1024);
    assert_eq!(QUARANTINE_DIR, "quarantine");
}
#[test]
fn crafted_invalid_schedule_name_is_quarantined_on_load() {
    let directory = TestDirectory::new("persistence-invalid-name");
    let fixture = Fixture::open(directory.path());
    submit_after_delay(&fixture, "invalid-name", 60_000);
    let path = record_path(&fixture.service, "invalid-name");
    rewrite_payload(&path, |payload| {
        payload["schedule_id"]["name"] = Value::String(String::from("../bad"));
    });
    let invalid = path.parent().expect("namespace").join(format!(
        "schedule-{}.json",
        storage_id_for("application", "../bad")
    ));
    fs::rename(&path, &invalid).expect("rename crafted record");
    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 1);
    assert_eq!(report.journal_records, 0);
    assert_eq!(fixture.service.schedule_count(), 0);
}

#[test]
fn non_last_invalid_delivery_observation_is_quarantined() {
    let directory = TestDirectory::new("persistence-delivery-all");
    let fixture = Fixture::open(directory.path());
    let schedule_id = id("delivery-all");
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                schedule_id.clone(),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: None,
                },
                content("Delivery all"),
            ),
        )
        .expect("submitted");
    let first_fire = receipt.next_fire_at_epoch_ms.expect("first");
    let first = fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(schedule_id.clone(), receipt.schedule_epoch, first_fire),
        )
        .expect("first presentation");
    let second_fire = first.next_fire_at_epoch_ms.expect("second");
    fixture
        .service
        .record_delivery_observation(
            &OperationContext::default(),
            DeliveryObservation::new(schedule_id.clone(), receipt.schedule_epoch, second_fire),
        )
        .expect("second presentation");

    let path = record_path(&fixture.service, "delivery-all");
    rewrite_payload(&path, |payload| {
        payload["delivery_observations"][0]["presented_at_epoch_ms"] = Value::from(1);
    });
    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 1);
    assert_eq!(report.journal_records, 0);
}

#[test]
fn non_last_invalid_activation_handoff_is_quarantined() {
    let directory = TestDirectory::new("persistence-activation-all");
    let fixture = Fixture::open(directory.path());
    let schedule_id = id("activation-all");
    let receipt = fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                schedule_id.clone(),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                actionable_content(),
            ),
        )
        .expect("submitted");
    let snooze = ActionId::new("snooze").expect("action");
    for offset in [1_000, 2_000] {
        fixture
            .service
            .record_activation(
                &OperationContext::default(),
                ActivationObservation::action(
                    schedule_id.clone(),
                    receipt.schedule_epoch,
                    snooze.clone(),
                    receipt.submitted_at_epoch_ms + offset,
                ),
            )
            .expect("activation");
    }

    let path = record_path(&fixture.service, "activation-all");
    rewrite_payload(&path, |payload| {
        payload["activation_handoffs"][0]["action_id"] = Value::String(String::from("undeclared"));
    });
    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 1);
    assert_eq!(report.journal_records, 0);
}

#[test]
fn active_interval_and_delay_records_require_an_anchor() {
    let directory = TestDirectory::new("persistence-anchor");
    let fixture = Fixture::open(directory.path());
    fixture
        .service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("interval-anchor"),
                ScheduleTrigger::RepeatingInterval {
                    interval_ms: MIN_REPEATING_INTERVAL_MS,
                    max_occurrences: None,
                },
                content("Interval anchor"),
            ),
        )
        .expect("interval");
    submit_after_delay(&fixture, "delay-anchor", 60_000);
    for name in ["interval-anchor", "delay-anchor"] {
        let path = record_path(&fixture.service, name);
        rewrite_payload(&path, |payload| {
            payload["anchor_fire_at_epoch_ms"] = Value::Null;
        });
    }
    drop(fixture);

    let fixture = Fixture::open(directory.path());
    let report = fixture
        .service
        .reconcile_startup(&OperationContext::default())
        .expect("reconcile");
    assert_eq!(report.quarantined, 2);
    assert_eq!(report.journal_records, 0);
}
