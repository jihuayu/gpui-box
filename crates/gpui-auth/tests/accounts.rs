//! Multi-account lifecycle, refresh single-flight, logout races, vault owner
//! enforcement and the explicit local-presence separation contract.

mod support;

use gpui_auth::*;
use gpui_secrets::{CredentialKey, SecretAccess, SecretBytes, SecretPolicy, Secrets};
use gpui_system::{CancelReason, Completion, OperationContext, SystemError};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use support::*;

struct RefreshGate {
    entered: Mutex<Sender<()>>,
    release: Mutex<bool>,
    condition: Condvar,
}

impl RefreshGate {
    fn new() -> (Arc<Self>, Receiver<()>) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(Self {
                entered: Mutex::new(sender),
                release: Mutex::new(false),
                condition: Condvar::new(),
            }),
            receiver,
        )
    }

    fn wait(&self) {
        let sender = self
            .entered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = sender.send(());
        drop(sender);
        let mut released = self
            .release
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while !*released {
            released = self
                .condition
                .wait(released)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self) {
        *self
            .release
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        self.condition.notify_all();
    }
}

struct RefreshGateBackend {
    inner: Arc<FakeTokenExchangeBackend>,
    gate: Arc<RefreshGate>,
}

impl TokenExchangeBackend for RefreshGateBackend {
    fn exchange_authorization_code(
        &self,
        request: TokenExchangeRequest,
    ) -> Result<IssuedAccountTokens, AuthError> {
        self.inner.exchange_authorization_code(request)
    }

    fn refresh_tokens(
        &self,
        request: TokenRefreshRequest,
    ) -> Result<RefreshedAccountTokens, AuthError> {
        self.gate.wait();
        self.inner.refresh_tokens(request)
    }

    fn revoke_tokens(&self, request: TokenRevocationRequest) -> Result<(), AuthError> {
        self.inner.revoke_tokens(request)
    }
}

#[derive(Default)]
struct PresenceGate {
    calls: AtomicUsize,
    allowed: AtomicBool,
}

impl LocalUserPresenceGate for PresenceGate {
    fn verify(
        &self,
        _purpose: LocalPresencePurpose,
        _context: &OperationContext,
    ) -> Result<LocalPresenceReceipt, AuthError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(if self.allowed.load(Ordering::Relaxed) {
            LocalPresenceReceipt::verified(Instant::now())
        } else {
            LocalPresenceReceipt::denied(Instant::now())
        })
    }
}

fn service_with_presence(
    platform: AuthPlatform,
    require_presence: bool,
    gate: Arc<PresenceGate>,
) -> (
    AuthService,
    Arc<MemoryCredentials>,
    Arc<FakeTokenExchangeBackend>,
) {
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone()).expect("secrets");
    let backend = Arc::new(FakeTokenExchangeBackend::new(
        account("account-1"),
        provider(),
    ));
    let service = AuthService::new(
        AuthServiceProviders {
            browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
            token_exchange: backend.clone(),
            passkey: Arc::new(FakePasskeyProvider::new()),
            passkey_verifier: Arc::new(FakePasskeyServerVerifier::with_account(
                provider(),
                account("account-1"),
            )),
            local_presence: Some(gate),
            secrets,
        },
        AuthConfig {
            platform,
            require_local_presence_for_sensitive_actions: require_presence,
            ..AuthConfig::default()
        },
    )
    .expect("service");
    (service, credentials, backend)
}

#[test]
fn multiple_accounts_refresh_and_switch_primary() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let first = login(&harness, 30, now);
    let second = login_with(&harness, 31, Some(account("account-2")), now);
    assert_eq!(first.account_id().as_str(), "account-1");
    assert_eq!(second.account_id().as_str(), "account-2");
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(snapshot.accounts.len(), 2);
    assert_eq!(snapshot.primary_account.as_ref(), Some(first.account_id()));

    harness
        .service
        .select_primary_account(ExecutionOwner::Ui, second.account_id())
        .expect("select primary");
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(snapshot.primary_account.as_ref(), Some(second.account_id()));
    assert!(
        snapshot
            .find(second.account_id())
            .is_some_and(AccountSessionSummary::is_primary)
    );

    let refreshed = harness
        .service
        .refresh_account(ExecutionOwner::Background, first.account_id(), now)
        .expect("refresh");
    assert_eq!(refreshed.state(), AccountSessionState::Active);
    assert_eq!(harness.backend.refreshed(), 1);
    let rotated = harness
        .service
        .access_token(ExecutionOwner::Background, first.account_id(), now)
        .expect("lease");
    assert_eq!(rotated.expose_access_token(), b"refreshed-access-token");
}

