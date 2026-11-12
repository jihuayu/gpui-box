//! Public-API contract tests for `gpui-push`.

use gpui_push::*;
use std::sync::Arc;

fn secrets(app_id: &str, credentials: &Arc<MemoryCredentials>) -> Secrets {
    Secrets::with_provider(app_id, credentials.clone()).expect("secrets")
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation failed") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn frame(message_id: &str, account: Option<(&PushAccountId, u64)>) -> IncomingPushFrame {
    let mut frame = IncomingPushFrame::new(
        PushMessageId::new(message_id).expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::NotificationAndData,
    )
    .with_notification(PushNotificationSummary::new("Order ready", "Pick it up").expect("summary"))
    .with_payload(b"{\"order\":7}".to_vec());
    if let Some((account, epoch)) = account {
        frame = frame.with_target_account(account.clone(), epoch);
    }
    frame
}

fn service_with(
    app_id: &str,
    credentials: &Arc<MemoryCredentials>,
    provider: &Arc<FakePushProvider>,
    presenter: &Arc<RecordingPresenter>,
    activations: &Arc<RecordingActivationSink>,
    permission: PermissionStatus,
) -> PushService {
    PushService::in_memory(
        vec![provider.clone() as Arc<dyn PushProvider>],
        secrets(app_id, credentials),
        PushConfig::default(),
        PushRuntime::new(
            Arc::new(ManualPushClock::new(1_000)),
            presenter.clone(),
            Arc::new(FixedPermissionProbe::new(permission)),
            activations.clone(),
        ),
    )
    .expect("service")
}

#[test]
fn the_support_matrix_is_complete_and_contract_only() {
    let matrix = platform_support_matrix();
    assert_eq!(matrix.len(), 36);
    for platform in PushPlatform::ALL {
        let rows = platform_operations(platform);
        assert_eq!(rows.len(), 6);
        for row in rows {
            assert_eq!(row.evidence, Evidence::ContractOnly);
            assert!(row.is_supported());
            assert!(!row.required_host_configuration.is_empty());
            assert!(!row.notes.is_empty());
            assert!(row.has_link("#45"));
            assert!(row.has_link("#54"));
        }
    }
    assert!(
        operation_support(PushPlatform::Web, PushOperation::RoutePushActivation).has_link("#18")
    );
    assert!(
        operation_support(PushPlatform::Ios, PushOperation::DiscoverPushProviders).has_link("#9")
    );
    assert!(
        operation_support(PushPlatform::Android, PushOperation::DiscoverPushProviders)
            .has_link("#2")
    );
}

