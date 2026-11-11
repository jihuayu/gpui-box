//! Behavioral contract tests for local verification, protected keys and unlock
//! policy. Everything runs headlessly against the deterministic fake provider.

use gpui_local_auth::{
    AccountId, AccountUnlockManager, AccountUnlockStatus, AppPhase, ChallengeBytes, Completion,
    Configuration, DiagnosticPhase, ErrorCode, Event, EventEnvelope, EventSubscription,
    FakeLocalAuthProvider, FakeVerificationMode, LocalAuthEvent, LocalAuthHostState,
    LocalAuthOperation, LocalAuthPromptCancelReason, LocalAuthReadiness, LocalAuthService,
    LocalAuthSnapshot, LocalUnlockFlowModel, LocalUnlockFlowState, LocalVerificationReceipt,
    LockReason, OperationContext, PendingLocalAuthPrompt, ProtectedKeyId, ProtectedSecretOutcome,
    ProtectedSecretRequest, ProtectedSignatureOutcome, RecoveryAction, Request, RetryAdvice,
    SecretBytes, SessionLockState, SignatureAlgorithm, SignatureRequest, Support, SurfaceId,
    SystemError, UiOwner, UnlockPolicy, VerificationOutcome, VerificationPolicy,
    VerificationPurpose, VerificationRequest, VerifiedFactor,
};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context as TaskContext, Poll, Wake, Waker},
    time::{Duration, Instant},
};

fn poll_ready<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut TaskContext::from_waker(Waker::noop()))
}

fn expect_completed<T>(mut request: Request<T>) -> T {
    match poll_ready(&mut request) {
        Poll::Ready(Ok(Completion::Completed(value))) => value,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("request was cancelled: {reason:?}")
        }
        Poll::Ready(Err(error)) => panic!("request failed: {error}"),
        Poll::Pending => panic!("request did not resolve inline"),
    }
}

fn expect_error<T>(mut request: Request<T>) -> SystemError {
    match poll_ready(&mut request) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(_)) => panic!("request unexpectedly succeeded"),
        Poll::Pending => panic!("request did not resolve inline"),
    }
}

fn purpose(text: &str) -> VerificationPurpose {
    VerificationPurpose::new(text).expect("valid purpose")
}

fn ui(owner: &UiOwner) -> OperationContext {
    OperationContext {
        ui: Some(owner.context(None).expect("current UI context")),
        ..Default::default()
    }
}

fn harness() -> (LocalAuthService, Rc<FakeLocalAuthProvider>, UiOwner) {
    let fake = Rc::new(FakeLocalAuthProvider::new());
    let service = LocalAuthService::new(fake.clone());
    let owner = UiOwner::new(SurfaceId(1));
    (service, fake, owner)
}

fn verified_receipt(
    service: &LocalAuthService,
    fake: &FakeLocalAuthProvider,
    owner: &UiOwner,
    policy: VerificationPolicy,
) -> LocalVerificationReceipt {
    fake.set_policy_fallback_required(false);
    fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
    match expect_completed(service.verify_user(
        VerificationRequest::new(policy, purpose("unlock")),
        ui(owner),
    )) {
        VerificationOutcome::Verified(receipt) => receipt,
        other => panic!("expected verification receipt, got {other:?}"),
    }
}

fn start_pending(
    service: &LocalAuthService,
    fake: &FakeLocalAuthProvider,
    owner: &UiOwner,
) -> (Request<VerificationOutcome>, PendingLocalAuthPrompt) {
    fake.set_auto_verify(None);
    let request = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(owner),
    );
    let pending = fake.take_pending_prompt().expect("pending fake prompt");
    (request, pending)
}

fn next_event(subscription: &EventSubscription<LocalAuthEvent>) -> LocalAuthEvent {
    let mut next = subscription.try_next().expect("single event reader");
    match poll_ready(&mut next) {
        Poll::Ready(Some(EventEnvelope {
            payload: Event::Value(event),
            ..
        })) => event,
        Poll::Ready(Some(EventEnvelope {
            payload: Event::ResyncRequired,
            ..
        })) => panic!("local-auth stream requested a resync"),
        Poll::Ready(None) => panic!("local-auth stream closed"),
        Poll::Pending => panic!("event was not ready"),
    }
}

fn assert_not_verified(resolved: &Poll<Result<Completion<VerificationOutcome>, SystemError>>) {
    assert!(!matches!(
        resolved,
        Poll::Ready(Ok(Completion::Completed(VerificationOutcome::Verified(_))))
    ));
}

