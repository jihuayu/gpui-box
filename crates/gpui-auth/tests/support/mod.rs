//! Shared deterministic fakes and request builders for the auth suite.

#![allow(dead_code)]

use gpui_auth::*;
use gpui_secrets::{
    CredentialProvider, DeleteReceipt, ProviderResult, SecretAccess, SecretBytes, SecretOperation,
    SecretPolicy, Secrets,
};
use gpui_system::{
    Availability, Capability, Configuration, Evidence, PermissionState, Support, SurfaceId,
    SystemError,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Deterministic in-memory `CredentialProvider`. It implements the real
/// `gpui-secrets` boundary; it is not a mock of the vault itself.
#[derive(Default)]
pub struct MemoryCredentials {
    records: Mutex<BTreeMap<String, Vec<u8>>>,
    locked: AtomicBool,
}

impl MemoryCredentials {
    pub fn targets(&self) -> Vec<String> {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect()
    }

    pub fn contains_fragment(&self, fragment: &str) -> bool {
        let encoded = hex(fragment);
        self.targets()
            .iter()
            .any(|target| target.contains(&encoded))
    }

    pub fn len(&self) -> usize {
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::Relaxed);
    }

    fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
}

fn hex(value: &str) -> String {
    let mut result = String::new();
    for byte in value.as_bytes() {
        result.push_str(&format!("{byte:02x}"));
    }
    result
}

impl CredentialProvider for MemoryCredentials {
    fn capabilities(&self) -> Vec<Capability<SecretOperation>> {
        [
            SecretOperation::Put,
            SecretOperation::Get,
            SecretOperation::Delete,
            SecretOperation::RemoveAccount,
        ]
        .into_iter()
        .map(|operation| Capability {
            operation,
            support: Support::Native,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 1,
        })
        .collect()
    }

    fn validate_policy(&self, _: SecretPolicy) -> Result<(), SystemError> {
        Ok(())
    }

    fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
        if self.is_locked() {
            return Ok(SecretAccess::Locked);
        }
        self.records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target.to_owned(), value.expose().to_vec());
        Ok(SecretAccess::Available(()))
    }

    fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
        if self.is_locked() {
            return Ok(SecretAccess::Locked);
        }
        let value = self
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
            .cloned();
        match value {
            Some(value) => {
                SecretBytes::new(value).map(|value| SecretAccess::Available(Some(value)))
            }
            None => Ok(SecretAccess::Available(None)),
        }
    }

    fn delete(&self, target: &str) -> ProviderResult<DeleteReceipt> {
        if self.is_locked() {
            return Ok(SecretAccess::Locked);
        }
        let existed = self
            .records
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(target)
            .is_some();
        Ok(SecretAccess::Available(DeleteReceipt { existed }))
    }

    fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
        if self.is_locked() {
            return Ok(SecretAccess::Locked);
        }
        Ok(SecretAccess::Available(
            self.records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .keys()
                .filter(|target| target.starts_with(prefix))
                .cloned()
                .collect(),
        ))
    }
}

pub struct Harness {
    pub service: AuthService,
    pub browser: Arc<FakeBrowserAuthProvider>,
    pub backend: Arc<FakeTokenExchangeBackend>,
    pub passkey: Arc<FakePasskeyProvider>,
    pub verifier: Arc<FakePasskeyServerVerifier>,
    pub credentials: Arc<MemoryCredentials>,
    pub secrets: Secrets,
    pub platform: AuthPlatform,
}

impl Harness {
    pub fn new(platform: AuthPlatform) -> Self {
        Self::with_backend(
            platform,
            Arc::new(FakeTokenExchangeBackend::new(
                account("account-1"),
                provider(),
            )),
        )
    }