#[test]
fn ambiguous_or_empty_provider_selection_fails_closed() {
    let credentials = Arc::new(MemoryCredentials::new());
    let service = PushService::in_memory(
        vec![
            Arc::new(FakePushProvider::ready(PushProviderKind::Fcm, "fcm-a"))
                as Arc<dyn PushProvider>,
            Arc::new(FakePushProvider::ready(PushProviderKind::Apns, "apns-b"))
                as Arc<dyn PushProvider>,
        ],
        secrets("gpui-push-selection", &credentials),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("service");
    let context = OperationContext::default();
    let ambiguous = service
        .select_provider(ExecutionOwner::Background, &context)
        .expect_err("ambiguous selection");
    assert_eq!(ambiguous.code, ErrorCode::InvalidConfiguration);

    let empty = PushService::in_memory(
        Vec::new(),
        secrets("gpui-push-empty", &credentials),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("service");
    let missing = empty
        .select_provider(ExecutionOwner::Background, &context)
        .expect_err("no provider");
    assert_eq!(missing.code, ErrorCode::Unavailable);

    let unconfigured = PushService::in_memory(
        vec![
            Arc::new(UnavailablePushProvider::new(PushProviderKind::Wns)) as Arc<dyn PushProvider>,
        ],
        secrets("gpui-push-unconfigured", &credentials),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("service");
    let unconfigured = unconfigured
        .select_provider(ExecutionOwner::Background, &context)
        .expect_err("unconfigured provider");
    assert_eq!(unconfigured.code, ErrorCode::Unavailable);
}

#[test]
fn push_secrets_are_isolated_under_the_account_namespace() {
    let credentials = Arc::new(MemoryCredentials::new());
    let secrets = secrets("gpui-push-isolation", &credentials);
    let vault = PushSecretVault::new(secrets);
    let account = PushAccountId::new("account-iso").expect("account");
    let context = OperationContext::default();
    let material =
        RegistrationMaterial::new(Some(b"isolation-token".to_vec()), None).expect("material");
    let fingerprint = vault
        .store_registration(Some(&account), material, &context)
        .expect("store registration");
    assert_eq!(fingerprint, TokenFingerprint::of(b"isolation-token"));
    assert_eq!(credentials.value_count(), 1);
    assert_eq!(credentials.service_target_count(PUSH_SECRET_SERVICE), 1);

    let lease = vault
        .read_lease(Some(&account), 1, 1, &fingerprint, &context)
        .expect("read lease")
        .expect("lease present");
    assert_eq!(lease.device_token(), Some(b"isolation-token".as_slice()));
    assert_eq!(lease.registration_epoch(), 1);
    assert!(!format!("{lease:?}").contains("isolation-token"));
    let read_back = lease.into_material().expect("material");
    assert_eq!(
        read_back.device_token_bytes(),
        Some(b"isolation-token".as_slice())
    );

    assert_eq!(
        vault
            .delete_registration(Some(&account), &context)
            .expect("delete"),
        1
    );
    assert!(credentials.is_empty());

    credentials.set_locked(true);
    assert!(matches!(
        vault.delete_registration(None, &context),
        Err(SecretVaultError::Locked)
    ));
    assert_eq!(
        SecretVaultError::Locked.blocker(),
        Some(SecretCleanupBlocker::Locked)
    );
}

#[test]
fn the_message_journal_survives_a_restart() {
    let directory = tempfile::TempDir::new().expect("temp directory");
    let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let context = OperationContext::default();
    let config = || PushConfig::default();
    let first = PushService::open(
        &app,
        vec![provider.clone() as Arc<dyn PushProvider>],
        secrets("gpui-push-restart", &credentials),
        config(),
        PushRuntime::default(),
    )
    .expect("first service");
    completed(first.register_device(ExecutionOwner::Background, &context));
    let accepted = completed(first.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-restart", None),
    ));
    assert!(accepted.receipt.is_some());
    drop(first);

    let second = PushService::open(
        &app,
        vec![provider as Arc<dyn PushProvider>],
        secrets("gpui-push-restart", &credentials),
        config(),
        PushRuntime::default(),
    )
    .expect("second service");
    assert_eq!(second.registration().token_revision, 1);
    assert_eq!(second.journal_len(), 1);
    let duplicate = completed(second.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-restart", None),
    ));
    assert!(matches!(
        duplicate.handoff.outcome,
        ProviderHandoffOutcome::DuplicateSuppressed { .. }
    ));
    assert!(duplicate.receipt.is_none());
    assert_eq!(second.counters().duplicate_suppressed, 1);
}

#[test]
fn an_account_switch_fences_stale_epochs_and_keeps_sibling_secrets() {
    let credentials = Arc::new(MemoryCredentials::new());
    let secrets = secrets("gpui-push-switch", &credentials);
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let service = PushService::in_memory(
        vec![provider as Arc<dyn PushProvider>],
        secrets.clone(),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("service");
    let context = OperationContext::default();
    let first = PushAccountId::new("account-first").expect("account");
    let second = PushAccountId::new("account-second").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));

    let sibling = CredentialKey::new(first.as_str(), "gpui-auth/v1", "session")
        .expect("sibling credential key");
    assert!(matches!(
        secrets.put(
            &sibling,
            SecretBytes::new(b"sibling-token".to_vec()).expect("bytes"),
            SecretPolicy::default(),
            &context,
        ),
        Ok(Completion::Completed(SecretAccess::Available(())))
    ));

    let bound =
        completed(service.bind_account(ExecutionOwner::Background, &context, first.clone()));
    assert_eq!(bound.account.as_ref(), Some(&first));
    assert_eq!(bound.registration_epoch, 1);

    let switched =
        completed(service.switch_account(ExecutionOwner::Background, &context, second.clone()));
    assert_eq!(switched.account.as_ref(), Some(&second));
    assert_eq!(switched.registration_epoch, 2);
    assert!(switched.registered);

    let stale_account = completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-stale-account", Some((&first, 1))),
    ));
    assert!(matches!(
        stale_account.handoff.outcome,
        ProviderHandoffOutcome::AccountMismatchOrLoggedOut
    ));
    let stale_epoch = completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-stale-epoch", Some((&second, 1))),
    ));
    assert!(matches!(
        stale_epoch.handoff.outcome,
        ProviderHandoffOutcome::StaleRegistrationEpoch {
            expected: 2,
            received: 1
        }
    ));
    let current = completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-current", Some((&second, 2))),
    ));
    assert!(current.receipt.is_some());
    assert!(credentials.values_contain(b"sibling-token"));
    assert!(credentials.values_contain(b"fake-device-token/fcm-primary/1"));

    let locked = {
        credentials.set_locked(true);
        service
            .logout_account(ExecutionOwner::Background, &context, &second)
            .expect_err("locked logout")
    };
    assert_eq!(locked.code, ErrorCode::Unavailable);
    assert!(service.pending_secret_cleanup(&second).is_some());
    credentials.unlock();
    assert!(credentials.values_contain(b"sibling-token"));
    completed(service.retry_pending_secret_cleanup(ExecutionOwner::Background, &context, &second));
    assert!(service.pending_secret_cleanup(&second).is_none());
    assert!(!credentials.values_contain(b"fake-device-token"));
    assert!(credentials.values_contain(b"sibling-token"));
}