#[test]
fn availability_queries_are_non_interactive() {
    let (service, fake, owner) = harness();
    let background = OperationContext::default();

    fake.set_available();
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::Available {
            biometric_enrolled: true,
            device_credential_configured: true,
        }
    );
    assert_eq!(snapshot.policy, VerificationPolicy::BiometricOnly);
    assert_eq!(fake.verify_requests().len(), 0);
    assert_eq!(fake.pending_prompt_count(), 0);

    fake.set_not_enrolled(true, Some(RecoveryAction::OpenSettings));
    let snapshot = expect_completed(service.query_availability(
        VerificationPolicy::BiometricOrDeviceCredential,
        background.clone(),
    ));
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::NotEnrolled {
            device_credential_configured: true,
            recovery: Some(RecoveryAction::OpenSettings),
        }
    );

    fake.set_locked_out(false);
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::LockedOut { permanent: false }
    );

    fake.set_locked_out(true);
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::LockedOut { permanent: true }
    );

    fake.set_hardware_missing();
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(snapshot.readiness, LocalAuthReadiness::HardwareMissing);

    fake.set_policy_restricted(true);
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::PolicyRestricted {
            device_credential_required: true,
        }
    );

    fake.set_unavailable();
    let snapshot = expect_completed(
        service.query_availability(VerificationPolicy::BiometricOnly, background.clone()),
    );
    assert_eq!(snapshot.readiness, LocalAuthReadiness::Unavailable);

    assert_eq!(fake.verify_requests().len(), 0);
    assert_eq!(fake.pending_prompt_count(), 0);
    assert!(owner.context(None).is_ok(), "query never needs a UI owner");

    // Verification remains a separate interactive operation.
    fake.set_available();
    fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert!(matches!(outcome, VerificationOutcome::Verified(_)));
    assert_eq!(fake.verify_requests().len(), 1);
}

#[test]
fn verification_outcomes_distinguish_factors_and_fallbacks() {
    let (service, fake, owner) = harness();
    fake.set_available();

    let receipt = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    assert_eq!(receipt.factor(), VerifiedFactor::Biometric);
    assert_eq!(receipt.policy(), VerificationPolicy::BiometricOnly);
    assert_eq!(receipt.surface(), SurfaceId(1));
    assert_eq!(receipt.generation(), 1);
    assert!(receipt.revision() >= 1);

    fake.set_policy_fallback_required(true);
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::PolicyFallbackRequired);

    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose("unlock"),
        ),
        ui(&owner),
    ));
    match outcome {
        VerificationOutcome::Verified(receipt) => {
            assert_eq!(receipt.factor(), VerifiedFactor::DeviceCredentialFallback);
        }
        other => panic!("expected device-credential fallback, got {other:?}"),
    }

    fake.set_policy_fallback_required(false);
    fake.set_auto_verify(Some(FakeVerificationMode::DeviceCredentialFallback));
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::PolicyFallbackRequired);

    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose("unlock"),
        ),
        ui(&owner),
    ));
    match outcome {
        VerificationOutcome::Verified(receipt) => {
            assert_eq!(receipt.factor(), VerifiedFactor::DeviceCredentialFallback);
        }
        other => panic!("expected device-credential fallback, got {other:?}"),
    }

    fake.set_auto_verify(Some(FakeVerificationMode::UserCancelled));
    let cancelled = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(cancelled, VerificationOutcome::UserCancelled);

    fake.set_not_enrolled(false, None);
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    match outcome {
        VerificationOutcome::NotEnrolled { recovery } => assert_eq!(recovery, None),
        other => panic!("expected not-enrolled outcome, got {other:?}"),
    }
}

#[test]
fn verification_reports_enrollment_lockout_and_hardware_states() {
    let (service, fake, owner) = harness();

    fake.set_not_enrolled(true, Some(RecoveryAction::OpenSettings));
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    match outcome {
        VerificationOutcome::NotEnrolled { recovery } => {
            assert_eq!(recovery, Some(RecoveryAction::OpenSettings));
        }
        other => panic!("expected not-enrolled outcome, got {other:?}"),
    }

    fake.set_locked_out(false);
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::LockedOut { permanent: false });

    fake.set_locked_out(true);
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::LockedOut { permanent: true });

    fake.set_hardware_missing();
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::HardwareMissing);

    fake.set_policy_restricted(true);
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(outcome, VerificationOutcome::PolicyFallbackRequired);

    fake.set_unavailable();
    let error = expect_error(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(service.active_prompt(), None);
}

#[test]
fn manual_prompt_handles_complete_or_cancel() {
    let (service, fake, owner) = harness();
    fake.set_available();
    fake.set_auto_verify(None);

    let mut request = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    );
    let pending = fake.take_pending_prompt().expect("pending prompt");
    assert!(poll_ready(&mut request).is_pending());
    assert!(pending.verify(VerifiedFactor::Biometric));
    match poll_ready(&mut request) {
        Poll::Ready(Ok(Completion::Completed(VerificationOutcome::Verified(receipt)))) => {
            assert_eq!(receipt.factor(), VerifiedFactor::Biometric);
        }
        other => panic!("manual verification failed: {other:?}"),
    }

    let mut cancelled = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    );
    let pending = fake.take_pending_prompt().expect("pending prompt");
    assert!(pending.cancel());
    match poll_ready(&mut cancelled) {
        Poll::Ready(Ok(Completion::Completed(VerificationOutcome::UserCancelled))) => {}
        other => panic!("manual cancellation failed: {other:?}"),
    }
    assert_eq!(service.active_prompt(), None);
}

