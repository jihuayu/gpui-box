//! Warm/cold browser ceremonies, redirect capture, concurrency and TOCTOU.

mod support;

use gpui_activation::ActivationSource;
use gpui_auth::*;
use gpui_secrets::Secrets;
use gpui_system::{CancelReason, SurfaceId};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use support::*;

struct Gate {
    entered: Mutex<Sender<()>>,
    release: Mutex<bool>,
    condition: Condvar,
}

impl Gate {
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

    fn enter(&self) {
        let sender = self
            .entered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = sender.send(());
    }

    fn wait(&self) {
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

struct GatedBackend {
    gate: Arc<Gate>,
    account: AccountId,
}

impl GatedBackend {
    fn tokens(&self, now: Instant) -> IssuedAccountTokens {
        IssuedAccountTokens {
            account_id: self.account.clone(),
            provider_id: provider(),
            label: AccountLabel::new("Gated account").expect("label"),
            scopes: vec![AuthScope::new("openid").expect("scope")],
            access_token: SecretTokenMaterial::from_string("gated-access").expect("token"),
            refresh_token: Some(SecretTokenMaterial::from_string("gated-refresh").expect("token")),
            id_token: None,
            authenticated_at: now,
            access_expires_at: Some(now + Duration::from_secs(900)),
            refresh_expires_at: Some(now + Duration::from_secs(86_400)),
        }
    }
}

impl TokenExchangeBackend for GatedBackend {
    fn exchange_authorization_code(
        &self,
        request: TokenExchangeRequest,
    ) -> Result<IssuedAccountTokens, AuthError> {
        self.gate.enter();
        self.gate.wait();
        Ok(self.tokens(request.now))
    }

    fn refresh_tokens(
        &self,
        request: TokenRefreshRequest,
    ) -> Result<RefreshedAccountTokens, AuthError> {
        self.gate.enter();
        self.gate.wait();
        Ok(RefreshedAccountTokens {
            access_token: SecretTokenMaterial::from_string("gated-refreshed").expect("token"),
            refresh_token: Some(SecretTokenMaterial::from_string("gated-rotated").expect("token")),
            id_token: None,
            access_expires_at: Some(request.now + Duration::from_secs(900)),
            refresh_expires_at: Some(request.now + Duration::from_secs(86_400)),
        })
    }
}

fn gated_harness(platform: AuthPlatform) -> (Harness, Arc<Gate>, Arc<GatedBackend>, Receiver<()>) {
    let (gate, receiver) = Gate::new();
    let backend = Arc::new(GatedBackend {
        gate: gate.clone(),
        account: account("account-1"),
    });
    let credentials = Arc::new(MemoryCredentials::default());
    let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone()).expect("secrets");
    let browser = Arc::new(FakeBrowserAuthProvider::new(49_152));
    let passkey = Arc::new(FakePasskeyProvider::new());
    let verifier = Arc::new(FakePasskeyServerVerifier::new(provider()));
    let service = AuthService::new(
        AuthServiceProviders {
            browser: browser.clone(),
            token_exchange: backend.clone(),
            passkey: passkey.clone(),
            passkey_verifier: verifier.clone(),
            local_presence: None,
            secrets: secrets.clone(),
        },
        AuthConfig {
            platform,
            ..AuthConfig::default()
        },
    )
    .expect("service");
    (
        Harness {
            service,
            browser,
            backend: Arc::new(FakeTokenExchangeBackend::new(account("unused"), provider())),
            passkey,
            verifier,
            credentials,
            secrets,
            platform,
        },
        gate,
        backend,
        receiver,
    )
}

#[test]
fn warm_login_exchanges_once_and_publishes_zero_secret_session() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let started = start_login(&harness, 7, 3, 1, None, now);
    assert!(harness.browser.last_authorize_url().is_some());
    assert_eq!(
        ready_callback(&harness, &started, "code-1", now),
        RedirectCaptureOutcome::ReadyForExchange {
            ceremony_id: started.handle.ceremony_id()
        }
    );
    let summary = harness
        .service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("exchange");
    assert_eq!(summary.account_id().as_str(), "account-1");
    assert_eq!(summary.state(), AccountSessionState::Active);
    assert_eq!(summary.auth_method(), AuthMethodKind::BrowserOAuth);
    assert!(summary.has_refresh_token());
    assert_eq!(harness.backend.exchanged(), 1);
    assert!(harness.credentials.len() >= 3);