#[test]
fn an_invalidated_vault_key_blocks_logout_until_it_is_recovered() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-invalidated",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let account = PushAccountId::new("account-key").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, account.clone()));
    assert!(credentials.value_count() > 0);

    credentials.set_key_invalidated(true);
    let failure = service
        .logout_account(ExecutionOwner::Background, &context, &account)
        .expect_err("invalidated logout");
    assert_eq!(failure.code, ErrorCode::Unavailable);
    assert_eq!(failure.retry, RetryAdvice::AfterConditionsChange);
    let tombstone = service
        .pending_secret_cleanup(&account)
        .expect("pending cleanup");
    assert_eq!(tombstone.state, TombstoneState::PendingSecretCleanup);
    assert_eq!(service.registration().token_revision, 0);

    credentials.unlock();
    let deleted = completed(service.retry_pending_secret_cleanup(
        ExecutionOwner::Background,
        &context,
        &account,
    ));
    assert!(deleted >= 1);
    assert!(service.pending_secret_cleanup(&account).is_none());
    assert!(credentials.is_empty());
}

#[test]
fn silent_data_push_produces_a_bounded_background_handoff_without_presentation() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-silent",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    let silent = IncomingPushFrame::new(
        PushMessageId::new("m-silent").expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::SilentDataOnly,
    )
    .with_payload(b"{\"sync\":true}".to_vec());
    let outcome = completed(service.deliver(ExecutionOwner::Background, &context, silent));
    let receipt = outcome.receipt.expect("stage 2 receipt");
    let handoff = receipt.background_handoff.expect("background handoff");
    assert_eq!(handoff.class, BackgroundWorkClass::SilentData);
    assert_eq!(handoff.payload_bytes, b"{\"sync\":true}".len());
    assert_eq!(handoff.registration_epoch, 1);
    assert!(matches!(
        outcome.presentation,
        Some(LocalPresentationOutcome::NotApplicableSilentPush)
    ));
    assert_eq!(presenter.call_count(), 0);
    assert_eq!(service.pending_background_handoffs().len(), 1);
    assert_eq!(
        service
            .take_background_handoff()
            .map(|handoff| handoff.class),
        Some(BackgroundWorkClass::SilentData)
    );
    assert!(service.pending_background_handoffs().is_empty());
}