#[test]
fn interactive_prompts_are_serialized() {
    let (service, fake, owner) = harness();
    fake.set_available();
    fake.set_auto_verify(None);

    let mut first = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    );
    let pending = fake.take_pending_prompt().expect("pending prompt");
    let error = expect_error(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(error.retry, RetryAdvice::AfterConditionsChange);
    assert!(poll_ready(&mut first).is_pending());

    assert!(pending.cancel());
    match poll_ready(&mut first) {
        Poll::Ready(Ok(Completion::Completed(VerificationOutcome::UserCancelled))) => {}
        other => panic!("first prompt did not resolve: {other:?}"),
    }
    assert_eq!(service.active_prompt(), None);

    let failed = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    );
    let pending = fake.take_pending_prompt().expect("pending prompt");
    let failure = SystemError::new(ErrorCode::PlatformFailure, "fake native failure");
    assert!(pending.fail(failure));
    assert_eq!(expect_error(failed).code, ErrorCode::PlatformFailure);
    assert_eq!(service.active_prompt(), None);

    fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
    let outcome = expect_completed(service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        ui(&owner),
    ));
    assert!(matches!(outcome, VerificationOutcome::Verified(_)));
}

#[test]
fn healthy_reconciliation_keeps_the_active_prompt() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let (mut request, pending) = start_pending(&service, &fake, &owner);
    let operation = LocalAuthOperation::VerifyUser(VerificationPolicy::BiometricOnly);

    assert_eq!(service.active_prompt(), Some(operation));
    assert!(
        !service.reconcile_host_state(
            &LocalAuthHostState::foreground(SurfaceId(1), 1),
            Instant::now()
        ),
        "a healthy foreground reconciliation must not invalidate the prompt"
    );
    assert_eq!(service.active_prompt(), Some(operation));
    assert!(pending.cancel());
    match poll_ready(&mut request) {
        Poll::Ready(Ok(Completion::Completed(VerificationOutcome::UserCancelled))) => {}
        other => panic!("healthy prompt did not cancel cleanly: {other:?}"),
    }
    assert_eq!(service.active_prompt(), None);
}

#[test]
fn disposed_owner_rejects_late_callback() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let events = service.subscribe();
    let (mut request, pending) = start_pending(&service, &fake, &owner);

    owner.dispose();
    assert!(service.reconcile_host_state(
        &LocalAuthHostState::foreground(SurfaceId(1), 1),
        Instant::now()
    ));
    assert_eq!(service.active_prompt(), None);
    let resolved = poll_ready(&mut request);
    assert_not_verified(&resolved);

    assert!(!pending.verify(VerifiedFactor::Biometric));
    assert_eq!(service.rejected_callbacks().len(), 1);
    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::PromptCancelled {
            reason: LocalAuthPromptCancelReason::OwnerChanged,
            ..
        }
    ));
    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::CallbackRejected(record)
            if record.phase == DiagnosticPhase::LateCallbackDiscarded
    ));
}

#[test]
fn recreated_generation_rejects_late_callback() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let events = service.subscribe();
    let (mut request, pending) = start_pending(&service, &fake, &owner);

    owner.advance_generation().expect("advance generation");
    assert!(service.reconcile_host_state(
        &LocalAuthHostState::foreground(SurfaceId(1), 2),
        Instant::now()
    ));
    assert_eq!(service.active_prompt(), None);
    let resolved = poll_ready(&mut request);
    assert_not_verified(&resolved);
    assert!(!pending.verify(VerifiedFactor::Biometric));

    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::PromptCancelled {
            reason: LocalAuthPromptCancelReason::GenerationAdvanced,
            ..
        }
    ));
    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::CallbackRejected(_)
    ));
}

#[test]
fn backgrounding_rejects_late_callback() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let events = service.subscribe();
    let (mut request, pending) = start_pending(&service, &fake, &owner);

    let background =
        LocalAuthHostState::foreground(SurfaceId(1), 1).with_phase(AppPhase::Background);
    assert!(service.reconcile_host_state(&background, Instant::now()));
    let resolved = poll_ready(&mut request);
    assert_not_verified(&resolved);
    assert!(!pending.verify(VerifiedFactor::Biometric));

    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::PromptCancelled {
            reason: LocalAuthPromptCancelReason::Backgrounded,
            ..
        }
    ));
    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::CallbackRejected(_)
    ));
}

#[test]
fn session_lock_rejects_late_callback() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let events = service.subscribe();
    let (mut request, pending) = start_pending(&service, &fake, &owner);

    let locked =
        LocalAuthHostState::foreground(SurfaceId(1), 1).with_session_lock(SessionLockState::Locked);
    assert!(service.reconcile_host_state(&locked, Instant::now()));
    let resolved = poll_ready(&mut request);
    assert_not_verified(&resolved);
    assert!(!pending.verify(VerifiedFactor::Biometric));

    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::PromptCancelled {
            reason: LocalAuthPromptCancelReason::SessionLocked,
            ..
        }
    ));
    assert!(matches!(
        next_event(&events),
        LocalAuthEvent::CallbackRejected(_)
    ));
}

#[test]
fn deadline_expiry_rejects_late_callback() {
    let (service, fake, owner) = harness();
    fake.set_available();
    fake.set_auto_verify(None);
    let deadline = Instant::now() + Duration::from_millis(100);
    let context = OperationContext {
        ui: Some(owner.context(None).expect("current UI context")),
        deadline: Some(deadline),
        ..Default::default()
    };
    let request = service.verify_user(
        VerificationRequest::new(VerificationPolicy::BiometricOnly, purpose("unlock")),
        context,
    );
    let pending = fake.take_pending_prompt().expect("pending prompt");
    std::thread::sleep(Duration::from_millis(140));

    let mut timed = std::pin::pin!(request.with_deadline_signal(std::future::ready(())));
    let resolved = timed
        .as_mut()
        .poll(&mut TaskContext::from_waker(Waker::noop()));
    match resolved {
        Poll::Ready(Err(error)) => assert_eq!(error.code, ErrorCode::TimedOut),
        other => panic!("deadline did not resolve: {other:?}"),
    }
    assert!(!pending.verify(VerifiedFactor::Biometric));
    assert_eq!(service.rejected_callbacks().len(), 1);
    assert_eq!(service.active_prompt(), None);
}