#[test]
fn refresh_is_single_flight_and_logout_wins_the_race() {
    let now = Instant::now();
    let (gate, entered) = RefreshGate::new();
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone()).expect("secrets");
    let inner = Arc::new(FakeTokenExchangeBackend::new(
        account("account-1"),
        provider(),
    ));
    let backend = Arc::new(RefreshGateBackend {
        inner: inner.clone(),
        gate: gate.clone(),
    });
    let service = AuthService::new(
        AuthServiceProviders {
            browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
            token_exchange: backend,
            passkey: Arc::new(FakePasskeyProvider::new()),
            passkey_verifier: Arc::new(FakePasskeyServerVerifier::new(provider())),
            local_presence: None,
            secrets: secrets.clone(),
        },
        AuthConfig {
            platform: AuthPlatform::Linux,
            ..AuthConfig::default()
        },
    )
    .expect("service");
    let started = start_login(&harness_for(&service), 7, 3, 32, None, now);
    assert!(ready_callback(&harness_for(&service), &started, "code-32", now).is_ready());
    service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("login");

    let worker_service = service.clone();
    let worker = thread::spawn(move || {
        worker_service.refresh_account(ExecutionOwner::Background, &account("account-1"), now)
    });
    entered.recv().expect("refresh entered");
    assert!(service.is_refreshing(&account("account-1")));
    assert!(matches!(
        service.refresh_account(ExecutionOwner::Background, &account("account-1"), now),
        Err(AuthError::Busy(_))
    ));
    service
        .logout_account(ExecutionOwner::Background, &account("account-1"), now)
        .expect("logout");
    gate.release();
    let result = worker.join().expect("worker");
    assert!(matches!(result, Err(AuthError::StaleCeremony)));
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(credentials.len(), 0);
    assert!(matches!(
        service.access_token(ExecutionOwner::Background, &account("account-1"), now),
        Err(AuthError::NotAuthenticated)
    ));
    assert_eq!(inner.revoked(), 1);
}

fn harness_for(service: &AuthService) -> Harness {
    // Only the service is used by the helpers called with this harness.
    Harness {
        service: service.clone(),
        browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
        backend: Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
        passkey: Arc::new(FakePasskeyProvider::new()),
        verifier: Arc::new(FakePasskeyServerVerifier::new(provider())),
        credentials: Arc::new(MemoryCredentials::default()),
        secrets: Secrets::with_provider("unused", Arc::new(MemoryCredentials::default()))
            .expect("secrets"),
        platform: AuthPlatform::Linux,
    }
}

#[test]
fn invalid_grant_moves_only_the_affected_account_to_reauth_required() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let first = login(&harness, 33, now);
    let second = login_with(&harness, 34, Some(account("account-2")), now);
    harness.backend.fail_refresh(AuthError::ReauthRequired);
    let error = harness
        .service
        .refresh_account(ExecutionOwner::Background, first.account_id(), now)
        .expect_err("refresh must fail");
    assert_eq!(error, AuthError::ReauthRequired);
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    let first = snapshot.find(first.account_id()).expect("first");
    let second = snapshot.find(second.account_id()).expect("second");
    assert_eq!(first.state(), AccountSessionState::ReauthRequired);
    assert!(!first.has_refresh_token());
    assert_eq!(second.state(), AccountSessionState::Active);
    assert!(matches!(
        harness
            .service
            .access_token(ExecutionOwner::Background, first.account_id(), now),
        Err(AuthError::NotAuthenticated)
    ));
}

