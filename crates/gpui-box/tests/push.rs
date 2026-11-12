//! Facade contract tests for the optional `push` feature.

#![cfg(feature = "push")]

use gpui_box::push::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const APP_ID: &str = "gpui-box-push-facade";

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "gpui-box-push-{}-{}-{label}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&path).expect("test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Harness {
    _directory: TestDirectory,
    credentials: Arc<MemoryCredentials>,
    secrets: Secrets,
    provider: Arc<FakePushProvider>,
    service: PushService,
    clock: Arc<ManualPushClock>,
    presenter: Arc<RecordingPresenter>,
    activations: Arc<RecordingActivationSink>,
}

impl Harness {
    fn new(label: &str) -> Self {
        Self::with_permission(label, PermissionStatus::Granted)
    }

    fn with_permission(label: &str, permission: PermissionStatus) -> Self {
        let directory = TestDirectory::new(label);
        let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
        let credentials = Arc::new(MemoryCredentials::new());
        let secrets = Secrets::with_provider(APP_ID, credentials.clone()).expect("secrets");
        let provider = Arc::new(FakePushProvider::ready(
            PushProviderKind::Fcm,
            "fcm-primary",
        ));
        let clock = Arc::new(ManualPushClock::new(1_000));
        let presenter = Arc::new(RecordingPresenter::new());
        let activations = Arc::new(RecordingActivationSink::new());
        let runtime = PushRuntime::new(
            clock.clone(),
            presenter.clone(),
            Arc::new(FixedPermissionProbe::new(permission)),
            activations.clone(),
        );
        let config = PushConfig::new(
            PushPlatform::Linux,
            PushClientConfiguration::new("app.example").expect("client configuration"),
        );
        let service = PushService::open(
            &app,
            vec![provider.clone() as Arc<dyn PushProvider>],
            secrets.clone(),
            config,
            runtime,
        )
        .expect("push service");
        Self {
            _directory: directory,
            credentials,
            secrets,
            provider,
            service,
            clock,
            presenter,
            activations,
        }
    }

    fn register(&self) -> DeviceRegistration {
        completed(
            self.service
                .register_device(ExecutionOwner::Background, &OperationContext::default()),
        )
    }
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation failed") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn notification_summary() -> PushNotificationSummary {
    PushNotificationSummary::new("Order ready", "Your order is ready for pickup").expect("summary")
}

fn notification_frame(message_id: &str) -> IncomingPushFrame {
    IncomingPushFrame::new(
        PushMessageId::new(message_id).expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::NotificationAndData,
    )
    .with_notification(notification_summary())
    .with_payload(b"{\"order\":42}".to_vec())
}

#[test]
fn the_facade_exposes_the_coordinated_contract_surface() {
    assert_eq!(UPSTREAM_GPUI_REVISION.len(), 40);
    assert_eq!(PACKAGE_VERSION, "0.1.0-dev");
    assert_eq!(PushProviderKind::ALL.len(), 6);
    assert_eq!(PushPlatform::ALL.len(), 6);
    assert_eq!(PushOperation::ALL.len(), 6);
    assert_eq!(platform_support_matrix().len(), 36);
    assert_eq!(MAX_PUSH_PAYLOAD_BYTES, 4096);
    assert_eq!(PUSH_SECRET_SERVICE, "gpui-push/v1");
    assert_eq!(PUSH_SECRET_PURPOSES.len(), 4);
    assert!(
        platform_support_matrix()
            .iter()
            .all(|row| row.evidence == Evidence::ContractOnly)
    );
}