#[test]
fn receipts_are_single_use_and_account_scoped() {
    let (service, fake, owner) = harness();
    let alice = AccountId::new("alice").expect("account");
    let bob = AccountId::new("bob").expect("account");
    let mut manager = AccountUnlockManager::default();
    let receipt = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    let now = receipt.verified_at();

    let grant = manager
        .grant_unlock(&alice, &receipt, now)
        .expect("first grant");
    assert_eq!(grant.factor, VerifiedFactor::Biometric);
    assert!(matches!(
        manager.status(&alice, now),
        AccountUnlockStatus::Unlocked { .. }
    ));
    assert_eq!(
        manager.status(&bob, now),
        AccountUnlockStatus::Locked {
            reason: LockReason::NeverUnlocked
        }
    );
    assert!(manager.grant_unlock(&alice, &receipt, now).is_err());
    assert!(manager.receipt_consumed(&receipt));

    let second_owner = UiOwner::new(SurfaceId(2));
    let second = verified_receipt(
        &service,
        &fake,
        &second_owner,
        VerificationPolicy::BiometricOnly,
    );
    assert!(
        manager
            .grant_unlock(&alice, &second, second.verified_at())
            .is_ok()
    );
    assert_ne!(second.surface(), receipt.surface());
}

#[test]
fn receipt_freshness_and_fallback_policy_are_enforced() {
    let (service, fake, owner) = harness();
    let alice = AccountId::new("alice").expect("account");
    let receipt = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);

    let strict = UnlockPolicy {
        max_receipt_age: Duration::from_secs(1),
        ..Default::default()
    };
    let mut manager = AccountUnlockManager::new(strict).expect("policy");
    let too_late = receipt
        .verified_at()
        .checked_add(Duration::from_secs(2))
        .expect("instant");
    assert!(manager.grant_unlock(&alice, &receipt, too_late).is_err());
    let before = receipt
        .verified_at()
        .checked_sub(Duration::from_secs(1))
        .expect("instant");
    assert!(manager.grant_unlock(&alice, &receipt, before).is_err());

    fake.set_policy_fallback_required(true);
    fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
    let fallback = match expect_completed(service.verify_user(
        VerificationRequest::new(
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose("unlock"),
        ),
        ui(&owner),
    )) {
        VerificationOutcome::Verified(receipt) => receipt,
        other => panic!("expected fallback receipt, got {other:?}"),
    };
    assert_eq!(fallback.factor(), VerifiedFactor::DeviceCredentialFallback);
    let no_fallback = UnlockPolicy {
        allow_device_credential_fallback: false,
        ..Default::default()
    };
    let mut manager = AccountUnlockManager::new(no_fallback).expect("policy");
    let error = manager
        .grant_unlock(&alice, &fallback, Instant::now())
        .expect_err("fallback must be rejected");
    assert_eq!(error.code, ErrorCode::PermissionDenied);
}

#[test]
fn idle_timeout_extends_monotonically_and_caps_at_max_lifetime() {
    let (service, fake, owner) = harness();
    let alice = AccountId::new("alice").expect("account");
    let bob = AccountId::new("bob").expect("account");
    let policy = UnlockPolicy {
        idle_timeout: Duration::from_secs(10),
        max_lifetime: Duration::from_secs(30),
        max_receipt_age: Duration::from_secs(120),
        ..Default::default()
    };
    let mut manager = AccountUnlockManager::new(policy).expect("policy");
    let alice_receipt =
        verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    let t0 = alice_receipt.verified_at();
    let grant = manager
        .grant_unlock(&alice, &alice_receipt, t0)
        .expect("grant");
    assert_eq!(grant.expires_at, t0 + Duration::from_secs(10));

    match manager.touch_activity(&alice, t0 + Duration::from_secs(5)) {
        AccountUnlockStatus::Unlocked {
            expires_at,
            remaining,
            ..
        } => {
            assert_eq!(expires_at, t0 + Duration::from_secs(15));
            assert_eq!(remaining, Duration::from_secs(10));
        }
        other => panic!("activity did not extend unlock: {other:?}"),
    }
    match manager.touch_activity(&alice, t0 + Duration::from_secs(14)) {
        AccountUnlockStatus::Unlocked {
            expires_at,
            remaining,
            ..
        } => {
            assert_eq!(expires_at, t0 + Duration::from_secs(24));
            assert_eq!(remaining, Duration::from_secs(10));
        }
        other => panic!("second activity extension failed: {other:?}"),
    }
    match manager.touch_activity(&alice, t0 + Duration::from_secs(23)) {
        AccountUnlockStatus::Unlocked {
            expires_at,
            remaining,
            ..
        } => {
            assert_eq!(expires_at, t0 + Duration::from_secs(30));
            assert_eq!(remaining, Duration::from_secs(7));
        }
        other => panic!("activity cap failed: {other:?}"),
    }
    assert_eq!(
        manager.status(&alice, t0 + Duration::from_secs(31)),
        AccountUnlockStatus::Locked {
            reason: LockReason::MaxLifetime
        }
    );
    assert_eq!(
        manager.touch_activity(&alice, t0 + Duration::from_secs(32)),
        AccountUnlockStatus::Locked {
            reason: LockReason::MaxLifetime
        }
    );

    let second_owner = UiOwner::new(SurfaceId(2));
    let bob_receipt = verified_receipt(
        &service,
        &fake,
        &second_owner,
        VerificationPolicy::BiometricOnly,
    );
    let bob_t0 = bob_receipt.verified_at();
    manager
        .grant_unlock(&bob, &bob_receipt, bob_t0)
        .expect("bob grant");
    assert_eq!(
        manager.status(&bob, bob_t0 + Duration::from_secs(11)),
        AccountUnlockStatus::Locked {
            reason: LockReason::IdleTimeout
        }
    );
    assert_eq!(
        manager.touch_activity(&bob, bob_t0 + Duration::from_secs(12)),
        AccountUnlockStatus::Locked {
            reason: LockReason::IdleTimeout
        }
    );
}

