//! The explicit push-provider boundary, plus deterministic fake and unavailable
//! implementations.
//!
//! A provider owns transport registration. It never persists anything: it
//! returns [`RegistrationMaterial`], whose raw bytes are written only to
//! `gpui-secrets` by the service. Provider calls happen with no internal service
//! lock held.

use crate::model::{
    IncomingPushFrame, ProviderCapabilities, ProviderConfiguration, ProviderHealth,
    PushProviderDescriptor, PushProviderKind, TokenFingerprint,
};
use crate::platform::PushPlatform;
use gpui_system::{ErrorCode, SystemError};
use std::{
    any::Any,
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};

/// Largest accepted single registration secret.
pub const MAX_REGISTRATION_SECRET_BYTES: usize = 2048;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn unavailable(message: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::Unavailable, message);
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

fn outage(message: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::Unavailable, message);
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}

fn provider_failure() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::PlatformFailure,
        "Push provider registration failed",
    );
    error.retry = gpui_system::RetryAdvice::Retry;
    error
}

/// Web Push registration material: an endpoint capability URL plus the two
/// subscription keys. None of these bytes may appear in a public snapshot.
pub struct WebPushRegistrationMaterial {
    endpoint: Vec<u8>,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
}

impl WebPushRegistrationMaterial {
    pub fn new(
        endpoint: impl Into<Vec<u8>>,
        p256dh: impl Into<Vec<u8>>,
        auth: impl Into<Vec<u8>>,
    ) -> Result<Self, SystemError> {
        let endpoint = endpoint.into();
        let p256dh = p256dh.into();
        let auth = auth.into();
        if endpoint.is_empty()
            || endpoint.len() > MAX_REGISTRATION_SECRET_BYTES
            || p256dh.is_empty()
            || p256dh.len() > MAX_REGISTRATION_SECRET_BYTES
            || auth.is_empty()
            || auth.len() > MAX_REGISTRATION_SECRET_BYTES
        {
            return Err(invalid(
                "Web Push registration material is empty or oversized",
            ));
        }
        Ok(Self {
            endpoint,
            p256dh,
            auth,
        })
    }

    pub fn endpoint(&self) -> &[u8] {
        &self.endpoint
    }

    pub fn p256dh(&self) -> &[u8] {
        &self.p256dh
    }

    pub fn auth(&self) -> &[u8] {
        &self.auth
    }

    /// Take the three secrets, leaving the struct inert for its zeroizing drop.
    pub fn into_parts(mut self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        (
            std::mem::take(&mut self.endpoint),
            std::mem::take(&mut self.p256dh),
            std::mem::take(&mut self.auth),
        )
    }
}

impl Drop for WebPushRegistrationMaterial {
    fn drop(&mut self) {
        self.endpoint.fill(0);
        self.p256dh.fill(0);
        self.auth.fill(0);
    }
}

impl fmt::Debug for WebPushRegistrationMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebPushRegistrationMaterial")
            .field("endpoint_bytes", &self.endpoint.len())
            .field("p256dh_bytes", &self.p256dh.len())
            .field("auth_bytes", &self.auth.len())
            .finish()
    }
}

/// Raw registration material returned by a provider.
///
/// It deliberately has no `Clone`, no serialization and a redacted `Debug`. The
/// service moves the bytes into [`gpui_secrets::SecretBytes`] and zeroizes the
/// remainder on drop.
pub struct RegistrationMaterial {
    device_token: Option<Vec<u8>>,
    web_push: Option<WebPushRegistrationMaterial>,
    fingerprint: TokenFingerprint,
}

impl RegistrationMaterial {
    pub fn new(
        device_token: Option<Vec<u8>>,
        web_push: Option<WebPushRegistrationMaterial>,
    ) -> Result<Self, SystemError> {
        let fingerprint = match (&device_token, &web_push) {
            (Some(token), _) if !token.is_empty() => TokenFingerprint::of(token),
            (None, Some(web)) => TokenFingerprint::of(web.endpoint()),
            _ => {
                return Err(invalid(
                    "Registration material requires a device token or a Web Push subscription",
                ));
            }
        };
        if let Some(token) = &device_token
            && token.len() > MAX_REGISTRATION_SECRET_BYTES
        {
            return Err(invalid("Device token exceeds its size bound"));
        }
        Ok(Self {
            device_token,
            web_push,
            fingerprint,
        })
    }

