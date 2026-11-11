//! Passkey registration/assertion, server verification and replay rejection.

mod support;

use gpui_auth::*;
use gpui_system::SurfaceId;
use std::time::{Duration, Instant};
use support::*;

fn creation(now: Instant, seed: u8) -> PasskeyCreationRequest {
    let mut challenge = [0u8; 32];
    challenge[0] = seed;
    passkey_creation(
        &challenge,
        now + Duration::from_secs(120),
        presentation(7, 3),
    )
}

fn assertion(now: Instant, seed: u8) -> PasskeyAssertionRequest {
    let mut challenge = [0u8; 32];
    challenge[0] = seed;
    passkey_assertion(
        &challenge,
        now + Duration::from_secs(120),
        presentation(7, 3),
    )
}

#[test]
fn registration_runs_provider_and_server_verifier_without_a_session() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let request = creation(now, 1);
    let result = harness
        .service
        .register_passkey(ExecutionOwner::Background, request, now)
        .expect("registration");
    assert_eq!(harness.passkey.creations(), 1);
    assert_eq!(harness.verifier.registrations(), 1);
    assert_eq!(result.rp_id.as_str(), "example.com");
    assert!(result.user_verified);
    assert!(result.session.is_none());
    assert!(result.credential_id.expose().starts_with(b"credential"));
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(harness.credentials.len(), 0);
}

#[test]
fn registration_can_establish_a_vault_backed_session() {
    let verifier = std::sync::Arc::new(FakePasskeyServerVerifier::with_account(
        provider(),
        account("passkey-account"),
    ));
    verifier.issue_session_next();
    let harness = Harness::with_services(
        AuthPlatform::Linux,
        std::sync::Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        verifier,
    );
    let now = Instant::now();
    let result = harness
        .service
        .register_passkey(ExecutionOwner::Background, creation(now, 2), now)
        .expect("registration");
    let summary = result.session.expect("session");
    assert_eq!(summary.account_id().as_str(), "passkey-account");
    assert_eq!(summary.auth_method(), AuthMethodKind::Passkey);
    let lease = harness
        .service
        .access_token(ExecutionOwner::Background, summary.account_id(), now)
        .expect("lease");
    assert_eq!(lease.expose_access_token(), b"passkey-access");
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(snapshot.accounts.len(), 1);
    assert!(!format!("{snapshot:?}").contains("passkey-access"));
}

#[test]
fn assertion_requires_an_allow_list_match_and_verified_account() {
    let verifier = std::sync::Arc::new(FakePasskeyServerVerifier::with_account(
        provider(),
        account("passkey-account"),
    ));
    let harness = Harness::with_services(
        AuthPlatform::Linux,
        std::sync::Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        verifier,
    );
    let now = Instant::now();
    let credential = CredentialId::new(b"credential-1".to_vec()).expect("credential");
    let request = assertion(now, 3)
        .with_allow_credentials([CredentialDescriptor::new(credential.clone(), [])])
        .expect("allow list");
    let result = harness
        .service
        .assert_passkey(ExecutionOwner::Background, request, now)
        .expect("assertion");
    assert_eq!(result.credential_id, credential);
    assert_eq!(result.account_id.as_str(), "passkey-account");
    assert_eq!(harness.verifier.assertions(), 1);
    assert_eq!(harness.passkey.last_allow(), vec![credential]);
    assert!(result.session.is_none());
}

#[test]
fn assertion_rejects_an_unlisted_credential_before_server_verification() {
    let verifier = std::sync::Arc::new(FakePasskeyServerVerifier::with_account(
        provider(),
        account("passkey-account"),
    ));
    let harness = Harness::with_services(
        AuthPlatform::Linux,
        std::sync::Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        verifier,
    );
    let now = Instant::now();
    let credential = CredentialId::new(b"credential-1".to_vec()).expect("credential");
    let request = assertion(now, 4)
        .with_allow_credentials([CredentialDescriptor::new(credential, [])])
        .expect("allow list");
    harness.passkey.mismatch_credential_next();
    let error = harness
        .service
        .assert_passkey(ExecutionOwner::Background, request, now)
        .expect_err("unlisted credential");
    assert!(matches!(error, AuthError::VerificationFailed(_)));
    assert_eq!(harness.verifier.assertions(), 0);
}

