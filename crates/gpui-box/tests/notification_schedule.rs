//! Facade and cross-service integration tests for persistent local
//! notification scheduling.
//!
//! These tests are deterministic, use no sleeps, open real private application
//! storage in a temporary directory and verify that submission, presentation,
//! activation and reconciliation remain separate evidence stages.

#![cfg(feature = "notification-schedule")]

use gpui_box::notification_schedule::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gpui-box-notification-schedule-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn id(name: &str) -> ScheduledNotificationId {
    ScheduledNotificationId::new(ScheduleNamespace::application(), name).expect("schedule id")
}

fn content() -> NotificationContent {
    NotificationContent::new("Reminder", "Stand up").expect("content")
}

fn open(
    path: &Path,
    provider: Arc<FakeNotificationScheduleProvider>,
    clock: Arc<FakeScheduleClock>,
    permissions: Arc<ScheduleAuthorization>,
) -> (AppData, NotificationScheduleService) {
    let app = AppData::open(DirectoryRoots::under(path)).expect("private app data");
    let service = NotificationScheduleService::open(
        &app,
        provider,
        clock,
        Arc::new(BuiltinTimeZoneResolver::new()),
        permissions,
    )
    .expect("service");
    (app, service)
}

#[test]
fn notification_schedule_facade_exposes_the_coordinated_contracts() {
    assert_eq!(
        gpui_box::notification_schedule::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(
        gpui_box::notification_schedule::PACKAGE_VERSION,
        gpui_box::VERSION
    );
    assert_eq!(ScheduleOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(platform_support_matrix().len(), 36);
    assert_eq!(DEFAULT_NOTIFICATION_SCHEDULE_EVENT_CAPACITY, 64);
    assert_eq!(DEFAULT_MAX_ACTIVE_SCHEDULES, 64);
    assert_eq!(MIN_REPEATING_INTERVAL_MS, 60_000);
    assert_eq!(NOTIFICATION_SCHEDULE_FORMAT_VERSION, 1);
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    assert!(!durable_scheduling_available(PlatformTarget::Linux));
    assert!(
        platform_support_matrix()
            .iter()
            .all(|row| row.evidence == Evidence::ContractOnly)
    );
}

#[test]
fn durable_schedule_separates_submission_presentation_and_activation() {
    let directory = TestDirectory::new("evidence");
    let path = directory.path().to_path_buf();
    let (epoch, fire, token) = {
        let provider = Arc::new(FakeNotificationScheduleProvider::new());
        let clock = Arc::new(FakeScheduleClock::new(1_700_000_000_000));
        let permissions = Arc::new(ScheduleAuthorization::granting());
        let (app, service) = open(&path, provider.clone(), clock.clone(), permissions.clone());
        let receipt = service
            .submit(
                &OperationContext::default(),
                ScheduleSubmission::new(
                    id("facade"),
                    ScheduleTrigger::RepeatingInterval {
                        interval_ms: MIN_REPEATING_INTERVAL_MS,
                        max_occurrences: Some(3),
                    },
                    content(),
                )
                .with_conflict_policy(ScheduleConflictPolicy::Replace),
            )
            .expect("submitted");
        assert_eq!(receipt.disposition, SubmissionDisposition::Created);
        let record = service.schedule(&id("facade")).expect("record");
        assert_eq!(record.presented_count, 0);
        assert_eq!(record.activation_count, 0);

        let fire = receipt.next_fire_at_epoch_ms.expect("fire");
        let presented = service
            .record_delivery_observation(
                &OperationContext::default(),
                DeliveryObservation::new(id("facade"), receipt.schedule_epoch, fire),
            )
            .expect("presented");
        assert_eq!(presented.presented_count, 1);
        assert!(presented.next_fire_at_epoch_ms.expect("next") > fire);
        let activated = service
            .record_activation(
                &OperationContext::default(),
                ActivationObservation::tap(id("facade"), receipt.schedule_epoch, fire + 1),
            )
            .expect("activated");
        assert_eq!(activated.schedule_epoch, receipt.schedule_epoch);
        let record = service.schedule(&id("facade")).expect("record");
        assert_eq!(record.presented_count, 1);
        assert_eq!(record.activation_count, 1);
        assert_eq!(record.last_presented_at_epoch_ms, Some(fire));
        assert_eq!(record.last_activated_at_epoch_ms, Some(fire + 1));
        drop(app);
        (
            receipt.schedule_epoch,
            receipt.next_fire_at_epoch_ms,
            receipt.provider_token,
        )
    };

    let provider = Arc::new(FakeNotificationScheduleProvider::new());
    let clock = Arc::new(FakeScheduleClock::new(1_700_000_000_000));
    let permissions = Arc::new(ScheduleAuthorization::granting());
    let (app, service) = open(&path, provider.clone(), clock, permissions);
    let report = service
        .reconcile_startup(&OperationContext::default())
        .expect("restart reconciliation");
    assert_eq!(report.journal_records, 1);
    assert_eq!(report.resubmitted, 1);
    let record = service.schedule(&id("facade")).expect("record");
    assert_eq!(record.schedule_epoch, epoch);
    assert_eq!(
        record.next_fire_at_epoch_ms,
        Some(fire.expect("fire") + MIN_REPEATING_INTERVAL_MS)
    );
    assert_eq!(record.provider_token, token);
    drop(app);
}

#[test]
fn permission_and_channel_loss_suspends_and_readmits() {
    let directory = TestDirectory::new("policy");
    let provider = Arc::new(FakeNotificationScheduleProvider::new());
    let clock = Arc::new(FakeScheduleClock::new(1_700_000_000_000));
    let permissions = Arc::new(ScheduleAuthorization::granting());
    let (app, service) = open(
        directory.path(),
        provider.clone(),
        clock,
        permissions.clone(),
    );
    service
        .submit(
            &OperationContext::default(),
            ScheduleSubmission::new(
                id("channelled"),
                ScheduleTrigger::AfterDelay { delay_ms: 60_000 },
                NotificationContent::new("Reminder", "Stand up")
                    .expect("content")
                    .with_channel_id("reminders")
                    .expect("channel"),
            ),
        )
        .expect("submitted");
    provider.set_channel("reminders", ChannelAvailability::Muted);
    let report = service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconciliation");
    assert_eq!(report.suspended, 1);
    assert_eq!(provider.pending_count(), 0);
    assert_eq!(
        service.schedule(&id("channelled")).expect("record").status,
        NotificationScheduleStatus::SuspendedByPolicy
    );
    provider.set_channel("reminders", ChannelAvailability::Enabled);
    let report = service
        .reconcile_permissions_and_channels(&OperationContext::default())
        .expect("policy reconciliation");
    assert_eq!(report.resumed, 1);
    assert_eq!(provider.pending_count(), 1);
    drop(app);
}

#[cfg(feature = "notification-schedule-gpui")]
#[test]
fn gpui_adapter_registers_the_service_and_fences_a_surface_generation() {
    use gpui_box::notification_schedule::gpui_adapter;

    let service = NotificationScheduleService::in_memory(
        Arc::new(FakeNotificationScheduleProvider::new()),
        Arc::new(FakeScheduleClock::new(1_700_000_000_000)),
        Arc::new(BuiltinTimeZoneResolver::new()),
        Arc::new(ScheduleAuthorization::granting()),
    )
    .expect("service");
    let mut services = SystemServices::new();
    gpui_adapter::install_notification_schedule_in_services(&mut services, service.clone())
        .expect("register");
    assert!(
        services
            .service::<NotificationScheduleService>()
            .is_ok_and(|registered| service.shares_state_with(&registered))
    );
    let owner = UiOwner::new(SurfaceId(88));
    let scope = gpui_adapter::SurfaceNotificationScheduleScope::new(owner.clone(), service);
    assert_eq!(scope.surface(), SurfaceId(88));
    assert_eq!(scope.generation(), 1);
    assert!(scope.context().is_ok());
    drop(scope);
    assert!(owner.context(None).is_err());
    assert!(std::mem::size_of::<gpui_adapter::NotificationScheduleModel>() > 0);
}