#[test]
fn monotonic_expiry_advances_without_wall_clock() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let summary = login(&harness, 35, now);
    let later = now + Duration::from_secs(901);
    let changed = harness.service.evaluate_expiries(ExecutionOwner::Ui, later);
    assert_eq!(changed, vec![summary.account_id().clone()]);
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, later);
    assert_eq!(
        snapshot
            .find(summary.account_id())
            .expect("account")
            .state(),
        AccountSessionState::AccessTokenExpired
    );
    assert!(matches!(
        harness
            .service
            .access_token(ExecutionOwner::Background, summary.account_id(), later),
        Err(AuthError::ReauthRequired)
    ));
    let refreshed = harness
        .service
        .refresh_account(ExecutionOwner::Background, summary.account_id(), later)
        .expect("refresh after access expiry");
    assert_eq!(refreshed.state(), AccountSessionState::Active);
}

#[test]
fn vault_and_token_contracts_enforce_background_owner_and_redact_debug() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let summary = login(&harness, 36, now);
    let vault = SecretTokenVault::new(harness.secrets.clone());
    assert!(matches!(
        vault.read_access_token(
            ExecutionOwner::Ui,
            summary.account_id(),
            &OperationContext::default()
        ),
        Err(AuthError::WrongExecutionOwner(_))
    ));
    assert!(matches!(
        vault.read_access_token(
            ExecutionOwner::Renderer,
            summary.account_id(),
            &OperationContext::default()
        ),
        Err(AuthError::WrongExecutionOwner(_))
    ));

    let tokens = IssuedAccountTokens {
        account_id: summary.account_id().clone(),
        provider_id: provider(),
        label: AccountLabel::new("Audit").expect("label"),
        scopes: Vec::new(),
        access_token: SecretTokenMaterial::from_string("super-secret-access").expect("token"),
        refresh_token: Some(
            SecretTokenMaterial::from_string("super-secret-refresh").expect("token"),
        ),
        id_token: Some(SecretTokenMaterial::from_string("super-secret-id").expect("token")),
        authenticated_at: now,
        access_expires_at: None,
        refresh_expires_at: None,
    };
    let debug = format!("{tokens:?}");
    for secret in [
        "super-secret-access",
        "super-secret-refresh",
        "super-secret-id",
    ] {
        assert!(!debug.contains(secret));
    }
    let exchange = TokenExchangeRequest {
        ceremony_id: CeremonyId::new(1).expect("ceremony"),
        provider_id: provider(),
        account_hint: None,
        code: SecretTokenMaterial::from_string("super-secret-code").expect("code"),
        redirect: loopback("/cb"),
        code_verifier: PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            .expect("verifier"),
        nonce: None,
        scopes: Vec::new(),
        now,
    };
    let debug = format!("{exchange:?}");
    for secret in ["super-secret-code", "dBjftJeZ4CVP"] {
        assert!(!debug.contains(secret));
    }
    assert!(debug.contains("REDACTED"));

    let events = harness.service.events_since(harness.service.subscribe());
    let debug = format!("{events:?}");
    for secret in ["access-token", "refresh-token", "code-36", "state-"] {
        assert!(!debug.contains(secret), "event batch leaked {secret}");
    }
}

#[test]
fn local_presence_never_substitutes_for_remote_exchange() {
    let gate = Arc::new(PresenceGate::default());
    gate.allowed.store(true, Ordering::Relaxed);
    let (service, credentials, backend) =
        service_with_presence(AuthPlatform::Linux, true, gate.clone());
    let now = Instant::now();
    let receipt = service
        .verify_local_presence(
            ExecutionOwner::Ui,
            LocalPresencePurpose::StepUpCeremony,
            now,
        )
        .expect("local presence");
    assert!(receipt.verified);
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(credentials.len(), 0);
    assert_eq!(backend.exchanged(), 0);

    let state = state_for(37);
    let request = support::request(
        loopback("/oauth/callback"),
        presentation(7, 3),
        state.clone(),
        37,
    )
    .with_expected_account(account("account-1"));
    let handle = service
        .begin_browser_auth(ExecutionOwner::Ui, request, now)
        .expect("step-up launch");
    assert_eq!(gate.calls.load(Ordering::Relaxed), 2);
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    let redirect = handle.resolved_redirect().clone();
    let raw = callback_for(
        &redirect,
        &[("code", "step-up-code"), ("state", state.expose())],
    );
    assert!(
        service
            .submit_callback(ExecutionOwner::Ui, &raw, now)
            .is_ready()
    );
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(credentials.len(), 0);
    let summary = service
        .complete_browser_ceremony(ExecutionOwner::Background, handle.ceremony_id(), now)
        .expect("remote exchange");
    assert_eq!(summary.account_id().as_str(), "account-1");
    assert_eq!(backend.exchanged(), 1);
    assert!(credentials.len() >= 3);
}