#[test]
fn account_switching_and_resume_reconciliation_relock() {
    let (service, fake, owner) = harness();
    let alice = AccountId::new("alice").expect("account");
    let bob = AccountId::new("bob").expect("account");
    let mut manager = AccountUnlockManager::default();
    manager.set_active_account(Some(alice.clone()));
    let first = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    let first_at = first.verified_at();
    manager
        .grant_unlock(&alice, &first, first_at)
        .expect("alice grant");

    manager.set_active_account(Some(bob));
    assert_eq!(
        manager.status(&alice, first_at),
        AccountUnlockStatus::Locked {
            reason: LockReason::AccountSwitched
        }
    );

    manager.set_active_account(Some(alice.clone()));
    let second_owner = UiOwner::new(SurfaceId(2));
    let second = verified_receipt(
        &service,
        &fake,
        &second_owner,
        VerificationPolicy::BiometricOnly,
    );
    let second_at = second.verified_at();
    manager
        .grant_unlock(&alice, &second, second_at)
        .expect("alice grant");
    manager.reconcile_host_state(
        &LocalAuthHostState::foreground(SurfaceId(1), 1).with_phase(AppPhase::Background),
        second_at + Duration::from_secs(1),
    );
    assert_eq!(
        manager.status(&alice, second_at + Duration::from_secs(1)),
        AccountUnlockStatus::Locked {
            reason: LockReason::AppBackgrounded
        }
    );

    let third_owner = UiOwner::new(SurfaceId(3));
    let third = verified_receipt(
        &service,
        &fake,
        &third_owner,
        VerificationPolicy::BiometricOnly,
    );
    let third_at = third.verified_at();
    manager
        .grant_unlock(&alice, &third, third_at)
        .expect("alice grant");
    manager.reconcile_host_state(
        &LocalAuthHostState::foreground(SurfaceId(1), 1)
            .with_session_lock(SessionLockState::Locked),
        third_at + Duration::from_secs(1),
    );
    assert_eq!(
        manager.status(&alice, third_at + Duration::from_secs(1)),
        AccountUnlockStatus::Locked {
            reason: LockReason::SessionLocked
        }
    );
    manager.invalidate_enrollment();
    assert_eq!(
        manager.status(&alice, third_at + Duration::from_secs(2)),
        AccountUnlockStatus::Locked {
            reason: LockReason::Invalidated
        }
    );
}

#[test]
fn unlock_flow_model_couples_account_and_policy() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let alice = AccountId::new("alice").expect("account");
    let policy = UnlockPolicy::default();
    let purpose = purpose("unlock");
    let mut flow =
        LocalUnlockFlowModel::new(alice.clone(), policy, VerificationPolicy::BiometricOnly)
            .expect("flow");
    assert_eq!(flow.state(), LocalUnlockFlowState::Locked);
    assert_eq!(flow.account(), &alice);

    flow.begin_availability_check();
    assert_eq!(flow.state(), LocalUnlockFlowState::CheckingAvailability);
    let snapshot = LocalAuthSnapshot::new(
        LocalAuthReadiness::NotEnrolled {
            device_credential_configured: false,
            recovery: Some(RecoveryAction::OpenSettings),
        },
        VerificationPolicy::BiometricOnly,
        1,
    );
    flow.apply_availability(&snapshot);
    assert_eq!(flow.state(), LocalUnlockFlowState::RecoveryRequired);
    assert!(!flow.begin_prompt());

    flow.recover();
    flow.begin_availability_check();
    let snapshot = LocalAuthSnapshot::new(
        LocalAuthReadiness::Available {
            biometric_enrolled: true,
            device_credential_configured: true,
        },
        VerificationPolicy::BiometricOnly,
        2,
    );
    flow.apply_availability(&snapshot);
    assert!(flow.begin_prompt());
    assert!(flow.is_prompting());
    assert_eq!(
        flow.verification_request(purpose).policy,
        VerificationPolicy::BiometricOnly
    );

    let receipt = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    flow.apply_outcome(&VerificationOutcome::Verified(receipt));
    assert!(flow.is_unlocked());
    flow.lock();
    assert_eq!(flow.state(), LocalUnlockFlowState::Locked);
}