#[test]
fn not_determined_permission_suppresses_presentation_and_keeps_the_receipt() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-not-determined",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::NotDetermined,
    );
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    let outcome = completed(service.deliver(
        ExecutionOwner::Background,
        &context,
        frame("m-not-determined", None),
    ));
    assert!(outcome.receipt.is_some());
    assert!(matches!(
        outcome.presentation,
        Some(LocalPresentationOutcome::SuppressedPermissionDenied {
            status: PermissionStatus::NotDetermined
        })
    ));
    assert_eq!(presenter.call_count(), 0);
    assert_eq!(service.counters().presentations_suppressed, 1);
}

#[test]
fn a_user_tap_routes_once_and_rejects_duplicates_and_silent_messages() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-tap",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.deliver(ExecutionOwner::Background, &context, frame("m-tap", None)));
    let silent = IncomingPushFrame::new(
        PushMessageId::new("m-silent-tap").expect("message id"),
        PushProviderKind::Fcm,
        "fcm-primary",
        AppDeliveryCondition::Background,
        PushPayloadKind::SilentDataOnly,
    )
    .with_payload(b"{}".to_vec());
    completed(service.deliver(ExecutionOwner::Background, &context, silent));

    let request = PushActivationRequest::new(
        PushMessageId::new("m-tap").expect("message id"),
        AppDeliveryCondition::ForegroundActive,
    )
    .with_route("inbox/thread-7");
    let routed = completed(service.activate_notification(
        ExecutionOwner::Background,
        &context,
        request.clone(),
    ));
    assert_eq!(routed, PushActivationOutcome::Routed);
    assert_eq!(activations.call_count(), 1);
    assert_eq!(
        activations.last().map(|envelope| envelope.route),
        Some(String::from("inbox/thread-7"))
    );

    let duplicate =
        completed(service.activate_notification(ExecutionOwner::Background, &context, request));
    assert_eq!(
        duplicate,
        PushActivationOutcome::RejectedDuplicateActivation
    );
    assert_eq!(activations.call_count(), 1);

    let silent_request = PushActivationRequest::new(
        PushMessageId::new("m-silent-tap").expect("message id"),
        AppDeliveryCondition::Background,
    )
    .with_route("inbox/silent");
    let silent_outcome = completed(service.activate_notification(
        ExecutionOwner::Background,
        &context,
        silent_request,
    ));
    assert_eq!(silent_outcome, PushActivationOutcome::RejectedSilentMessage);
    assert_eq!(activations.call_count(), 1);
    assert_eq!(service.counters().activations_routed, 1);
}

#[test]
fn unregistering_deletes_only_push_secrets_and_revokes_the_token() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-unregister",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let account = PushAccountId::new("account-delete").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, account));
    assert!(credentials.value_count() > 0);

    let receipt = completed(service.unregister_device(ExecutionOwner::Background, &context));
    assert_eq!(receipt.provider_revocation, ProviderRevocation::Revoked);
    assert_eq!(receipt.secrets_deleted, 1);
    assert_eq!(provider.revocations().len(), 1);
    assert!(credentials.is_empty());
    assert_eq!(service.registration().token_revision, 0);
    assert!(!service.registration().registered);

    let again = service
        .unregister_device(ExecutionOwner::Background, &context)
        .expect_err("already unregistered");
    assert_eq!(again.code, ErrorCode::InvalidInput);
}

#[test]
fn a_corrupt_journal_is_quarantined_and_the_state_stays_intact() {
    let directory = tempfile::TempDir::new().expect("temp directory");
    let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let context = OperationContext::default();
    let first = PushService::open(
        &app,
        vec![provider.clone() as Arc<dyn PushProvider>],
        secrets("gpui-push-quarantine", &credentials),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("first service");
    completed(first.register_device(ExecutionOwner::Background, &context));
    completed(first.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-before-corruption", None),
    ));
    assert_eq!(first.journal_len(), 1);
    let journal = first.namespace_path().join("dedup-journal.json");
    drop(first);
    std::fs::write(&journal, b"{not a valid envelope").expect("corrupt journal");

    let second = PushService::open(
        &app,
        vec![provider as Arc<dyn PushProvider>],
        secrets("gpui-push-quarantine", &credentials),
        PushConfig::default(),
        PushRuntime::default(),
    )
    .expect("second service");
    assert_eq!(second.journal_len(), 0);
    assert_eq!(second.registration().token_revision, 1);
    let quarantined = std::fs::read_dir(second.namespace_path().join("quarantine"))
        .expect("quarantine directory")
        .count();
    assert!(quarantined >= 1);
    let accepted = completed(second.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-before-corruption", None),
    ));
    assert!(accepted.receipt.is_some());
}