#[test]
fn configured_presence_gate_blocks_logout_until_verified() {
    let gate = Arc::new(PresenceGate::default());
    let (service, credentials, _) = service_with_presence(AuthPlatform::Linux, true, gate.clone());
    let now = Instant::now();
    let harness = Harness {
        service: service.clone(),
        browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
        backend: Arc::new(FakeTokenExchangeBackend::new(
            account("account-1"),
            provider(),
        )),
        passkey: Arc::new(FakePasskeyProvider::new()),
        verifier: Arc::new(FakePasskeyServerVerifier::new(provider())),
        credentials: credentials.clone(),
        secrets: Secrets::with_provider("unused", Arc::new(MemoryCredentials::default()))
            .expect("secrets"),
        platform: AuthPlatform::Linux,
    };
    let started = start_login(&harness, 7, 3, 38, None, now);
    let raw = callback_for(
        &started.redirect,
        &[("code", "code-38"), ("state", started.state.expose())],
    );
    // Presence is required for step-up only; this login has no expected account.
    assert_eq!(
        service.submit_callback(ExecutionOwner::Ui, &raw, now),
        RedirectCaptureOutcome::ReadyForExchange {
            ceremony_id: started.handle.ceremony_id()
        }
    );
    service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("login");
    gate.allowed.store(false, Ordering::Relaxed);
    assert_eq!(
        service
            .logout_account(ExecutionOwner::Background, &account("account-1"), now)
            .unwrap_err(),
        AuthError::UserPresenceDenied
    );
    assert_eq!(service.snapshot(ExecutionOwner::Ui, now).accounts.len(), 1);
    gate.allowed.store(true, Ordering::Relaxed);
    service
        .logout_account(ExecutionOwner::Background, &account("account-1"), now)
        .expect("verified logout");
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(credentials.len(), 0);
}

#[test]
fn logout_all_is_bounded_and_emits_public_events() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let _ = login(&harness, 39, now);
    let _ = login_with(&harness, 40, Some(account("account-2")), now);
    let cursor = harness.service.subscribe();
    assert_eq!(
        harness
            .service
            .logout_all(ExecutionOwner::Background, now)
            .expect("logout all"),
        2
    );
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(harness.credentials.len(), 0);
    let batch = harness.service.events_since(cursor);
    assert!(batch.events.iter().any(|event| matches!(
        event,
        AuthEvent::AccountLoggedOut { account_id } if account_id.as_str() == "account-1"
    )));
    let debug = format!("{:?}", batch.events);
    assert!(!debug.contains("access-token"));
}

#[test]
fn cancel_reason_is_public_and_static() {
    let error = AuthError::Cancelled(CancelReason::User);
    assert_eq!(
        error.to_system_error().message,
        "Authentication was cancelled by the user"
    );
    assert_eq!(
        error.to_system_error().code,
        gpui_system::ErrorCode::Unavailable
    );
    let system: SystemError = error.to_system_error();
    assert!(!system.message.contains('/'));
}