#[test]
fn registration_is_durable_and_never_persists_raw_material() {
    let harness = Harness::new("registration");
    let registration = harness.register();
    assert_eq!(registration.token_revision, 1);
    assert!(matches!(
        registration.sync,
        RegistrationSyncState::PendingServerSync { revision: 1 }
    ));
    assert_eq!(harness.provider.registration_count(), 1);
    let raw = fake_token("fcm-primary", 1);
    assert!(harness.credentials.values_contain(&raw));

    let state_file = harness.service.namespace_path().join("push-state.json");
    let state = std::fs::read_to_string(&state_file).expect("durable state");
    assert!(!state.contains("fake-device-token"));
    assert!(state.contains(registration.token_fingerprint.as_str()));

    let snapshot = harness.service.snapshot();
    assert!(snapshot.registration.registered);
    assert!(!format!("{snapshot:?}").contains("fake-device-token"));

    let acknowledged = completed(harness.service.acknowledge_server_sync(
        ExecutionOwner::Background,
        &OperationContext::default(),
        1,
    ));
    assert_eq!(
        acknowledged,
        RegistrationSyncState::SyncedAtRevision { revision: 1 }
    );
}

#[test]
fn registration_requires_a_background_owner_before_any_side_effect() {
    let harness = Harness::new("owner");
    let context = OperationContext::default();
    for owner in [ExecutionOwner::Ui, ExecutionOwner::Renderer] {
        let failure = harness
            .service
            .register_device(owner, &context)
            .expect_err("owner rejection");
        assert_eq!(failure.code, ErrorCode::InvalidInput);
    }
    assert_eq!(harness.provider.registration_count(), 0);
    assert!(harness.credentials.is_empty());
    assert_eq!(harness.service.registration().token_revision, 0);
}

#[test]
fn token_rotation_advances_the_revision_and_requires_a_new_sync() {
    let harness = Harness::new("rotation");
    let context = OperationContext::default();
    harness.register();
    let rotated = completed(harness.service.rotate_token(
        ExecutionOwner::Background,
        &context,
        TokenRotationTrigger::ProviderNotice,
    ));
    assert_eq!(rotated.token_revision, 2);
    assert!(matches!(
        rotated.sync,
        RegistrationSyncState::PendingServerSync { revision: 2 }
    ));
    assert_eq!(harness.provider.rotation_count(), 1);
    assert!(
        !harness
            .credentials
            .values_contain(b"fake-device-token/fcm-primary/1")
    );
    assert!(
        harness
            .credentials
            .values_contain(b"fake-device-token/fcm-primary/2")
    );

    let stale = harness
        .service
        .acknowledge_server_sync(ExecutionOwner::Background, &context, 1)
        .expect_err("stale acknowledgement");
    assert_eq!(stale.code, ErrorCode::ContextExpired);
    completed(
        harness
            .service
            .acknowledge_server_sync(ExecutionOwner::Background, &context, 2),
    );
}

#[test]
fn logout_is_retryable_and_never_deletes_sibling_service_secrets() {
    let harness = Harness::new("logout");
    let context = OperationContext::default();
    let account = PushAccountId::new("account-a").expect("account");
    harness.register();
    completed(
        harness
            .service
            .bind_account(ExecutionOwner::Background, &context, account.clone()),
    );

    let sibling = CredentialKey::new(account.as_str(), "gpui-auth/v1", "session")
        .expect("sibling credential key");
    let written = harness.secrets.put(
        &sibling,
        SecretBytes::new(b"sibling-session-token".to_vec()).expect("secret bytes"),
        SecretPolicy::default(),
        &context,
    );
    assert!(matches!(
        written,
        Ok(Completion::Completed(SecretAccess::Available(())))
    ));

    harness.credentials.set_locked(true);
    let failure = harness
        .service
        .logout_account(ExecutionOwner::Background, &context, &account)
        .expect_err("locked logout");
    assert_eq!(failure.code, ErrorCode::Unavailable);
    assert_eq!(failure.retry, RetryAdvice::AfterConditionsChange);
    let tombstone = harness
        .service
        .pending_secret_cleanup(&account)
        .expect("pending cleanup");
    assert_eq!(tombstone.state, TombstoneState::PendingSecretCleanup);
    assert_eq!(harness.service.registration().token_revision, 0);

    harness.credentials.unlock();
    let deleted = completed(harness.service.retry_pending_secret_cleanup(
        ExecutionOwner::Background,
        &context,
        &account,
    ));
    assert!(deleted >= 1);
    assert!(harness.service.pending_secret_cleanup(&account).is_none());
    assert!(!harness.credentials.values_contain(b"fake-device-token"));
    assert!(harness.credentials.values_contain(b"sibling-session-token"));
}