    let snapshot = harness.service.snapshot(ExecutionOwner::Ui, now);
    assert_eq!(snapshot.accounts.len(), 1);
    assert_eq!(
        snapshot.primary_account.as_ref(),
        Some(summary.account_id())
    );
    let debug = format!("{snapshot:?}");
    for secret in [
        "access-token",
        "refresh-token",
        "id-token",
        "code-1",
        "state-",
        "dBjftJeZ",
    ] {
        assert!(!debug.contains(secret), "snapshot leaked {secret}");
    }
    let summary_debug = format!("{summary:?}");
    for secret in ["access-token", "refresh-token", "id-token", "code-1"] {
        assert!(!summary_debug.contains(secret));
    }

    let lease = harness
        .service
        .access_token(ExecutionOwner::Background, summary.account_id(), now)
        .expect("lease");
    assert_eq!(lease.expose_access_token(), b"access-token");
    assert!(lookup_owner_rejected(&harness, summary.account_id(), now));

    // Replaying the same callback must be rejected without mutating state.
    let replay = ready_callback(&harness, &started, "code-1", now);
    assert_eq!(replay, RedirectCaptureOutcome::RejectedStaleOrUnknownState);
    assert_eq!(harness.service.active_ceremony_count(), 0);
}

fn lookup_owner_rejected(harness: &Harness, account: &AccountId, now: Instant) -> bool {
    matches!(
        harness
            .service
            .access_token(ExecutionOwner::Ui, account, now),
        Err(AuthError::WrongExecutionOwner(_))
    ) && matches!(
        harness
            .service
            .access_token(ExecutionOwner::Renderer, account, now),
        Err(AuthError::WrongExecutionOwner(_))
    )
}

#[test]
fn oauth_errors_are_typed_and_do_not_leak_descriptions() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();

    let denied = start_login(&harness, 7, 3, 2, None, now);
    let raw = callback_for(
        &denied.redirect,
        &[
            ("error", "access_denied"),
            ("error_description", "private reason"),
            ("state", denied.state.expose()),
        ],
    );
    assert_eq!(
        harness
            .service
            .submit_callback(ExecutionOwner::Ui, &raw, now),
        RedirectCaptureOutcome::CeremonyCancelled {
            ceremony_id: denied.handle.ceremony_id(),
            reason: CancelReason::User
        }
    );
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());

    let invalid = start_login(&harness, 7, 3, 3, None, now);
    let raw = callback_for(
        &invalid.redirect,
        &[
            ("error", "invalid_request"),
            ("error_description", "private reason"),
            ("state", invalid.state.expose()),
        ],
    );
    let outcome = harness
        .service
        .submit_callback(ExecutionOwner::Ui, &raw, now);
    assert_eq!(
        outcome,
        RedirectCaptureOutcome::RejectedOAuthError {
            code: OAuthErrorCode::InvalidRequest
        }
    );
    let debug = format!("{outcome:?}");
    assert!(!debug.contains("private reason"));

    let server = start_login(&harness, 7, 3, 4, None, now);
    let raw = callback_for(
        &server.redirect,
        &[("error", "server_error"), ("state", server.state.expose())],
    );
    assert_eq!(
        harness
            .service
            .submit_callback(ExecutionOwner::Ui, &raw, now),
        RedirectCaptureOutcome::RejectedOAuthError {
            code: OAuthErrorCode::ServerError
        }
    );
}