#[test]
fn challenge_replay_expiry_and_unknown_ceremonies_are_rejected() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let first = creation(now, 5);
    assert!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, first, now)
            .is_ok()
    );
    let replay = creation(now, 5);
    assert_eq!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, replay, now)
            .expect_err("challenge replay"),
        AuthError::StaleOrUnknownState
    );
    let expired = passkey_creation(&[9u8; 32], now - Duration::from_secs(1), presentation(7, 3));
    assert_eq!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, expired, now)
            .expect_err("expired"),
        AuthError::Expired
    );
    assert!(
        !harness
            .service
            .cancel_passkey_ceremony(CeremonyId::new(999).expect("id"))
            .expect("unknown")
    );
}

#[test]
fn relying_party_origin_and_user_verification_invariants_hold() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    harness.passkey.mismatch_rp_next();
    assert!(matches!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, creation(now, 6), now),
        Err(AuthError::VerificationFailed(_))
    ));
    assert_eq!(harness.verifier.registrations(), 0);

    harness.passkey.user_verified_next(false);
    assert_eq!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, creation(now, 7), now)
            .expect_err("user verification"),
        AuthError::UserPresenceRequired
    );

    assert!(
        PasskeyCreationRequest::new(
            RelyingPartyId::new("example.com").expect("rp"),
            Some(AuthOrigin::parse("https://other.example").expect("origin")),
            PasskeyChallenge::new([1u8; 32].to_vec()).expect("challenge"),
            now + Duration::from_secs(10),
            presentation(7, 3),
            PasskeyUserHandle::new(b"u".to_vec()).expect("handle"),
            AccountLabel::new("label").expect("label"),
            PublicKeyCoseAlgorithm::DEFAULT,
        )
        .is_err()
    );
}

#[test]
fn excluded_credentials_and_missing_credentials_are_typed() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let credential = CredentialId::new(b"credential-9".to_vec()).expect("credential");
    harness.passkey.set_credential_id(credential.clone());
    let request = creation(now, 8)
        .with_exclude_credentials([CredentialDescriptor::new(credential, [])])
        .expect("exclude");
    assert_eq!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, request, now)
            .expect_err("excluded"),
        AuthError::CredentialExcluded
    );

    harness
        .passkey
        .fail_next(PasskeyProviderError::NoMatchingCredentials);
    let error = harness
        .service
        .assert_passkey(ExecutionOwner::Background, assertion(now, 9), now)
        .expect_err("missing");
    assert_eq!(error, AuthError::NoMatchingCredentials);
    assert_eq!(
        error.to_system_error().code,
        gpui_system::ErrorCode::Unavailable
    );
}

#[test]
fn server_verifier_failure_discards_the_result() {
    let harness = Harness::new(AuthPlatform::Linux);
    harness
        .verifier
        .fail_registration(PasskeyVerificationError::SignatureInvalid);
    let now = Instant::now();
    let error = harness
        .service
        .register_passkey(ExecutionOwner::Background, creation(now, 10), now)
        .expect_err("verifier failure");
    assert!(matches!(error, AuthError::VerificationFailed(_)));
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(harness.credentials.len(), 0);
}

#[test]
fn two_phase_registration_is_cancelled_by_surface_disposal() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let ceremony_id = harness
        .service
        .begin_passkey_registration(ExecutionOwner::Ui, creation(now, 11), now)
        .expect("begin");
    assert!(harness.service.passkey_ceremony_active(ceremony_id));
    assert_eq!(
        harness.service.dispose_surface_generation(SurfaceId(7), 3),
        1
    );
    assert_eq!(harness.passkey.cancelled(), vec![ceremony_id]);
    assert_eq!(
        harness
            .service
            .complete_passkey_registration(ExecutionOwner::Background, ceremony_id, now)
            .expect_err("disposed"),
        AuthError::StaleCeremony
    );
}