#[test]
fn account_limit_failure_clears_the_commit_guard() {
    let harness = Harness::with_config(
        AuthPlatform::Linux,
        AuthConfig {
            max_accounts: 1,
            ..AuthConfig::default()
        },
    );
    let now = Instant::now();
    let first = login(&harness, 41, now);
    let second = start_login(&harness, 7, 3, 42, Some(account("account-2")), now);
    assert!(ready_callback(&harness, &second, "code-42", now).is_ready());
    assert_eq!(
        harness
            .service
            .complete_browser_ceremony(ExecutionOwner::Background, second.handle.ceremony_id(), now)
            .expect_err("account limit"),
        AuthError::AccountLimit
    );
    assert_eq!(harness.service.active_ceremony_count(), 0);
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(snapshot.accounts[0].account_id(), first.account_id());
    assert!(matches!(
        harness
            .service
            .access_token(ExecutionOwner::Background, &account("account-2"), now),
        Err(AuthError::NotAuthenticated)
    ));
}

fn issued(account_id: AccountId, access: &str, now: Instant) -> IssuedAccountTokens {
    IssuedAccountTokens {
        account_id,
        provider_id: provider(),
        label: AccountLabel::new("Regression").expect("label"),
        scopes: Vec::new(),
        access_token: SecretTokenMaterial::from_string(access).expect("access"),
        refresh_token: Some(
            SecretTokenMaterial::from_string("regression-refresh").expect("refresh"),
        ),
        id_token: None,
        authenticated_at: now,
        access_expires_at: None,
        refresh_expires_at: None,
    }
}

#[test]
fn vault_deletion_is_service_scoped_and_cold_account_cannot_collide() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let context = OperationContext::default();
    let shared = account("shared-account");
    let network_key =
        CredentialKey::new(shared.as_str(), "gpui-network/v1", "proxy-password").expect("key");
    harness
        .secrets
        .put(
            &network_key,
            SecretBytes::new(b"network-secret".to_vec()).expect("secret"),
            SecretPolicy::default(),
            &context,
        )
        .expect("network put");

    let vault = SecretTokenVault::new(harness.secrets.clone());
    vault
        .store_issued(
            ExecutionOwner::Background,
            &issued(shared.clone(), "auth-access", now),
            &context,
        )
        .expect("auth put");
    assert_eq!(
        vault
            .delete_account(ExecutionOwner::Background, &shared, &context)
            .expect("delete auth tokens"),
        2
    );
    assert!(matches!(
        harness
            .secrets
            .get(&network_key, &context)
            .expect("network get"),
        Completion::Completed(SecretAccess::Available(Some(_)))
    ));

    let cold = ColdCeremonyStore::new(vault.clone());
    let ceremony_id = CeremonyId::new(700).expect("ceremony");
    let verifier =
        PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").expect("verifier");
    cold.persist(
        ExecutionOwner::Background,
        &ColdRedirectDescriptor {
            ceremony_id,
            redirect: loopback("/oauth/callback"),
            state: state_for(70),
            nonce: None,
            scopes: Vec::new(),
            presentation: presentation(7, 3),
            remaining_ttl: Duration::from_secs(300),
            expected_account: None,
            provider_id: provider(),
        },
        &verifier,
        &context,
    )
    .expect("persist cold");

    let cold_named = account("cold");
    vault
        .store_issued(
            ExecutionOwner::Background,
            &issued(cold_named.clone(), "cold-access", now),
            &context,
        )
        .expect("cold-named account tokens");
    assert_eq!(
        vault
            .delete_account(ExecutionOwner::Background, &cold_named, &context)
            .expect("delete cold-named account tokens"),
        2
    );
    assert_eq!(
        cold.list(ExecutionOwner::Background, &context),
        vec![ceremony_id]
    );
    assert!(harness.credentials.contains_fragment("pkce-verifier"));
    assert!(AccountId::new("gpui-auth/cold").is_err());
}

#[test]
fn non_rotating_refresh_preserves_the_existing_refresh_token() {
    let harness = Harness::new(AuthPlatform::Linux);
    harness.backend.without_refresh_rotation();
    let now = Instant::now();
    let summary = login(&harness, 71, now);
    assert!(summary.has_refresh_token());
    for expected in 1..=2 {
        let refreshed = harness
            .service
            .refresh_account(ExecutionOwner::Background, summary.account_id(), now)
            .expect("non-rotating refresh");
        assert!(refreshed.has_refresh_token());
        assert_eq!(refreshed.state(), AccountSessionState::Active);
        assert_eq!(harness.backend.refreshed(), expected);
        let lease = harness
            .service
            .access_token(ExecutionOwner::Background, summary.account_id(), now)
            .expect("lease after refresh");
        assert_eq!(lease.expose_access_token(), b"refreshed-access-token");
    }
}