#[test]
fn protected_secrets_and_signatures_detect_enrollment_changes() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let alice = AccountId::new("alice").expect("account");
    let secret_key = ProtectedKeyId::new("vault-key").expect("key");
    let missing_key = ProtectedKeyId::new("missing-key").expect("key");
    fake.seal_protected_secret(
        &alice,
        &secret_key,
        SecretBytes::new(b"hunter2".to_vec()).expect("secret"),
    );

    match expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            secret_key.clone(),
            VerificationPolicy::BiometricOnly,
            purpose("unlock vault"),
        ),
        ui(&owner),
    )) {
        ProtectedSecretOutcome::Unlocked(secret) => {
            assert_eq!(secret.expose(), b"hunter2");
        }
        other => panic!("expected unlocked secret, got {other:?}"),
    }

    match expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            missing_key.clone(),
            VerificationPolicy::BiometricOnly,
            purpose("unlock vault"),
        ),
        ui(&owner),
    )) {
        ProtectedSecretOutcome::SecretNotFound => {}
        other => panic!("expected missing secret, got {other:?}"),
    }

    fake.advance_enrollment_epoch();
    match expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            secret_key.clone(),
            VerificationPolicy::BiometricOnly,
            purpose("unlock vault"),
        ),
        ui(&owner),
    )) {
        ProtectedSecretOutcome::KeyInvalidated => {}
        other => panic!("expected invalidated secret, got {other:?}"),
    }

    // Re-enrollment recovery: sealing under the new epoch succeeds again.
    fake.seal_protected_secret(
        &alice,
        &secret_key,
        SecretBytes::new(b"rotated".to_vec()).expect("secret"),
    );
    match expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            secret_key.clone(),
            VerificationPolicy::BiometricOnly,
            purpose("unlock vault"),
        ),
        ui(&owner),
    )) {
        ProtectedSecretOutcome::Unlocked(secret) => {
            assert_eq!(secret.expose(), b"rotated");
        }
        other => panic!("expected rotated secret, got {other:?}"),
    }

    let signing_key = ProtectedKeyId::new("signing-key").expect("key");
    fake.register_signing_key(&alice, &signing_key, SignatureAlgorithm::Ed25519);
    let first = match expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            signing_key.clone(),
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge-one".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOnly,
            purpose("sign in"),
        ),
        ui(&owner),
    )) {
        ProtectedSignatureOutcome::Signed(receipt) => {
            assert_eq!(receipt.algorithm, SignatureAlgorithm::Ed25519);
            assert_eq!(receipt.factor, VerifiedFactor::Biometric);
            assert!(!receipt.signature.is_empty());
            receipt
        }
        other => panic!("expected signature, got {other:?}"),
    };
    let second = match expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            signing_key.clone(),
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge-one".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOnly,
            purpose("sign in"),
        ),
        ui(&owner),
    )) {
        ProtectedSignatureOutcome::Signed(receipt) => receipt,
        other => panic!("expected signature, got {other:?}"),
    };
    assert_eq!(first.signature, second.signature);

    match expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            missing_key,
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOnly,
            purpose("sign in"),
        ),
        ui(&owner),
    )) {
        ProtectedSignatureOutcome::KeyNotFound => {}
        other => panic!("expected missing key, got {other:?}"),
    }

    fake.advance_enrollment_epoch();
    match expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice,
            signing_key,
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOnly,
            purpose("sign in"),
        ),
        ui(&owner),
    )) {
        ProtectedSignatureOutcome::KeyInvalidated => {}
        other => panic!("expected invalidated key, got {other:?}"),
    }
}

#[test]
fn policy_fallback_blocks_secret_release_and_signing() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let alice = AccountId::new("alice").expect("account");
    let key = ProtectedKeyId::new("vault-key").expect("key");
    fake.seal_protected_secret(
        &alice,
        &key,
        SecretBytes::new(b"hunter2".to_vec()).expect("secret"),
    );
    fake.register_signing_key(&alice, &key, SignatureAlgorithm::Ed25519);
    fake.set_policy_fallback_required(true);

    let outcome = expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            key.clone(),
            VerificationPolicy::BiometricOnly,
            purpose("unlock vault"),
        ),
        ui(&owner),
    ));
    assert!(matches!(
        outcome,
        ProtectedSecretOutcome::PolicyFallbackRequired
    ));

    let outcome = expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            key.clone(),
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOnly,
            purpose("sign in"),
        ),
        ui(&owner),
    ));
    assert_eq!(outcome, ProtectedSignatureOutcome::PolicyFallbackRequired);

    let outcome = expect_completed(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            key.clone(),
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose("unlock vault"),
        ),
        ui(&owner),
    ));
    match outcome {
        ProtectedSecretOutcome::Unlocked(secret) => assert_eq!(secret.expose(), b"hunter2"),
        other => panic!("fallback policy did not release the secret: {other:?}"),
    }

    let outcome = expect_completed(service.sign_challenge(
        SignatureRequest::new(
            alice,
            key,
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            VerificationPolicy::BiometricOrDeviceCredential,
            purpose("sign in"),
        ),
        ui(&owner),
    ));
    match outcome {
        ProtectedSignatureOutcome::Signed(receipt) => {
            assert_eq!(receipt.factor, VerifiedFactor::DeviceCredentialFallback);
        }
        other => panic!("fallback policy did not sign the challenge: {other:?}"),
    }
}