    pub fn with_config(platform: AuthPlatform, config: AuthConfig) -> Self {
        let credentials = Arc::new(MemoryCredentials::default());
        let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone())
            .expect("in-memory provider");
        let browser = Arc::new(FakeBrowserAuthProvider::new(49_152));
        let backend = Arc::new(FakeTokenExchangeBackend::new(
            account("account-1"),
            provider(),
        ));
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
            AuthConfig { platform, ..config },
        )
        .expect("service");
        Self {
            service,
            browser,
            backend,
            passkey,
            verifier,
            credentials,
            secrets,
            platform,
        }
    }

    pub fn with_backend(platform: AuthPlatform, backend: Arc<FakeTokenExchangeBackend>) -> Self {
        Self::with_services(
            platform,
            backend,
            Arc::new(FakePasskeyServerVerifier::new(provider())),
        )
    }

    pub fn with_services(
        platform: AuthPlatform,
        backend: Arc<FakeTokenExchangeBackend>,
        verifier: Arc<FakePasskeyServerVerifier>,
    ) -> Self {
        let credentials = Arc::new(MemoryCredentials::default());
        let secrets = Secrets::with_provider("gpui-auth-tests", credentials.clone())
            .expect("in-memory provider");
        let browser = Arc::new(FakeBrowserAuthProvider::new(49_152));
        let passkey = Arc::new(FakePasskeyProvider::new());
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
        Self {
            service,
            browser,
            backend,
            passkey,
            verifier,
            credentials,
            secrets,
            platform,
        }
    }

    /// Rebuild the service around the same vault, simulating process restart.
    pub fn restart(&self) -> AuthService {
        AuthService::new(
            AuthServiceProviders {
                browser: self.browser.clone(),
                token_exchange: self.backend.clone(),
                passkey: self.passkey.clone(),
                passkey_verifier: self.verifier.clone(),
                local_presence: None,
                secrets: self.secrets.clone(),
            },
            AuthConfig {
                platform: self.platform,
                ..AuthConfig::default()
            },
        )
        .expect("restarted service")
    }
}

pub fn account(value: &str) -> AccountId {
    AccountId::new(value).expect("account")
}

pub fn provider() -> ProviderId {
    ProviderId::new("idp.example").expect("provider")
}

pub fn scopes() -> AuthScopes {
    AuthScopes::new([
        AuthScope::new("openid").expect("scope"),
        AuthScope::new("profile").expect("scope"),
    ])
    .expect("scopes")
}

pub fn state_for(seed: u64) -> OAuthState {
    OAuthState::new(format!("state-{seed:016x}")).expect("state")
}

pub fn pkce(seed: u8) -> PkcePair {
    let mut verifier = String::from("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    if seed != 0 {
        let mut bytes = verifier.into_bytes();
        bytes[0] = b'A' + (seed % 26);
        verifier = String::from_utf8(bytes).expect("ascii");
    }
    PkcePair::from_verifier(PkceVerifier::new(verifier).expect("verifier"))
}

pub fn presentation(surface: u64, generation: u64) -> PresentationContext {
    PresentationContext::new(SurfaceId(surface), generation).expect("presentation")
}

pub fn loopback(path: &str) -> RedirectTarget {
    RedirectTarget::loopback(LoopbackHost::Ipv4, None, path).expect("loopback")
}

pub fn request(
    redirect: RedirectTarget,
    presentation: PresentationContext,
    state: OAuthState,
    seed: u8,
) -> BrowserAuthRequest {
    BrowserAuthRequest::new(
        provider(),
        "gpui-box-client",
        AuthEndpointUrl::parse("https://idp.example/oauth/authorize").expect("endpoint"),
        redirect,
        scopes(),
        state,
        pkce(seed),
        presentation,
    )
    .expect("request")
}

pub fn callback_for(redirect: &RedirectTarget, parameters: &[(&str, &str)]) -> String {
    let mut value = redirect.redirect_uri().expect("resolved redirect");
    value.push('?');
    for (index, (name, parameter)) in parameters.iter().enumerate() {
        if index > 0 {
            value.push('&');
        }
        value.push_str(&encode_query_component(name));
        value.push('=');
        value.push_str(&encode_query_component(parameter));
    }
    value
}