#[test]
fn locked_logout_is_retryable_and_cleans_cold_records_after_unlock() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let summary = login(&harness, 72, now);
    let cold = harness
        .service
        .begin_cold_resumable_browser_auth(
            ExecutionOwner::Background,
            support::request(
                loopback("/oauth/callback"),
                presentation(9, 4),
                state_for(73),
                73,
            )
            .with_expected_account(summary.account_id().clone()),
            now,
        )
        .expect("cold step-up");
    assert!(harness.credentials.contains_fragment("pkce-verifier"));
    let cursor = harness.service.subscribe();
    harness.credentials.set_locked(true);

    let error = harness
        .service
        .logout_account(ExecutionOwner::Background, summary.account_id(), now)
        .expect_err("locked vault must fail");
    assert!(matches!(error, AuthError::Unavailable(_)));
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    let record = snapshot
        .find(summary.account_id())
        .expect("retained account");
    assert_eq!(record.state(), AccountSessionState::LoggingOut);
    assert!(matches!(
        harness
            .service
            .access_token(ExecutionOwner::Background, summary.account_id(), now),
        Err(AuthError::NotAuthenticated)
    ));
    assert!(matches!(
        harness
            .service
            .refresh_account(ExecutionOwner::Background, summary.account_id(), now),
        Err(AuthError::NotAuthenticated)
    ));
    assert!(matches!(
        harness
            .service
            .select_primary_account(ExecutionOwner::Ui, summary.account_id()),
        Err(AuthError::NotAuthenticated)
    ));
    assert!(harness.credentials.contains_fragment("pkce-verifier"));

    harness.credentials.set_locked(false);
    harness
        .service
        .logout_account(ExecutionOwner::Background, summary.account_id(), now)
        .expect("retry after unlock");
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert!(!harness.credentials.contains_fragment("access-token"));
    assert!(!harness.credentials.contains_fragment("refresh-token"));
    assert!(!harness.credentials.contains_fragment("pkce-verifier"));
    assert_eq!(harness.service.active_ceremony_count(), 0);
    assert!(cold.ceremony_id().get() > 0);
    let events = harness.service.events_since(cursor);
    assert!(events.events.iter().any(|event| matches!(
        event,
        AuthEvent::AccountLoggedOut { account_id } if account_id == summary.account_id()
    )));
}

#[test]
fn stale_refresh_does_not_overwrite_a_newer_login_generation() {
    let now = Instant::now();
    let (gate, entered) = RefreshGate::new();
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone()).expect("secrets");
    let inner = Arc::new(FakeTokenExchangeBackend::new(
        account("account-1"),
        provider(),
    ));
    inner.set_access_token("old-access");
    let backend = Arc::new(RefreshGateBackend {
        inner: inner.clone(),
        gate: gate.clone(),
    });
    let service = AuthService::new(
        AuthServiceProviders {
            browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
            token_exchange: backend,
            passkey: Arc::new(FakePasskeyProvider::new()),
            passkey_verifier: Arc::new(FakePasskeyServerVerifier::new(provider())),
            local_presence: None,
            secrets,
        },
        AuthConfig {
            platform: AuthPlatform::Linux,
            ..AuthConfig::default()
        },
    )
    .expect("service");
    let harness = harness_for(&service);
    let initial = start_login(&harness, 7, 3, 74, None, now);
    assert!(ready_callback(&harness, &initial, "code-74", now).is_ready());
    service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            initial.handle.ceremony_id(),
            now,
        )
        .expect("initial login");
    assert_eq!(
        service
            .access_token(ExecutionOwner::Background, &account("account-1"), now)
            .expect("old lease")
            .expose_access_token(),
        b"old-access"
    );

    let worker_service = service.clone();
    let worker = thread::spawn(move || {
        worker_service.refresh_account(ExecutionOwner::Background, &account("account-1"), now)
    });
    entered.recv().expect("refresh entered");

    inner.set_access_token("new-access");
    let newer = start_login(&harness, 7, 3, 75, None, now);
    assert!(ready_callback(&harness, &newer, "code-75", now).is_ready());
    service
        .complete_browser_ceremony(ExecutionOwner::Background, newer.handle.ceremony_id(), now)
        .expect("newer login");
    assert_eq!(
        service
            .access_token(ExecutionOwner::Background, &account("account-1"), now)
            .expect("new lease")
            .expose_access_token(),
        b"new-access"
    );

    gate.release();
    let stale = worker.join().expect("worker");
    assert!(matches!(stale, Err(AuthError::StaleCeremony)));
    assert_eq!(
        service
            .access_token(ExecutionOwner::Background, &account("account-1"), now)
            .expect("newer lease survives")
            .expose_access_token(),
        b"new-access"
    );
    assert_eq!(inner.refreshed(), 1);
    assert!(credentials.len() >= 3);
}