#[test]
fn duplicate_expired_and_oversize_frames_never_create_a_receipt() {
    let harness = Harness::new("dedup");
    let context = OperationContext::default();
    harness.register();
    harness.clock.advance(1);
    assert_eq!(harness.service.clock_now_ms(), 1_001);

    let first = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        notification_frame("m-1"),
    ));
    assert!(first.receipt.is_some());
    let receipt = first.receipt.expect("receipt");
    assert_eq!(receipt.payload_bytes, b"{\"order\":42}".len());
    assert!(receipt.has_background_work());

    let duplicate = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        notification_frame("m-1"),
    ));
    assert!(matches!(
        duplicate.handoff.outcome,
        ProviderHandoffOutcome::DuplicateSuppressed { .. }
    ));
    assert!(duplicate.receipt.is_none());

    let expired = IncomingPushFrame::new(
        PushMessageId::new("m-expired").expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::NotificationAndData,
    )
    .with_notification(notification_summary())
    .with_expiry(1_000);
    let expired = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        expired,
    ));
    assert!(matches!(
        expired.handoff.outcome,
        ProviderHandoffOutcome::ExpiredDrop
    ));

    let oversize = IncomingPushFrame::new(
        PushMessageId::new("m-oversize").expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::NotificationAndData,
    )
    .with_notification(notification_summary())
    .with_payload(vec![0u8; MAX_PUSH_PAYLOAD_BYTES + 1]);
    let oversize = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        oversize,
    ));
    assert!(matches!(
        oversize.handoff.outcome,
        ProviderHandoffOutcome::OversizePayload { .. }
    ));

    let counters = harness.service.counters();
    assert_eq!(counters.duplicate_suppressed, 1);
    assert_eq!(counters.expired_dropped, 1);
    assert_eq!(counters.oversize_rejected, 1);
    assert_eq!(harness.service.journal_len(), 1);
}

#[test]
fn permission_denial_suppresses_presentation_but_keeps_the_receipt() {
    let harness = Harness::with_permission(
        "denied",
        PermissionStatus::Denied {
            can_request_again: false,
        },
    );
    let context = OperationContext::default();
    harness.register();
    let outcome = completed(harness.service.deliver(
        ExecutionOwner::Background,
        &context,
        notification_frame("m-denied"),
    ));
    assert!(outcome.receipt.is_some());
    assert!(matches!(
        outcome.presentation,
        Some(LocalPresentationOutcome::SuppressedPermissionDenied {
            status: PermissionStatus::Denied { .. }
        })
    ));
    assert_eq!(harness.presenter.call_count(), 0);
    assert_eq!(harness.service.counters().presentations_suppressed, 1);
}

#[test]
fn granted_permission_presents_through_the_host_boundary() {
    let harness = Harness::new("presented");
    let context = OperationContext::default();
    harness.register();
    let outcome = completed(harness.service.deliver(
        ExecutionOwner::Background,
        &context,
        notification_frame("m-presented"),
    ));
    assert!(matches!(
        outcome.presentation,
        Some(LocalPresentationOutcome::Presented { .. })
    ));
    assert_eq!(harness.presenter.call_count(), 1);
}

#[test]
fn cold_launch_activation_is_queued_until_a_surface_attaches() {
    let harness = Harness::new("cold-launch");
    let context = OperationContext::default();
    harness.register();
    let request = PushActivationRequest::new(
        PushMessageId::new("m-cold").expect("message id"),
        AppDeliveryCondition::TerminatedColdLaunch,
    )
    .with_route("inbox/thread-42");
    let queued = completed(harness.service.activate_notification(
        ExecutionOwner::Background,
        &context,
        request,
    ));
    assert!(matches!(
        queued,
        PushActivationOutcome::QueuedForColdLaunch { depth: 1 }
    ));
    assert_eq!(harness.service.pending_cold_activations().len(), 1);
    assert_eq!(harness.activations.call_count(), 0);

    let routed = completed(
        harness
            .service
            .attach_ui_surface(ExecutionOwner::Background, &context),
    );
    assert_eq!(routed, 1);
    assert_eq!(harness.activations.call_count(), 1);
    let envelope = harness.activations.last().expect("activation envelope");
    assert_eq!(envelope.route, "inbox/thread-42");
    assert_eq!(envelope.registration_epoch, 1);
    assert!(harness.service.pending_cold_activations().is_empty());
}