#[test]
fn unknown_state_unmatched_uri_and_fragments_are_isolated() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let started = start_login(&harness, 7, 3, 5, None, now);

    assert_eq!(
        harness
            .service
            .submit_callback(ExecutionOwner::Ui, "myapp://settings/profile", now),
        RedirectCaptureOutcome::IgnoredUnmatchedUri
    );
    let wrong_state = callback_for(
        &started.redirect,
        &[("code", "attacker"), ("state", "state-ffffffffffffffff")],
    );
    assert_eq!(
        harness
            .service
            .submit_callback(ExecutionOwner::Ui, &wrong_state, now),
        RedirectCaptureOutcome::RejectedStaleOrUnknownState
    );
    let fragment = callback_for(&started.redirect, &[("state", started.state.expose())])
        + "#access_token=leaked";
    assert_eq!(
        harness
            .service
            .submit_callback(ExecutionOwner::Ui, &fragment, now),
        RedirectCaptureOutcome::RejectedMalformedCallback
    );

    // The legitimate ceremony survived every isolated rejection.
    assert!(ready_callback(&harness, &started, "code-5", now).is_ready());
}

#[test]
fn ready_callback_is_single_use_before_and_after_exchange() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let started = start_login(&harness, 7, 3, 6, None, now);
    assert!(ready_callback(&harness, &started, "code-6", now).is_ready());
    assert_eq!(
        ready_callback(&harness, &started, "code-6", now),
        RedirectCaptureOutcome::RejectedStaleOrUnknownState
    );
    harness
        .service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("exchange");
    assert_eq!(
        ready_callback(&harness, &started, "code-6", now),
        RedirectCaptureOutcome::RejectedStaleOrUnknownState
    );
}

#[test]
fn warm_and_cold_activation_events_are_ingested() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let warm = start_login(&harness, 7, 3, 7, None, now);
    let raw = callback_for(
        &warm.redirect,
        &[("code", "code-7"), ("state", warm.state.expose())],
    );
    let event = activation(&raw, ActivationSource::WarmLaunch, "warm-1");
    assert_eq!(
        harness
            .service
            .ingest_activation(ExecutionOwner::Ui, &event, now),
        RedirectCaptureOutcome::ReadyForExchange {
            ceremony_id: warm.handle.ceremony_id()
        }
    );

    let cold = Harness::new(AuthPlatform::Linux);
    let cold_event = activation(
        "http://127.0.0.1:49152/oauth/callback?code=x&state=state-0000000000000001",
        ActivationSource::ColdLaunch,
        "cold-1",
    );
    assert_eq!(
        cold.service
            .ingest_activation(ExecutionOwner::Ui, &cold_event, now),
        RedirectCaptureOutcome::ColdCeremonyUnavailable
    );
}

#[test]
fn cold_resumable_ceremony_hydrates_and_consumes_verifier_once() {
    let now = Instant::now();
    let harness = Harness::new(AuthPlatform::Linux);
    let state = state_for(8);
    let request = support::request(
        loopback("/oauth/callback"),
        presentation(7, 3),
        state.clone(),
        8,
    );
    let handle = harness
        .service
        .begin_cold_resumable_browser_auth(ExecutionOwner::Background, request, now)
        .expect("persist cold");
    assert!(harness.credentials.contains_fragment("pkce-verifier"));
    assert_eq!(harness.browser.launch_count(), 0);

    let restarted = harness.restart();
    assert_eq!(
        restarted
            .hydrate_cold_ceremonies(ExecutionOwner::Background, now)
            .expect("hydrate"),
        1
    );
    assert_eq!(
        restarted
            .launch_prepared_browser_auth(ExecutionOwner::Ui, handle.ceremony_id(), now)
            .expect_err("hydrated ceremony has no launch URL"),
        AuthError::StaleCeremony
    );
    let redirect = handle.resolved_redirect().clone();
    let raw = callback_for(
        &redirect,
        &[("code", "cold-code"), ("state", state.expose())],
    );
    let event = activation(&raw, ActivationSource::ColdLaunch, "cold-2");
    assert_eq!(
        restarted.ingest_activation(ExecutionOwner::Background, &event, now),
        RedirectCaptureOutcome::ReadyForExchange {
            ceremony_id: handle.ceremony_id()
        }
    );
    assert!(!harness.credentials.contains_fragment("pkce-verifier"));
    let summary = restarted
        .complete_browser_ceremony(ExecutionOwner::Background, handle.ceremony_id(), now)
        .expect("cold exchange");
    assert_eq!(summary.account_id().as_str(), "account-1");
    assert_eq!(harness.backend.exchanged(), 1);
    let replay = restarted.ingest_activation(ExecutionOwner::Background, &event, now);
    assert_eq!(replay, RedirectCaptureOutcome::RejectedStaleOrUnknownState);
}