#[test]
fn logout_all_propagates_locked_vault_errors_and_retries_cleanly() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let _ = login(&harness, 76, now);
    let _ = login_with(&harness, 77, Some(account("account-2")), now);
    harness.credentials.set_locked(true);
    let error = harness
        .service
        .logout_all(ExecutionOwner::Background, now)
        .expect_err("locked vault must fail");
    assert!(matches!(error, AuthError::Unavailable(_)));
    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(
        snapshot
            .find(&account("account-1"))
            .expect("first account")
            .state(),
        AccountSessionState::LoggingOut
    );
    assert_eq!(
        snapshot
            .find(&account("account-2"))
            .expect("second account")
            .state(),
        AccountSessionState::Active
    );
    harness.credentials.set_locked(false);
    assert_eq!(
        harness
            .service
            .logout_all(ExecutionOwner::Background, now)
            .expect("retry"),
        2
    );
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert!(!harness.credentials.contains_fragment("access-token"));
}

#[test]
fn late_refresh_failure_does_not_escape_logging_out() {
    let now = Instant::now();
    let (gate, entered) = RefreshGate::new();
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone()).expect("secrets");
    let inner = Arc::new(FakeTokenExchangeBackend::new(
        account("account-1"),
        provider(),
    ));
    let backend = Arc::new(RefreshGateBackend {
        inner: inner.clone(),
        gate: gate.clone(),
    });
    let service = AuthService::new(
        AuthServiceProviders {
            browser: Arc::new(FakeBrowserAuthProvider::new(49_152)),
            token_exchange: backend,
            passkey: Arc::new(FakePasskeyProvider::new()),
            passkey_verifier: Arc::new(FakePasskeyServerVerifier::new(provider())),
            local_presence: None,
            secrets,
        },
        AuthConfig {
            platform: AuthPlatform::Linux,
            ..AuthConfig::default()
        },
    )
    .expect("service");
    let harness = harness_for(&service);
    let started = start_login(&harness, 7, 3, 78, None, now);
    assert!(ready_callback(&harness, &started, "code-78", now).is_ready());
    service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("login");

    let worker_service = service.clone();
    let worker = thread::spawn(move || {
        worker_service.refresh_account(ExecutionOwner::Background, &account("account-1"), now)
    });
    entered.recv().expect("refresh entered");
    credentials.set_locked(true);
    assert!(matches!(
        service.logout_account(ExecutionOwner::Background, &account("account-1"), now),
        Err(AuthError::Unavailable(_))
    ));
    inner.fail_refresh(AuthError::ReauthRequired);
    gate.release();
    assert_eq!(
        worker.join().expect("worker").expect_err("refresh failure"),
        AuthError::ReauthRequired
    );
    let snapshot = service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(
        snapshot
            .find(&account("account-1"))
            .expect("retained")
            .state(),
        AccountSessionState::LoggingOut
    );

    credentials.set_locked(false);
    service
        .logout_account(ExecutionOwner::Background, &account("account-1"), now)
        .expect("retry after unlock");
    assert!(service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert!(!credentials.contains_fragment("access-token"));
}