#[test]
fn ui_attachment_and_cancellation_are_rejected_before_side_effects() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-owner-gate",
        &credentials,
        &provider,
        &presenter,
        &activations,
        PermissionStatus::Granted,
    );
    let owner = UiOwner::new(SurfaceId(7));
    let ui = owner.context(None).expect("ui context");
    let attached = OperationContext {
        ui: Some(ui),
        ..OperationContext::default()
    };
    let failure = service
        .register_device(ExecutionOwner::Background, &attached)
        .expect_err("ui attachment rejection");
    assert_eq!(failure.code, ErrorCode::InvalidInput);

    let cancelled = OperationContext::default();
    cancelled.cancellation.cancel(CancelReason::Caller);
    let outcome = service
        .register_device(ExecutionOwner::Background, &cancelled)
        .expect("cancelled result");
    assert_eq!(outcome, Completion::Cancelled(CancelReason::Caller));
    assert_eq!(provider.registration_count(), 0);
    assert!(credentials.is_empty());
    assert_eq!(service.registration().token_revision, 0);
}

#[test]
fn server_credentials_are_rejected_at_the_client_boundary() {
    assert!(reject_server_credentials(b"-----BEGIN PRIVATE KEY-----\nMIGT").is_err());
    assert!(reject_server_credentials(br#"{"type":"service_account","private_key":"x"}"#).is_err());
    assert!(reject_server_credentials(br#"{"client_secret":"value"}"#).is_err());
    assert!(reject_server_credentials(b"plain-client-material").is_ok());
    assert!(validate_vapid_public_key(&[0x11u8; 32]).is_err());
    assert!(validate_vapid_public_key_text("").is_err());

    let mut key = vec![0x04u8];
    key.extend(std::iter::repeat_n(0x11u8, 32));
    key.extend(std::iter::repeat_n(0x22u8, 32));
    assert_eq!(key.len(), VAPID_PUBLIC_KEY_BYTES);
    assert!(validate_vapid_public_key(&key).is_ok());

    let configuration = PushClientConfiguration::new("app.example").expect("configuration");
    assert!(
        configuration
            .clone()
            .with_client_material(b"-----BEGIN RSA PRIVATE KEY-----".to_vec())
            .validate()
            .is_err()
    );
    assert!(
        configuration
            .clone()
            .with_vapid_public_key(vec![0u8; 32])
            .validate()
            .is_err()
    );
    assert!(configuration.validate().is_ok());
}

// ---------------------------------------------------------------------------
// Round-1 review regressions
// ---------------------------------------------------------------------------

fn drain_events(subscription: &EventSubscription<PushEvent>) -> Vec<PushEvent> {
    let mut events = Vec::new();
    while let Ok(mut next) = subscription.try_next() {
        let waker = std::task::Waker::noop();
        let mut task = std::task::Context::from_waker(waker);
        match std::pin::Pin::new(&mut next).poll(&mut task) {
            std::task::Poll::Ready(Some(envelope)) => {
                if let gpui_system::Event::Value(event) = envelope.payload {
                    events.push(event);
                }
            }
            _ => break,
        }
    }
    events
}

fn cold_service(
    label: &str,
    credentials: &Arc<MemoryCredentials>,
    activations: &Arc<RecordingActivationSink>,
) -> PushService {
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    service_with(
        label,
        credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        activations,
        PermissionStatus::Granted,
    )
}

fn cold_request(id: &str) -> PushActivationRequest {
    PushActivationRequest::new(
        PushMessageId::new(id).expect("message id"),
        AppDeliveryCondition::TerminatedColdLaunch,
    )
    .with_route("inbox/thread-1")
}

#[test]
fn cold_launch_activations_are_re_fenced_after_an_account_switch() {
    let credentials = Arc::new(MemoryCredentials::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = cold_service("gpui-push-cold-switch", &credentials, &activations);
    let context = OperationContext::default();
    let first = PushAccountId::new("account-first").expect("account");
    let second = PushAccountId::new("account-second").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, first.clone()));
    assert_eq!(
        completed(service.activate_notification(
            ExecutionOwner::Background,
            &context,
            cold_request("m-cold-switch")
        )),
        PushActivationOutcome::QueuedForColdLaunch { depth: 1 }
    );

    // Switching away makes the queued activation belong to another account.
    completed(service.switch_account(ExecutionOwner::Background, &context, second));
    assert_eq!(service.registration().registration_epoch, 2);
    let subscription = service.subscribe();
    assert_eq!(
        completed(service.attach_ui_surface(ExecutionOwner::Background, &context)),
        0
    );
    assert_eq!(activations.call_count(), 0);
    assert!(service.pending_cold_activations().is_empty());
    let events = drain_events(&subscription);
    assert!(
        events.iter().any(|event| matches!(
            event,
            PushEvent::ActivationRejected {
                outcome: PushActivationOutcome::RejectedAccountMismatch,
                ..
            }
        )),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !matches!(event, PushEvent::ActivationRouted { .. }))
    );
}

#[test]
fn cold_launch_activations_are_fenced_by_the_registration_epoch() {
    let credentials = Arc::new(MemoryCredentials::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = cold_service("gpui-push-cold-epoch", &credentials, &activations);
    let context = OperationContext::default();
    let first = PushAccountId::new("account-first").expect("account");
    let second = PushAccountId::new("account-second").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, first.clone()));
    completed(service.activate_notification(
        ExecutionOwner::Background,
        &context,
        cold_request("m-cold-epoch"),
    ));
    // Switching away and back keeps the same account but advances the epoch.
    completed(service.switch_account(ExecutionOwner::Background, &context, second.clone()));
    completed(service.switch_account(ExecutionOwner::Background, &context, first));
    assert_eq!(service.registration().registration_epoch, 3);

    let subscription = service.subscribe();
    assert_eq!(
        completed(service.attach_ui_surface(ExecutionOwner::Background, &context)),
        0
    );
    assert_eq!(activations.call_count(), 0);
    let events = drain_events(&subscription);
    assert!(
        events.iter().any(|event| matches!(
            event,
            PushEvent::ActivationRejected {
                outcome: PushActivationOutcome::RejectedStaleEpoch {
                    expected: 3,
                    received: 1
                },
                ..
            }
        )),
        "{events:?}"
    );
}

#[test]
fn cold_launch_activations_route_once_when_attached() {
    let credentials = Arc::new(MemoryCredentials::new());
    let activations = Arc::new(RecordingActivationSink::new());
    let service = cold_service("gpui-push-cold-happy", &credentials, &activations);
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.activate_notification(
        ExecutionOwner::Background,
        &context,
        cold_request("m-cold-happy"),
    ));
    assert_eq!(service.pending_cold_activations().len(), 1);
    assert_eq!(
        completed(service.attach_ui_surface(ExecutionOwner::Background, &context)),
        1
    );
    assert_eq!(activations.call_count(), 1);
    assert_eq!(
        activations.last().map(|envelope| envelope.route),
        Some(String::from("inbox/thread-1"))
    );
    // A second drain has nothing left to route, even though the message is now
    // recorded as activated.
    assert_eq!(
        completed(service.attach_ui_surface(ExecutionOwner::Background, &context)),
        0
    );
    assert_eq!(activations.call_count(), 1);
    assert_eq!(service.counters().activations_routed, 1);
}