#[test]
fn cold_record_expiry_is_deleted_on_hydration() {
    let now = Instant::now();
    let harness = Harness::new(AuthPlatform::Linux);
    let request = support::request(
        loopback("/oauth/callback"),
        presentation(7, 3),
        state_for(9),
        9,
    );
    let _ = harness
        .service
        .begin_cold_resumable_browser_auth(ExecutionOwner::Background, request, now)
        .expect("persist cold");
    let later = now + Duration::from_secs(301);
    let _ = harness
        .service
        .evaluate_expiries(ExecutionOwner::Background, later);
    assert_eq!(harness.service.active_ceremony_count(), 0);
    assert!(!harness.credentials.contains_fragment("pkce-verifier"));
}

#[test]
fn surface_conflicts_limits_cancellation_and_disposal_are_deterministic() {
    let harness = Harness::new(AuthPlatform::Linux);
    let now = Instant::now();
    let first = start_login(&harness, 7, 3, 10, None, now);
    let conflict = support::request(
        loopback("/oauth/callback"),
        presentation(7, 3),
        state_for(11),
        11,
    );
    assert!(matches!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, conflict, now),
        Err(AuthError::AlreadyActive)
    ));
    let duplicate_state = support::request(
        loopback("/oauth/callback"),
        presentation(9, 3),
        first.state.clone(),
        11,
    );
    assert!(matches!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, duplicate_state, now),
        Err(AuthError::AlreadyActive)
    ));

    let second = start_login(&harness, 8, 3, 12, None, now);
    assert_eq!(harness.service.active_ceremony_count(), 2);
    assert_eq!(
        harness.service.dispose_surface_generation(SurfaceId(0), 3),
        0
    );
    assert_eq!(
        harness.service.dispose_surface_generation(SurfaceId(7), 2),
        0
    );
    assert_eq!(
        harness.service.dispose_surface_generation(SurfaceId(7), 3),
        1
    );
    assert_eq!(harness.browser.cancelled_sessions().len(), 1);
    assert!(
        !harness
            .service
            .cancel_ceremony(ExecutionOwner::Ui, first.handle.ceremony_id(), now)
            .expect("cancel unknown")
    );
    assert!(
        harness
            .service
            .cancel_ceremony(ExecutionOwner::Ui, second.handle.ceremony_id(), now)
            .expect("cancel")
    );
    assert_eq!(harness.service.active_ceremony_count(), 0);
}

#[test]
fn ceremony_limit_is_bounded() {
    let harness = Harness::with_config(
        AuthPlatform::Linux,
        AuthConfig {
            max_ceremonies: 2,
            ..AuthConfig::default()
        },
    );
    let now = Instant::now();
    let _a = start_login(&harness, 1, 1, 13, None, now);
    let _b = start_login(&harness, 2, 1, 14, None, now);
    let request = support::request(
        loopback("/oauth/callback"),
        presentation(3, 1),
        state_for(15),
        15,
    );
    assert!(matches!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, request, now),
        Err(AuthError::CeremonyLimit)
    ));
}