    pub fn device_token_bytes(&self) -> Option<&[u8]> {
        self.device_token.as_deref()
    }

    pub fn web_push(&self) -> Option<&WebPushRegistrationMaterial> {
        self.web_push.as_ref()
    }

    pub fn fingerprint(&self) -> &TokenFingerprint {
        &self.fingerprint
    }

    /// Take both halves, leaving the struct inert for its zeroizing drop.
    pub fn into_parts(mut self) -> (Option<Vec<u8>>, Option<WebPushRegistrationMaterial>) {
        (self.device_token.take(), self.web_push.take())
    }
}

impl Drop for RegistrationMaterial {
    fn drop(&mut self) {
        if let Some(token) = &mut self.device_token {
            token.fill(0);
        }
    }
}

impl fmt::Debug for RegistrationMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RegistrationMaterial")
            .field("fingerprint", &self.fingerprint)
            .field(
                "device_token_bytes",
                &self.device_token.as_ref().map(Vec::len),
            )
            .field("web_push", &self.web_push)
            .finish()
    }
}

/// A marker type for providers that cannot produce registration material.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnavailableRegistrationMaterial;

/// What happened when a provider was asked to revoke a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderRevocation {
    Revoked,
    /// The provider no longer knows the token; local cleanup is still required.
    NotFound,
    /// The provider has no revocation API; local cleanup is still required.
    Unsupported,
    /// The provider could not be reached; local revocation is recorded cleanly.
    Unreachable,
}

impl ProviderRevocation {
    /// True when the local side may proceed. Every outcome does; the value only
    /// records whether the remote side confirmed the revocation.
    pub const fn is_clean(self) -> bool {
        true
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::NotFound => "not_found",
            Self::Unsupported => "unsupported",
            Self::Unreachable => "unreachable",
        }
    }
}

impl fmt::Display for ProviderRevocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Explicit native/transport push boundary.
///
/// Every method is safe to call from a background thread and must not block on
/// UI. Capability, configuration and health are observations that the service
/// re-checks before every side effect.
pub trait PushProvider: Send + Sync + 'static {
    /// Describe this provider/channel without registering anything.
    fn descriptor(&self) -> PushProviderDescriptor;

    /// Acquire fresh registration material.
    fn register(&self) -> Result<RegistrationMaterial, SystemError>;

    /// Mint replacement material. Called for both caller-requested and
    /// provider-initiated rotation.
    fn rotate(&self) -> Result<RegistrationMaterial, SystemError>;

    /// Best-effort remote revocation. `Unsupported`/`Unreachable` are normal.
    fn revoke(&self, fingerprint: &TokenFingerprint) -> Result<ProviderRevocation, SystemError>;

    fn as_any(&self) -> &dyn Any;
}

/// True when a real native push provider is installed for the target. This
/// contract crate installs [`UnavailablePushProvider`] everywhere, so the answer
/// is always `false`; the platform matrix describes intended host mechanisms.
pub fn push_provider_available(_platform: PushPlatform) -> bool {
    false
}

/// Honest fail-closed provider used when no host adapter is installed.
#[derive(Debug, Clone, Copy)]
pub struct UnavailablePushProvider {
    kind: PushProviderKind,
}

impl UnavailablePushProvider {
    pub const fn new(kind: PushProviderKind) -> Self {
        Self { kind }
    }
}

impl Default for UnavailablePushProvider {
    fn default() -> Self {
        Self::new(PushProviderKind::CustomChannel)
    }
}

impl PushProvider for UnavailablePushProvider {
    fn descriptor(&self) -> PushProviderDescriptor {
        PushProviderDescriptor::new(self.kind, "unavailable")
            .with_configuration(ProviderConfiguration::NotConfigured)
            .with_health(ProviderHealth::Unavailable)
            .with_capabilities(ProviderCapabilities::minimal())
    }

    fn register(&self) -> Result<RegistrationMaterial, SystemError> {
        Err(unavailable("No native push provider is installed"))
    }

    fn rotate(&self) -> Result<RegistrationMaterial, SystemError> {
        Err(unavailable("No native push provider is installed"))
    }