#[test]
fn cold_launch_activations_are_rejected_after_logout() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-cold-logout",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let account = PushAccountId::new("account-first").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, account.clone()));
    completed(
        service.activate_notification(
            ExecutionOwner::Background,
            &context,
            PushActivationRequest::new(
                PushMessageId::new("m-cold-logout").expect("message id"),
                AppDeliveryCondition::TerminatedColdLaunch,
            )
            .with_route("inbox/thread-2"),
        ),
    );
    completed(service.logout_account(ExecutionOwner::Background, &context, &account));
    assert_eq!(service.registration().token_revision, 0);

    let subscription = service.subscribe();
    let routed = completed(service.attach_ui_surface(ExecutionOwner::Background, &context));
    assert_eq!(routed, 0);
    assert_eq!(activations.call_count(), 0);
    let events = drain_events(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        PushEvent::ActivationRejected {
            outcome: PushActivationOutcome::RejectedNotRegistered,
            ..
        }
    )));
}

#[test]
fn duplicate_cold_launch_activations_are_rejected() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let service = service_with(
        "gpui-push-cold-duplicate",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &Arc::new(RecordingActivationSink::new()),
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    let request = PushActivationRequest::new(
        PushMessageId::new("m-cold-duplicate").expect("message id"),
        AppDeliveryCondition::TerminatedColdLaunch,
    )
    .with_route("inbox/thread-3");
    assert_eq!(
        completed(service.activate_notification(
            ExecutionOwner::Background,
            &context,
            request.clone()
        )),
        PushActivationOutcome::QueuedForColdLaunch { depth: 1 }
    );
    assert_eq!(
        completed(service.activate_notification(ExecutionOwner::Background, &context, request)),
        PushActivationOutcome::RejectedDuplicateActivation
    );
    assert_eq!(service.pending_cold_activations().len(), 1);
}

