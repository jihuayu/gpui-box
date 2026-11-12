//! Remote push providers, device registration and bounded message routing.
//!
//! The default build has no GPUI dependency and installs no native push
//! provider. Every row in the platform matrix is [`Evidence::ContractOnly`];
//! `Support::Native` or `Support::Conditional` describes the intended host
//! mechanism, not a compiled or runtime-qualified implementation.
//!
//! Two execution domains are deliberately distinct:
//!
//! - Device registration, token rotation, account binding, logout, credential
//!   leases and every durable read/write require a background
//!   [`OperationContext`] with no UI attachment. UI and renderer owners fail
//!   before any side effect.
//! - Local presentation and activation handoff cross explicit traits
//!   ([`LocalNotificationPresenter`] and [`ActivationHandoffSink`]) that a host
//!   adapter implements. Raw device tokens, Web Push endpoint URLs, `p256dh`
//!   keys and `auth` secrets exist only inside `gpui-secrets` under the
//!   [`PUSH_SECRET_SERVICE`] namespace.
//!
//! Message routing is a four-stage pipeline:
//!
//! 1. [`PushService::provider_handoff`] rejects oversize, expired, duplicate,
//!    unknown-provider and stale-account frames.
//! 2. [`PushService::accept_message`] records a [`MessageReceipt`] and a bounded
//!    background-work handoff for `gpui-jobs` (#54).
//! 3. [`PushService::present_message`] evaluates the foreground policy and the
//!    `gpui-permissions` notification status before calling a presenter.
//! 4. [`PushService::activate_notification`] routes a user-initiated activation
//!    through the activation boundary. It never runs during Stages 1-3.
//!
//! See `docs/S_REMOTE_PUSH.md`.

#![forbid(unsafe_code)]

mod clock;
mod dedup;
mod model;
mod platform;
mod provider;
mod secrets;
mod service;
mod store;
mod validation;

pub use clock::{ManualPushClock, PushClock, SystemPushClock};
pub use dedup::{DedupEntry, DedupJournal};
pub use model::*;
pub use platform::{
    PushOperation, PushOperationSupport, PushPlatform, WebPushBackendMaturity,
    WebPushCapabilityPolicy, operation_support, platform_operations, platform_support_matrix,
    web_push_capability_policy,
};
pub use provider::{
    FakePushProvider, ProviderRevocation, PushProvider, RegistrationMaterial,
    UnavailablePushProvider, UnavailableRegistrationMaterial, WebPushRegistrationMaterial,
    fake_auth, fake_endpoint, fake_p256dh, fake_token, push_provider_available,
};
pub use secrets::{
    MemoryCredentials, PUSH_PURPOSE_DEVICE_TOKEN, PUSH_PURPOSE_WEB_PUSH_AUTH,
    PUSH_PURPOSE_WEB_PUSH_ENDPOINT, PUSH_PURPOSE_WEB_PUSH_P256DH, PUSH_SECRET_PURPOSES,
    PUSH_SECRET_SERVICE, PushSecretVault, RegistrationCredentialLease, RegistrationSecretSnapshot,
    SecretVaultError, WebPushCredentials, secret_account_name,
};
pub use service::{
    ActivationHandoffSink, FixedPermissionProbe, LocalNotificationPresenter, LogoutReport,
    NoopActivationHandoffSink, PushConfig, PushPermissionProbe, PushPipelineOutcome, PushRuntime,
    PushService, RecordingActivationSink, RecordingPresenter, UnavailableActivationSink,
    UnavailablePresenter, UnregistrationReceipt, notification_permission,
};
pub use store::{
    LoadOutcome, MAX_JOURNAL_ENVELOPE_BYTES, MAX_QUARANTINE_ENTRIES, MAX_STATE_ENVELOPE_BYTES,
    PUSH_FORMAT_VERSION, PUSH_NAMESPACE, PushStore, QUARANTINE_DIR, QuarantinedPayload,
};
pub use validation::{
    MAX_ACTIVATION_ROUTE_BYTES, MAX_CLIENT_MATERIAL_BYTES, PushClientConfiguration,
    VAPID_PUBLIC_KEY_BYTES, WebPushHostState, reject_server_credentials, validate_activation_route,
    validate_vapid_public_key, validate_vapid_public_key_text,
};

pub use gpui_app_data::{AppData, DataLocation, DirectoryPolicy, DirectoryRoots};
pub use gpui_permissions::{PermissionKind, PermissionStatus};
pub use gpui_secrets::{CredentialKey, SecretAccess, SecretBytes, SecretPolicy, Secrets};
pub use gpui_system::{
    CancelReason, Completion, ErrorCode, Event, EventEnvelope, EventSender, EventSubscription,
    Evidence, OperationContext, PROVIDER_MANIFEST, ProviderManifest, RetryAdvice, ServiceResult,
    Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-push package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Which executor owns a call.
///
/// A UI attachment means `Ui`; no attachment means `Background`. `Renderer` is
/// an explicit class used by callers that drive render-thread work; it is never
/// permitted for credentials, durable state or any routing side effect. A
/// renderer cannot be inferred from an [`OperationContext`], so callers that
/// represent one declare it explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionOwner {
    Ui,
    Background,
    Renderer,
}

impl ExecutionOwner {
    /// Classify a context. Renderer ownership is never inferred.
    pub fn from_context(context: &OperationContext) -> Self {
        if context.ui.is_some() {
            Self::Ui
        } else {
            Self::Background
        }
    }

    pub const fn is_background(self) -> bool {
        matches!(self, Self::Background)
    }

    /// Credential, durable-state and routing operations require a background
    /// context with no UI attachment. UI and renderer ownership fail before any
    /// side effect, and an inconsistent `Ui`/`Renderer` owner carrying a UI
    /// attachment is rejected as well.
    pub fn require_background(self, context: &OperationContext) -> Result<(), SystemError> {
        if !self.is_background() || context.ui.is_some() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Push credential and routing operations require a background execution owner",
            ));
        }
        context.check_at(std::time::Instant::now())?;
        Ok(())
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Background => "background",
            Self::Renderer => "renderer",
        }
    }
}

impl std::fmt::Display for ExecutionOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