#[test]
fn cancel_mid_exchange_discards_late_tokens_and_never_publishes_a_session() {
    let (harness, gate, _backend, entered) = gated_harness(AuthPlatform::Linux);
    let now = Instant::now();
    let started = start_login(&harness, 7, 3, 16, None, now);
    assert!(ready_callback(&harness, &started, "code-16", now).is_ready());
    let service = harness.service.clone();
    let ceremony_id = started.handle.ceremony_id();
    let worker = thread::spawn(move || {
        service.complete_browser_ceremony(ExecutionOwner::Background, ceremony_id, now)
    });
    entered.recv().expect("exchange entered");
    assert!(
        harness
            .service
            .cancel_ceremony(ExecutionOwner::Ui, ceremony_id, now)
            .expect("cancel")
    );
    gate.release();
    let result = worker.join().expect("worker");
    assert!(matches!(result, Err(AuthError::StaleCeremony)));
    assert!(harness.service.snapshot(ExecutionOwner::Ui, now).is_empty());
    assert_eq!(harness.credentials.len(), 0);
    assert!(matches!(
        harness
            .service
            .access_token(ExecutionOwner::Background, &account("account-1"), now),
        Err(AuthError::NotAuthenticated)
    ));
}

#[test]
fn web_platform_rejects_native_redirect_shapes() {
    let harness = Harness::new(AuthPlatform::Web);
    let now = Instant::now();
    let request = support::request(
        loopback("/oauth/callback"),
        presentation(7, 3),
        state_for(17),
        17,
    );
    assert!(matches!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, request, now),
        Err(AuthError::Unsupported(_))
    ));
    let custom = support::request(
        RedirectTarget::custom_scheme("com.example.app", None, "/callback").expect("custom"),
        presentation(7, 3),
        state_for(18),
        18,
    );
    assert!(matches!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, custom, now),
        Err(AuthError::Unsupported(_))
    ));
    let origin = AuthOrigin::parse("https://app.example.com").expect("origin");
    let web = support::request(
        RedirectTarget::web_same_origin(origin, "/oauth/callback").expect("web"),
        presentation(7, 3),
        state_for(19),
        19,
    );
    assert!(
        harness
            .service
            .begin_browser_auth(ExecutionOwner::Ui, web, now)
            .is_ok()
    );
}

#[test]
fn surface_scope_accepts_web_same_origin() {
    let harness = Harness::new(AuthPlatform::Web);
    let now = Instant::now();
    let origin = AuthOrigin::parse("http://localhost:3000").expect("origin");
    let redirect = RedirectTarget::web_same_origin(origin, "/cb").expect("web");
    let started = StartedLogin {
        handle: harness
            .service
            .begin_browser_auth(
                ExecutionOwner::Ui,
                support::request(redirect, presentation(7, 3), state_for(20), 20),
                now,
            )
            .expect("begin"),
        state: state_for(20),
        redirect: RedirectTarget::web_same_origin(
            AuthOrigin::parse("http://localhost:3000").expect("origin"),
            "/cb",
        )
        .expect("web"),
    };
    assert!(ready_callback(&harness, &started, "code-20", now).is_ready());
}

#[test]
fn cancel_and_surface_disposal_delete_cold_secrets() {
    let now = Instant::now();
    let harness = Harness::new(AuthPlatform::Linux);

    let first = harness
        .service
        .begin_cold_resumable_browser_auth(
            ExecutionOwner::Background,
            support::request(
                loopback("/oauth/callback"),
                presentation(7, 3),
                state_for(21),
                21,
            ),
            now,
        )
        .expect("cold one");
    assert!(harness.credentials.contains_fragment("pkce-verifier"));
    assert!(
        harness
            .service
            .cancel_ceremony(ExecutionOwner::Ui, first.ceremony_id(), now)
            .expect("cancel")
    );
    assert!(!harness.credentials.contains_fragment("pkce-verifier"));

    let _second = harness
        .service
        .begin_cold_resumable_browser_auth(
            ExecutionOwner::Background,
            support::request(
                loopback("/oauth/callback"),
                presentation(7, 3),
                state_for(22),
                22,
            ),
            now,
        )
        .expect("cold two");
    assert!(harness.credentials.contains_fragment("pkce-verifier"));
    assert_eq!(
        harness.service.dispose_surface_generation(SurfaceId(7), 3),
        1
    );
    assert!(!harness.credentials.contains_fragment("pkce-verifier"));
}