#[test]
fn delayed_presentation_after_an_account_switch_is_suppressed() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let presenter = Arc::new(RecordingPresenter::new());
    let service = service_with(
        "gpui-push-delayed-present",
        &credentials,
        &provider,
        &presenter,
        &Arc::new(RecordingActivationSink::new()),
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let first = PushAccountId::new("account-first").expect("account");
    let second = PushAccountId::new("account-second").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, first));
    completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-delayed", None),
    ));
    completed(service.switch_account(ExecutionOwner::Background, &context, second));

    let outcome = completed(service.present_message(
        ExecutionOwner::Background,
        &context,
        &PushMessageId::new("m-delayed").expect("message id"),
    ));
    assert_eq!(
        outcome,
        LocalPresentationOutcome::SuppressedRegistrationChanged
    );
    assert_eq!(presenter.call_count(), 0);
}

#[test]
fn stale_accepted_messages_cannot_route_after_a_switch() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-stale-route",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let first = PushAccountId::new("account-first").expect("account");
    let second = PushAccountId::new("account-second").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, first));
    completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-stale-route", None),
    ));
    completed(service.switch_account(ExecutionOwner::Background, &context, second));

    let outcome = completed(
        service.activate_notification(
            ExecutionOwner::Background,
            &context,
            PushActivationRequest::new(
                PushMessageId::new("m-stale-route").expect("message id"),
                AppDeliveryCondition::ForegroundActive,
            )
            .with_route("inbox/thread-9"),
        ),
    );
    assert_eq!(
        outcome,
        PushActivationOutcome::RejectedStaleEpoch {
            expected: 2,
            received: 1
        }
    );
    assert_eq!(activations.call_count(), 0);
}

#[test]
fn activation_handoff_failures_preserve_their_error_code() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let activations = Arc::new(RecordingActivationSink::new());
    let service = service_with(
        "gpui-push-handoff-code",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &activations,
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(
        service.activate_notification(
            ExecutionOwner::Background,
            &context,
            PushActivationRequest::new(
                PushMessageId::new("m-handoff-fail").expect("message id"),
                AppDeliveryCondition::TerminatedColdLaunch,
            )
            .with_route("inbox/thread-4"),
        ),
    );
    activations.fail_with(SystemError::new(
        ErrorCode::InvalidInput,
        "The host rejected the activation route",
    ));

    let subscription = service.subscribe();
    let routed = completed(service.attach_ui_surface(ExecutionOwner::Background, &context));
    assert_eq!(routed, 0);
    assert_eq!(activations.call_count(), 0);
    let events = drain_events(&subscription);
    assert!(events.iter().any(|event| matches!(
        event,
        PushEvent::ActivationRejected {
            outcome: PushActivationOutcome::HandoffFailed {
                code: ErrorCode::InvalidInput
            },
            ..
        }
    )));
}