#[test]
fn stale_completion_and_expected_account_mismatch_are_rejected() {
    let verifier = std::sync::Arc::new(FakePasskeyServerVerifier::with_account(
        provider(),
        account("passkey-account"),
    ));
    let harness = Harness::with_services(
        AuthPlatform::Linux,
        std::sync::Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        verifier,
    );
    let now = Instant::now();
    let ceremony_id = harness
        .service
        .begin_passkey_registration(ExecutionOwner::Ui, creation(now, 12), now)
        .expect("begin");
    harness
        .service
        .complete_passkey_registration(ExecutionOwner::Background, ceremony_id, now)
        .expect("complete");
    assert_eq!(
        harness
            .service
            .complete_passkey_registration(ExecutionOwner::Background, ceremony_id, now)
            .expect_err("stale"),
        AuthError::StaleCeremony
    );

    let request = assertion(now, 13).with_expected_account(account("someone-else"));
    let error = harness
        .service
        .assert_passkey(ExecutionOwner::Background, request, now)
        .expect_err("expected account");
    assert!(matches!(error, AuthError::VerificationFailed(_)));
}

#[test]
fn web_passkeys_require_secure_origin_and_conditional_mediation() {
    let harness = Harness::new(AuthPlatform::Web);
    let now = Instant::now();
    let request = creation(now, 14).with_mediation(PasskeyMediation::Conditional);
    let result = harness
        .service
        .register_passkey(ExecutionOwner::Background, request, now)
        .expect("conditional web registration");
    assert!(result.session.is_none());

    let without_origin = PasskeyCreationRequest::new(
        RelyingPartyId::new("example.com").expect("rp"),
        None,
        PasskeyChallenge::new(vec![3u8; 32]).expect("challenge"),
        now + Duration::from_secs(10),
        presentation(7, 3),
        PasskeyUserHandle::new(b"u".to_vec()).expect("handle"),
        AccountLabel::new("label").expect("label"),
        PublicKeyCoseAlgorithm::DEFAULT,
    )
    .expect("request");
    assert!(matches!(
        harness
            .service
            .register_passkey(ExecutionOwner::Background, without_origin, now),
        Err(AuthError::InvalidInput(_))
    ));
}

#[test]
fn passkey_challenge_and_user_handle_debug_are_redacted() {
    let challenge = PasskeyChallenge::new(b"0123456789abcdef".to_vec()).expect("challenge");
    let debug = format!("{challenge:?}");
    assert!(!debug.contains("0123456789abcdef"));
    assert!(debug.contains("length"));
    let handle = PasskeyUserHandle::new(b"private-handle".to_vec()).expect("handle");
    assert_eq!(format!("{handle:?}"), "PasskeyUserHandle([REDACTED])");
    let registration = ProviderPasskeyRegistration {
        ceremony_id: CeremonyId::new(1).expect("id"),
        credential_id: CredentialId::new(b"credential".to_vec()).expect("credential"),
        rp_id: RelyingPartyId::new("example.com").expect("rp"),
        client_data_json: ProviderResponseBytes::new(b"{\"private\":true}".to_vec())
            .expect("bytes"),
        attestation_object: ProviderResponseBytes::new(b"attestation".to_vec()).expect("bytes"),
        transports: vec![AuthenticatorTransport::Internal],
        user_verified: true,
        backup_eligible: true,
        backup_state: false,
        sign_count: 0,
    };
    let debug = format!("{registration:?}");
    assert!(!debug.contains("private"));
}

#[test]
fn logout_cancels_an_in_flight_assertion_bound_to_the_account() {
    let verifier = std::sync::Arc::new(FakePasskeyServerVerifier::with_account(
        provider(),
        account("passkey-account"),
    ));
    verifier.issue_session_next();
    let harness = Harness::with_services(
        AuthPlatform::Linux,
        std::sync::Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        verifier,
    );
    let now = Instant::now();
    let registration = harness
        .service
        .register_passkey(ExecutionOwner::Background, creation(now, 15), now)
        .expect("registration");
    assert!(registration.session.is_some());

    let request = assertion(now, 16).with_expected_account(account("passkey-account"));
    let ceremony_id = harness
        .service
        .begin_passkey_assertion(ExecutionOwner::Ui, request, now)
        .expect("begin assertion");
    assert!(harness.service.passkey_ceremony_active(ceremony_id));

    harness
        .service
        .logout_account(ExecutionOwner::Background, &account("passkey-account"), now)
        .expect("logout");
    assert!(!harness.service.passkey_ceremony_active(ceremony_id));
    assert!(harness.passkey.cancelled().contains(&ceremony_id));
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
}