pub fn activation(
    raw: &str,
    source: gpui_activation::ActivationSource,
    id: &str,
) -> gpui_activation::ActivationEvent {
    let scheme = raw
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .unwrap_or("https");
    let policy = gpui_activation::UrlPolicy::new([scheme]).expect("url policy");
    let url = policy.parse(raw).expect("allowed url");
    gpui_activation::ActivationEvent::new(
        gpui_activation::ActivationId::new(id).expect("activation id"),
        source,
        gpui_activation::ActivationPayload::OpenUrl(url),
        std::time::UNIX_EPOCH + Duration::from_secs(1),
    )
    .expect("activation event")
}

/// Start one warm browser login and return its handle, state and resolved URI.
pub struct StartedLogin {
    pub handle: BrowserAuthHandle,
    pub state: OAuthState,
    pub redirect: RedirectTarget,
}

pub fn start_login(
    harness: &Harness,
    surface: u64,
    generation: u64,
    seed: u64,
    expected: Option<AccountId>,
    now: std::time::Instant,
) -> StartedLogin {
    let state = state_for(seed);
    let mut request = request(
        loopback("/oauth/callback"),
        presentation(surface, generation),
        state.clone(),
        (seed % 250) as u8,
    );
    if let Some(expected) = expected {
        request = request.with_expected_account(expected);
    }
    let handle = harness
        .service
        .begin_browser_auth(ExecutionOwner::Ui, request, now)
        .expect("begin browser auth");
    let redirect = handle.resolved_redirect().clone();
    StartedLogin {
        handle,
        state,
        redirect,
    }
}

pub fn ready_callback(
    harness: &Harness,
    started: &StartedLogin,
    code: &str,
    now: std::time::Instant,
) -> RedirectCaptureOutcome {
    let raw = callback_for(
        &started.redirect,
        &[("code", code), ("state", started.state.expose())],
    );
    harness
        .service
        .submit_callback(ExecutionOwner::Ui, &raw, now)
}

pub fn login(harness: &Harness, seed: u64, now: std::time::Instant) -> AccountSessionSummary {
    login_with(harness, seed, None, now)
}

pub fn login_with(
    harness: &Harness,
    seed: u64,
    expected: Option<AccountId>,
    now: std::time::Instant,
) -> AccountSessionSummary {
    let started = start_login(harness, 7, 3, seed, expected, now);
    let outcome = ready_callback(harness, &started, &format!("code-{seed}"), now);
    assert!(outcome.is_ready(), "callback should be ready: {outcome:?}");
    harness
        .service
        .complete_browser_ceremony(
            ExecutionOwner::Background,
            started.handle.ceremony_id(),
            now,
        )
        .expect("token exchange")
}

pub fn passkey_creation(
    challenge: &[u8],
    expires_at: std::time::Instant,
    presentation: PresentationContext,
) -> PasskeyCreationRequest {
    PasskeyCreationRequest::new(
        RelyingPartyId::new("example.com").expect("rp id"),
        Some(AuthOrigin::parse("https://login.example.com").expect("origin")),
        PasskeyChallenge::new(challenge.to_vec()).expect("challenge"),
        expires_at,
        presentation,
        PasskeyUserHandle::new(b"user-handle".to_vec()).expect("user handle"),
        AccountLabel::new("Primary account").expect("label"),
        PublicKeyCoseAlgorithm::DEFAULT,
    )
    .expect("creation request")
}

pub fn passkey_assertion(
    challenge: &[u8],
    expires_at: std::time::Instant,
    presentation: PresentationContext,
) -> PasskeyAssertionRequest {
    PasskeyAssertionRequest::new(
        RelyingPartyId::new("example.com").expect("rp id"),
        Some(AuthOrigin::parse("https://login.example.com").expect("origin")),
        PasskeyChallenge::new(challenge.to_vec()).expect("challenge"),
        expires_at,
        presentation,
    )
    .expect("assertion request")
}