#[test]
fn frames_from_an_unselected_provider_are_rejected() {
    let credentials = Arc::new(MemoryCredentials::new());
    let primary = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let secondary = Arc::new(FakePushProvider::ready(
        PushProviderKind::Apns,
        "apns-secondary",
    ));
    let config = PushConfig {
        selection: ProviderSelectionPolicy::HostSelected {
            provider_id: String::from("fcm-primary"),
            channel_id: None,
        },
        ..PushConfig::default()
    };
    let service = PushService::in_memory(
        vec![
            primary as Arc<dyn PushProvider>,
            secondary as Arc<dyn PushProvider>,
        ],
        secrets("gpui-push-unselected", &credentials),
        config,
        PushRuntime::default(),
    )
    .expect("service");
    let context = OperationContext::default();
    completed(service.register_device(ExecutionOwner::Background, &context));
    assert_eq!(
        service.registration().provider_id.as_deref(),
        Some("fcm-primary")
    );

    let mut other = IncomingPushFrame::new(
        PushMessageId::new("m-other-provider").expect("message id"),
        PushProviderKind::Apns,
        "apns-secondary",
        AppDeliveryCondition::Background,
        PushPayloadKind::NotificationAndData,
    )
    .with_notification(PushNotificationSummary::new("Title", "Body").expect("summary"))
    .with_payload(b"{}".to_vec());
    other = other.with_registration_epoch(1);
    let outcome = completed(service.accept_message(ExecutionOwner::Background, &context, other));
    assert_eq!(
        outcome.handoff.outcome,
        ProviderHandoffOutcome::UnknownProvider
    );
    assert_eq!(service.journal_len(), 0);
}

#[test]
fn frames_before_registration_are_rejected() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let service = service_with(
        "gpui-push-unregistered-frames",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &Arc::new(RecordingActivationSink::new()),
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let outcome = completed(service.accept_message(
        ExecutionOwner::Background,
        &context,
        frame("m-before-registration", None),
    ));
    assert_eq!(
        outcome.handoff.outcome,
        ProviderHandoffOutcome::DeviceNotRegistered
    );
    assert_eq!(service.counters().account_rejected, 1);
    assert_eq!(service.journal_len(), 0);
}

#[test]
fn background_mutations_are_serialized_against_logout() {
    let credentials = Arc::new(MemoryCredentials::new());
    let provider = Arc::new(FakePushProvider::ready(
        PushProviderKind::Fcm,
        "fcm-primary",
    ));
    let service = service_with(
        "gpui-push-mutation-gate",
        &credentials,
        &provider,
        &Arc::new(RecordingPresenter::new()),
        &Arc::new(RecordingActivationSink::new()),
        PermissionStatus::Granted,
    );
    let context = OperationContext::default();
    let account = PushAccountId::new("account-first").expect("account");
    completed(service.register_device(ExecutionOwner::Background, &context));
    completed(service.bind_account(ExecutionOwner::Background, &context, account.clone()));

    std::thread::scope(|scope| {
        let rotating = Arc::new(service.clone());
        let rotating_context = context.clone();
        scope.spawn(move || {
            for _ in 0..8 {
                let _ = rotating.rotate_token(
                    ExecutionOwner::Background,
                    &rotating_context,
                    TokenRotationTrigger::ProviderNotice,
                );
            }
        });
        let logging_out = Arc::new(service.clone());
        let logout_context = context.clone();
        scope.spawn(move || {
            let _ =
                logging_out.logout_account(ExecutionOwner::Background, &logout_context, &account);
        });
    });

    // A mutation that completed after the logout must never leave credentials
    // behind for a logged-out account.
    let registered = service.snapshot().registration.registered;
    assert_eq!(
        registered,
        credentials.values_contain(b"fake-device-token/fcm-primary/")
    );
    if registered {
        assert_eq!(
            service
                .registration()
                .account
                .as_ref()
                .map(PushAccountId::as_str),
            Some("account-first")
        );
    } else {
        assert_eq!(service.registration().token_revision, 0);
    }
}