#[test]
fn provider_outage_fails_closed_and_recovers_after_a_new_probe() {
    let harness = Harness::new("outage");
    let context = OperationContext::default();
    harness.register();
    harness.provider.simulate_outage();
    let frame = notification_frame("m-outage");
    let rejected = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame.clone(),
    ));
    assert!(matches!(
        rejected.handoff.outcome,
        ProviderHandoffOutcome::ProviderNotReady { .. }
    ));
    assert!(rejected.receipt.is_none());
    harness.provider.recover();
    let accepted = completed(harness.service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame,
    ));
    assert!(accepted.receipt.is_some());
}

#[test]
fn a_failed_state_write_restores_the_previous_registration_secrets() {
    let harness = Harness::new("state-write-rollback");
    let context = OperationContext::default();
    let first = harness.register();
    assert_eq!(first.token_revision, 1);
    let state_file = harness.service.namespace_path().join("push-state.json");
    std::fs::remove_file(&state_file).expect("remove state");
    std::fs::create_dir(&state_file).expect("block state writes");

    harness
        .service
        .rotate_token(
            ExecutionOwner::Background,
            &context,
            TokenRotationTrigger::ProviderNotice,
        )
        .expect_err("state write failure must fail the rotation");

    // The prior revision stays active and its material is restored instead of
    // being deleted by the failed rotation.
    assert_eq!(harness.service.registration().token_revision, 1);
    let lease = completed(
        harness
            .service
            .lease_registration_credentials(ExecutionOwner::Background, &context),
    );
    assert_eq!(lease.token_revision(), 1);
    let expected = fake_token("fcm-primary", 1);
    assert_eq!(lease.device_token(), Some(expected.as_slice()));
    assert!(
        !harness
            .credentials
            .values_contain(&fake_token("fcm-primary", 2))
    );
}

#[test]
fn a_failed_tombstone_write_does_not_claim_a_pending_cleanup() {
    let harness = Harness::new("tombstone-rollback");
    let context = OperationContext::default();
    harness.register();
    let first = PushAccountId::new("account-first").expect("account");
    completed(
        harness
            .service
            .bind_account(ExecutionOwner::Background, &context, first.clone()),
    );
    let state_file = harness.service.namespace_path().join("push-state.json");
    std::fs::remove_file(&state_file).expect("remove state");
    std::fs::create_dir(&state_file).expect("block state writes");

    harness.credentials.set_locked(true);
    let failure = harness
        .service
        .switch_account(
            ExecutionOwner::Background,
            &context,
            PushAccountId::new("account-second").expect("account"),
        )
        .expect_err("locked rekey");
    harness.credentials.unlock();
    assert_eq!(failure.code, ErrorCode::Unavailable);
    assert!(harness.service.pending_secret_cleanup(&first).is_none());
}

#[cfg(feature = "push-gpui")]
#[test]
fn the_gpui_adapter_registers_in_the_typed_services_registry() {
    use gpui_box::push::gpui_adapter::{PushModel, SurfacePushScope, install_push_in_services};

    let harness = Harness::new("adapter");
    let mut services = SystemServices::new();
    install_push_in_services(&mut services, harness.service.clone()).expect("register service");
    let registered = services
        .service::<PushService>()
        .expect("registered push service");
    assert!(harness.service.shares_state_with(&registered));
    let model_size = std::mem::size_of::<PushModel>();
    let scope_size = std::mem::size_of::<SurfacePushScope>();
    assert!(model_size > 0);
    assert!(scope_size > 0);
}