    fn revoke(&self, _fingerprint: &TokenFingerprint) -> Result<ProviderRevocation, SystemError> {
        Ok(ProviderRevocation::Unsupported)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Deterministic device token bytes for one registration counter.
pub fn fake_token(provider_id: &str, counter: u64) -> Vec<u8> {
    format!("fake-device-token/{provider_id}/{counter}").into_bytes()
}

/// Deterministic Web Push endpoint capability URL for one registration counter.
pub fn fake_endpoint(provider_id: &str, counter: u64) -> Vec<u8> {
    format!("https://push.example.test/{provider_id}/{counter}").into_bytes()
}

/// Deterministic Web Push `p256dh` bytes for one registration counter.
pub fn fake_p256dh(counter: u64) -> Vec<u8> {
    format!("fake-p256dh-{counter}").into_bytes()
}

/// Deterministic Web Push `auth` bytes for one registration counter.
pub fn fake_auth(counter: u64) -> Vec<u8> {
    format!("fake-auth-{counter}").into_bytes()
}

struct FakeState {
    descriptor: PushProviderDescriptor,
    registrations: u64,
    rotations: u64,
    revocations: Vec<TokenFingerprint>,
    queued_frames: VecDeque<IncomingPushFrame>,
    fail_next_register: bool,
    hold_rotation: bool,
}

/// Deterministic transport provider for contract tests.
///
/// It supports multi-provider discovery, custom channels, generated and
/// provider-initiated token material, outage simulation and recovery, duplicate
/// and expired frames and every [`AppDeliveryCondition`](crate::AppDeliveryCondition).
#[derive(Clone)]
pub struct FakePushProvider {
    state: Arc<Mutex<FakeState>>,
}

impl FakePushProvider {
    pub fn new(descriptor: PushProviderDescriptor) -> Self {
        let descriptor = if descriptor.revision == 0 {
            descriptor.with_revision(1)
        } else {
            descriptor
        };
        Self {
            state: Arc::new(Mutex::new(FakeState {
                descriptor,
                registrations: 0,
                rotations: 0,
                revocations: Vec::new(),
                queued_frames: VecDeque::new(),
                fail_next_register: false,
                hold_rotation: false,
            })),
        }
    }

    /// A ready, configured provider for the given kind and identity.
    pub fn ready(kind: PushProviderKind, provider_id: impl Into<String>) -> Self {
        Self::new(
            PushProviderDescriptor::new(kind, provider_id)
                .with_configuration(ProviderConfiguration::Ready)
                .with_health(ProviderHealth::Ready)
                .with_capabilities(if kind.is_web_push() {
                    ProviderCapabilities::web_push()
                } else {
                    ProviderCapabilities::full()
                }),
        )
    }

    /// A ready Web Push provider on a custom channel.
    pub fn web_push(provider_id: impl Into<String>) -> Self {
        Self::ready(PushProviderKind::WebPush, provider_id)
    }

    pub fn descriptor(&self) -> PushProviderDescriptor {
        lock(&self.state).descriptor.clone()
    }

    pub fn set_descriptor(&self, descriptor: PushProviderDescriptor) {
        lock(&self.state).descriptor = descriptor;
    }

    pub fn set_health(&self, health: ProviderHealth) {
        lock(&self.state).descriptor.health = health;
    }

    pub fn simulate_outage(&self) {
        self.set_health(ProviderHealth::Outage);
    }

    pub fn recover(&self) {
        self.set_health(ProviderHealth::Ready);
    }

    pub fn mark_unconfigured(&self) {
        lock(&self.state).descriptor.configuration = ProviderConfiguration::NotConfigured;
    }

    pub fn mark_configured(&self) {
        lock(&self.state).descriptor.configuration = ProviderConfiguration::Ready;
    }

    pub fn fail_next_register(&self) {
        lock(&self.state).fail_next_register = true;
    }

    /// Make the next rotation fail, exercising retryable rotation paths.
    pub fn hold_next_rotation(&self) {
        lock(&self.state).hold_rotation = true;
    }

    pub fn registration_count(&self) -> u64 {
        lock(&self.state).registrations
    }

    pub fn rotation_count(&self) -> u64 {
        lock(&self.state).rotations
    }

    pub fn revocations(&self) -> Vec<TokenFingerprint> {
        lock(&self.state).revocations.clone()
    }

    /// Queue one transport frame for a host-driven test.
    pub fn queue_frame(&self, frame: IncomingPushFrame) {
        lock(&self.state).queued_frames.push_back(frame);
    }

    /// Queue the same frame twice, exercising bounded deduplication.
    pub fn queue_duplicate(&self, frame: IncomingPushFrame) {
        let mut state = lock(&self.state);
        state.queued_frames.push_back(frame.clone());
        state.queued_frames.push_back(frame);
    }

    /// Queue a frame whose expiry has already elapsed at `now_ms`.
    pub fn queue_expired(&self, frame: IncomingPushFrame, now_ms: u64) {
        self.queue_frame(frame.with_expiry(now_ms));
    }

    pub fn pending_frame_count(&self) -> usize {
        lock(&self.state).queued_frames.len()
    }

    pub fn take_frame(&self) -> Option<IncomingPushFrame> {
        lock(&self.state).queued_frames.pop_front()
    }

    pub fn deliver_all(&self) -> Vec<IncomingPushFrame> {
        let mut state = lock(&self.state);
        state.queued_frames.drain(..).collect()
    }

    fn material(&self, counter: u64, web_push: bool) -> Result<RegistrationMaterial, SystemError> {
        let provider_id = { lock(&self.state).descriptor.provider_id.clone() };
        if web_push {
            let endpoint = fake_endpoint(&provider_id, counter);
            let material = WebPushRegistrationMaterial::new(
                endpoint,
                fake_p256dh(counter),
                fake_auth(counter),
            )?;
            RegistrationMaterial::new(None, Some(material))
        } else {
            RegistrationMaterial::new(Some(fake_token(&provider_id, counter)), None)
        }
    }
}

impl PushProvider for FakePushProvider {
    fn descriptor(&self) -> PushProviderDescriptor {
        lock(&self.state).descriptor.clone()
    }

    fn register(&self) -> Result<RegistrationMaterial, SystemError> {
        let (counter, web_push) = {
            let mut state = lock(&self.state);
            if !state.descriptor.configuration.is_ready() {
                return Err(unavailable("Push provider is not configured"));
            }
            if !state.descriptor.health.is_selectable() {
                return Err(outage("Push provider is in outage or unavailable"));
            }
            if state.fail_next_register {
                state.fail_next_register = false;
                return Err(provider_failure());
            }
            state.registrations = state.registrations.saturating_add(1);
            (state.registrations, state.descriptor.capabilities.web_push)
        };
        self.material(counter, web_push)
    }

    fn rotate(&self) -> Result<RegistrationMaterial, SystemError> {
        let (counter, web_push) = {
            let mut state = lock(&self.state);
            if !state.descriptor.configuration.is_ready() {
                return Err(unavailable("Push provider is not configured"));
            }
            if !state.descriptor.health.is_selectable() {
                return Err(outage("Push provider is in outage or unavailable"));
            }
            if state.hold_rotation {
                state.hold_rotation = false;
                return Err(provider_failure());
            }
            if !state.descriptor.capabilities.token_rotation {
                return Err(SystemError::new(
                    ErrorCode::Unsupported,
                    "Push provider does not support token rotation",
                ));
            }
            state.rotations = state.rotations.saturating_add(1);
            (
                state.registrations.saturating_add(state.rotations),
                state.descriptor.capabilities.web_push,
            )
        };
        self.material(counter, web_push)
    }

    fn revoke(&self, fingerprint: &TokenFingerprint) -> Result<ProviderRevocation, SystemError> {
        let mut state = lock(&self.state);
        state.revocations.push(fingerprint.clone());
        if !state.descriptor.health.is_selectable() {
            return Ok(ProviderRevocation::Unreachable);
        }
        if !state.descriptor.capabilities.provider_revocation {
            return Ok(ProviderRevocation::Unsupported);
        }
        Ok(ProviderRevocation::Revoked)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for FakePushProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = lock(&self.state);
        formatter
            .debug_struct("FakePushProvider")
            .field("descriptor", &state.descriptor)
            .field("registrations", &state.registrations)
            .field("rotations", &state.rotations)
            .field("queued_frames", &state.queued_frames.len())
            .finish()
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