#[test]
fn capability_and_configuration_gate_operations() {
    let (service, fake, owner) = harness();
    fake.set_available();
    let alice = AccountId::new("alice").expect("account");
    let key = ProtectedKeyId::new("vault-key").expect("key");
    let biometric = VerificationPolicy::BiometricOnly;

    fake.set_operation_support(
        LocalAuthOperation::QueryAvailability(biometric),
        Support::Unsupported,
    );
    let error = expect_error(service.query_availability(biometric, OperationContext::default()));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(service.active_prompt(), None);
    assert_eq!(fake.pending_prompt_count(), 0);

    fake.clear_capability_overrides();
    fake.set_operation_configuration(
        LocalAuthOperation::QueryAvailability(biometric),
        Configuration::Required,
    );
    let error = expect_error(service.query_availability(biometric, OperationContext::default()));
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(service.active_prompt(), None);
    assert_eq!(fake.verify_requests().len(), 0);
    assert_eq!(fake.pending_prompt_count(), 0);

    fake.clear_capability_overrides();
    fake.set_operation_support(
        LocalAuthOperation::UnlockProtectedSecret,
        Support::Unsupported,
    );
    let error = expect_error(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            key.clone(),
            biometric,
            purpose("unlock vault"),
        ),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(fake.pending_secret_prompt_count(), 0);
    assert_eq!(service.active_prompt(), None);

    fake.clear_capability_overrides();
    fake.set_operation_configuration(
        LocalAuthOperation::UnlockProtectedSecret,
        Configuration::Required,
    );
    let error = expect_error(service.unlock_protected_secret(
        ProtectedSecretRequest::new(
            alice.clone(),
            key.clone(),
            biometric,
            purpose("unlock vault"),
        ),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(fake.pending_secret_prompt_count(), 0);

    fake.clear_capability_overrides();
    fake.set_operation_support(LocalAuthOperation::SignChallenge, Support::Unsupported);
    let error = expect_error(service.sign_challenge(
        SignatureRequest::new(
            alice.clone(),
            key.clone(),
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            biometric,
            purpose("sign in"),
        ),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(fake.pending_signature_prompt_count(), 0);
    assert_eq!(service.active_prompt(), None);

    fake.clear_capability_overrides();
    fake.set_operation_configuration(LocalAuthOperation::SignChallenge, Configuration::Required);
    let error = expect_error(service.sign_challenge(
        SignatureRequest::new(
            alice,
            key,
            SignatureAlgorithm::Ed25519,
            ChallengeBytes::new(b"challenge".to_vec()).expect("challenge"),
            biometric,
            purpose("sign in"),
        ),
        ui(&owner),
    ));
    assert_eq!(error.code, ErrorCode::InvalidConfiguration);
    assert_eq!(error.retry, RetryAdvice::Never);
    assert_eq!(fake.pending_signature_prompt_count(), 0);
    assert_eq!(service.active_prompt(), None);
}

#[test]
fn snapshots_are_tracked_per_verification_policy() {
    let (service, fake, _owner) = harness();
    fake.set_available();
    let events = service.subscribe();
    let biometric = VerificationPolicy::BiometricOnly;
    let fallback = VerificationPolicy::BiometricOrDeviceCredential;

    let first =
        expect_completed(service.query_availability(biometric, OperationContext::default()));
    assert_eq!(first.policy, biometric);
    assert_eq!(service.snapshot_for_policy(biometric), Some(first));
    assert_eq!(service.snapshot_for_policy(fallback), None);
    assert_eq!(service.snapshot(), Some(first));
    match next_event(&events) {
        LocalAuthEvent::AvailabilityChanged { snapshot } => assert_eq!(snapshot, first),
        other => panic!("expected first availability event, got {other:?}"),
    }

    let second =
        expect_completed(service.query_availability(fallback, OperationContext::default()));
    assert_eq!(second.policy, fallback);
    assert_eq!(service.snapshot_for_policy(fallback), Some(second));
    assert_eq!(service.snapshot(), Some(second));
    match next_event(&events) {
        LocalAuthEvent::AvailabilityChanged { snapshot } => assert_eq!(snapshot, second),
        other => panic!("expected fallback availability event, got {other:?}"),
    }

    let repeated =
        expect_completed(service.query_availability(biometric, OperationContext::default()));
    assert_eq!(repeated, first);
    let mut none = events.try_next().expect("single event reader");
    assert!(poll_ready(&mut none).is_pending());
    drop(none);

    fake.set_not_enrolled(true, None);
    let changed =
        expect_completed(service.query_availability(biometric, OperationContext::default()));
    assert_eq!(
        changed.readiness,
        LocalAuthReadiness::NotEnrolled {
            device_credential_configured: true,
            recovery: None,
        }
    );
    assert_eq!(service.snapshot_for_policy(biometric), Some(changed));
    assert_eq!(
        service.snapshot_for_policy(fallback),
        Some(second),
        "another policy's snapshot must stay independent"
    );
    match next_event(&events) {
        LocalAuthEvent::AvailabilityChanged { snapshot } => assert_eq!(snapshot, changed),
        other => panic!("expected changed availability event, got {other:?}"),
    }
}

#[test]
fn identifiers_purposes_and_secret_outcomes_are_redacted() {
    let account = AccountId::new("alice@example.com").expect("account");
    let key = ProtectedKeyId::new("top-secret-key").expect("key");
    let purpose = VerificationPurpose::new("  unlock the vault  ").expect("purpose");
    assert_eq!(format!("{account:?}"), "AccountId([REDACTED])");
    assert_eq!(format!("{key:?}"), "ProtectedKeyId([REDACTED])");
    assert_eq!(format!("{purpose:?}"), "VerificationPurpose([REDACTED])");
    assert_eq!(purpose.as_str(), "unlock the vault");
    assert!(!format!("{account:?}").contains("alice"));
    assert!(!format!("{key:?}").contains("top-secret"));
    assert!(!format!("{purpose:?}").contains("vault"));

    let secret = SecretBytes::new(b"super-secret-value".to_vec()).expect("secret");
    let debug = format!("{:?}", ProtectedSecretOutcome::Unlocked(secret));
    assert_eq!(debug, "Unlocked([REDACTED])");
    assert!(!debug.contains("super-secret-value"));

    let validation = AccountId::new("").expect_err("empty account");
    assert_eq!(validation.code, ErrorCode::InvalidInput);
    assert!(!validation.to_string().contains("alice"));

    let control = VerificationPurpose::new("bad\npurpose").expect_err("control character");
    assert_eq!(control.code, ErrorCode::InvalidInput);
    assert!(!control.to_string().contains("bad"));

    let fake = FakeLocalAuthProvider::new();
    let debug_key = ProtectedKeyId::new("top-secret").expect("key");
    fake.seal_protected_secret(
        &account,
        &debug_key,
        SecretBytes::new(b"super-secret-value".to_vec()).expect("secret"),
    );
    fake.register_signing_key(&account, &debug_key, SignatureAlgorithm::Ed25519);
    let debug = format!("{fake:?}");
    assert!(!debug.contains("alice"));
    assert!(!debug.contains("top-secret"));
    assert!(!debug.contains("super-secret-value"));
    assert!(debug.contains("FakeLocalAuthProvider"));
}

#[test]
fn receipt_fingerprints_do_not_expose_accounts_in_debug() {
    let (service, fake, owner) = harness();
    let alice = AccountId::new("alice@example.com").expect("account");
    let receipt = verified_receipt(&service, &fake, &owner, VerificationPolicy::BiometricOnly);
    let debug = format!("{receipt:?}");
    assert!(!debug.contains("alice@example.com"));
    assert!(debug.contains("Biometric"));

    let mut manager = AccountUnlockManager::default();
    manager.lock_account(&alice);
    let debug = format!("{manager:?}");
    assert!(!debug.contains("alice@example.com"));
    assert!(debug.contains("[REDACTED]"));
}

struct ReentrantWaker;

thread_local! {
    static REENTRANT_SERVICE: RefCell<Option<LocalAuthService>> = const { RefCell::new(None) };
}

impl ReentrantWaker {
    fn reenter() {
        let service = REENTRANT_SERVICE.with(|slot| slot.borrow().as_ref().cloned());
        if let Some(service) = service {
            let _ = service.subscribe();
            let _ = service.revision();
        }
    }
}

impl Wake for ReentrantWaker {
    fn wake(self: Arc<Self>) {
        Self::reenter();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        Self::reenter();
    }
}

#[test]
fn waker_reentrancy_is_safe_with_multiple_subscribers() {
    let (service, fake, _owner) = harness();
    let first = service.subscribe();
    let second = service.subscribe();
    REENTRANT_SERVICE.with(|slot| *slot.borrow_mut() = Some(service.clone()));
    let waker = Waker::from(Arc::new(ReentrantWaker));
    let mut task = TaskContext::from_waker(&waker);
    let mut next = first.try_next().expect("event reader");
    assert!(Pin::new(&mut next).poll(&mut task).is_pending());

    fake.set_not_enrolled(true, Some(RecoveryAction::OpenSettings));
    let snapshot = expect_completed(service.query_availability(
        VerificationPolicy::BiometricOnly,
        OperationContext::default(),
    ));
    assert_eq!(
        snapshot.readiness,
        LocalAuthReadiness::NotEnrolled {
            device_credential_configured: true,
            recovery: Some(RecoveryAction::OpenSettings),
        }
    );

    match poll_ready(&mut next) {
        Poll::Ready(Some(EventEnvelope {
            payload: Event::Value(LocalAuthEvent::AvailabilityChanged { snapshot: event }),
            ..
        })) => assert_eq!(event, snapshot),
        other => panic!("first subscriber did not receive the event: {other:?}"),
    }

    let mut next = second.try_next().expect("event reader");
    match poll_ready(&mut next) {
        Poll::Ready(Some(EventEnvelope {
            payload: Event::Value(LocalAuthEvent::AvailabilityChanged { .. }),
            ..
        })) => {}
        other => panic!("second subscriber did not receive the event: {other:?}"),
    }
    REENTRANT_SERVICE.with(|slot| {
        let _ = slot.borrow_mut().take();
    });
}
