//! Validated push identity, provider descriptions, registration state and the
//! bounded four-stage routing vocabulary.
//!
//! Public snapshots, receipts, events and errors never carry raw device tokens,
//! Web Push endpoint capability URLs, `p256dh` keys or `auth` secrets. Only a
//! SHA-256 [`TokenFingerprint`] prefix and non-secret metadata cross this module.

use crate::platform::{PushOperation, PushPlatform};
use gpui_permissions::PermissionStatus;
use gpui_system::{CancelReason, ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fmt};

/// Largest accepted push payload in bytes.
pub const MAX_PUSH_PAYLOAD_BYTES: usize = 4096;
/// Largest accepted provider, channel or provider-tag identifier.
pub const MAX_PUSH_PROVIDER_IDENTIFIER_BYTES: usize = 128;
/// Largest accepted account identifier. The extra room keeps the projected
/// `gpui-secrets` account name inside the shared 128-byte credential bound.
pub const MAX_PUSH_ACCOUNT_ID_BYTES: usize = 96;
/// Largest accepted topic identifier.
pub const MAX_PUSH_TOPIC_BYTES: usize = 96;
/// Largest accepted message identifier.
pub const MAX_PUSH_MESSAGE_ID_BYTES: usize = 128;
/// Largest accepted notification title.
pub const MAX_PUSH_TITLE_BYTES: usize = 256;
/// Largest accepted notification body.
pub const MAX_PUSH_BODY_BYTES: usize = 4096;
/// Default bounded message-ID journal capacity.
pub const DEFAULT_DEDUP_CAPACITY: usize = 512;
/// Hard bound on the message-ID journal capacity.
pub const MAX_DEDUP_CAPACITY: usize = 4096;
/// Default message-ID journal time-to-live.
pub const DEFAULT_DEDUP_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
/// Hard bound on the message-ID journal time-to-live (30 days).
pub const MAX_DEDUP_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
/// Default bounded background-work handoff queue depth.
pub const DEFAULT_PENDING_BACKGROUND_HANDOFFS: usize = 64;
/// Hard bound on the background-work handoff queue depth.
pub const MAX_PENDING_BACKGROUND_HANDOFFS: usize = 256;
/// Default bounded cold-launch activation queue depth.
pub const DEFAULT_PENDING_COLD_ACTIVATIONS: usize = 32;
/// Hard bound on the cold-launch activation queue depth.
pub const MAX_PENDING_COLD_ACTIVATIONS: usize = 128;
/// Default bounded push event stream capacity.
pub const DEFAULT_PUSH_EVENT_CAPACITY: usize = 64;
/// Default advisory background-work budget forwarded to `gpui-jobs`.
pub const DEFAULT_BACKGROUND_WORK_BUDGET_MS: u64 = 30_000;
/// Hard bound on the advisory background-work budget (ten minutes).
pub const MAX_BACKGROUND_WORK_BUDGET_MS: u64 = 10 * 60 * 1_000;
/// Default number of accepted messages retained for presentation/activation.
pub const DEFAULT_RECENT_MESSAGE_CAPACITY: usize = 256;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn identifier(value: &str, max: usize, extra: &[u8]) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && !value.chars().any(char::is_control)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || extra.contains(&byte)
        })
}

/// The transport family a push provider belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushProviderKind {
    /// Apple Push Notification service.
    Apns,
    /// Firebase Cloud Messaging.
    Fcm,
    /// Windows Notification Service.
    Wns,
    /// A Linux desktop transport such as a D-Bus notification daemon bridge.
    LinuxTransport,
    /// Standards-based Web Push over VAPID.
    WebPush,
    /// A host-defined channel that is not one of the named transports.
    CustomChannel,
}

impl PushProviderKind {
    pub const ALL: [Self; 6] = [
        Self::Apns,
        Self::Fcm,
        Self::Wns,
        Self::LinuxTransport,
        Self::WebPush,
        Self::CustomChannel,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apns => "apns",
            Self::Fcm => "fcm",
            Self::Wns => "wns",
            Self::LinuxTransport => "linux_transport",
            Self::WebPush => "web_push",
            Self::CustomChannel => "custom_channel",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Apns => "APNs",
            Self::Fcm => "FCM",
            Self::Wns => "WNS",
            Self::LinuxTransport => "LinuxTransport",
            Self::WebPush => "WebPush",
            Self::CustomChannel => "CustomChannel",
        }
    }

    pub const fn is_web_push(self) -> bool {
        matches!(self, Self::WebPush)
    }

    /// True when the transport can carry a standards-based Web Push
    /// subscription (endpoint capability URL plus `p256dh`/`auth` keys).
    pub const fn carries_web_push_credentials(self) -> bool {
        matches!(self, Self::WebPush | Self::CustomChannel)
    }
}

impl fmt::Display for PushProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A stable, opaque push account identifier.
///
/// The accepted alphabet matches `gpui-auth`'s account identifier so a host can
/// pass the same opaque value through `gpui-auth` (#46) without translation. It
/// must never be an email address, display name or other personally identifying
/// value.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct PushAccountId(String);

impl PushAccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_PUSH_ACCOUNT_ID_BYTES, b":@+=!#%") {
            return Err(invalid(
                "Push account identity must be opaque, bounded ASCII",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PushAccountId {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Debug for PushAccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PushAccountId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for PushAccountId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated push topic name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct PushTopic(String);

impl PushTopic {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_PUSH_TOPIC_BYTES, b":/@+=") {
            return Err(invalid("Push topic must be bounded ASCII"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PushTopic {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Debug for PushTopic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("PushTopic").field(&self.0).finish()
    }
}

impl fmt::Display for PushTopic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A validated provider message identity. Deduplication is keyed on this value
/// alone, independent of the payload or the receiving provider.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct PushMessageId(String);

impl PushMessageId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value, MAX_PUSH_MESSAGE_ID_BYTES, b":@+=%!#/") {
            return Err(invalid("Push message identity must be bounded ASCII"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PushMessageId {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Debug for PushMessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PushMessageId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for PushMessageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A truncated SHA-256 digest of raw registration material.
///
/// This is the only representation of a device token that may appear in a
/// receipt, event, snapshot or durable record. It is a correlation prefix, not a
/// reversible encoding.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct TokenFingerprint(String);

/// Number of hex characters retained from the SHA-256 digest.
pub const TOKEN_FINGERPRINT_HEX_CHARS: usize = 16;

impl TokenFingerprint {
    /// Fingerprint raw registration material without retaining or logging it.
    pub fn of(material: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"gpui-push-token\0");
        hasher.update(material);
        let digest = hasher.finalize();
        let mut encoded = String::with_capacity(TOKEN_FINGERPRINT_HEX_CHARS);
        for byte in digest.iter().take(TOKEN_FINGERPRINT_HEX_CHARS / 2) {
            encoded.push_str(&format!("{byte:02x}"));
        }
        Self(encoded)
    }

    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if value.len() != TOKEN_FINGERPRINT_HEX_CHARS
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(invalid(
                "Token fingerprint must be exactly 16 lowercase hex characters",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TokenFingerprint {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Debug for TokenFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("TokenFingerprint")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for TokenFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Point-in-time provider health. Discovery never synthesizes `Ready`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealth {
    Ready,
    Degraded,
    Outage,
    Unavailable,
}

impl ProviderHealth {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Degraded => "degraded",
            Self::Outage => "outage",
            Self::Unavailable => "unavailable",
        }
    }

    /// Degraded providers remain selectable; an outage does not.
    pub const fn is_selectable(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded)
    }

    pub const fn is_outage(self) -> bool {
        matches!(self, Self::Outage)
    }
}

impl fmt::Display for ProviderHealth {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Whether the host supplied the configuration a provider needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConfiguration {
    Ready,
    NotConfigured,
}

impl ProviderConfiguration {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Capabilities a provider claims for its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapabilities {
    pub token_registration: bool,
    pub token_rotation: bool,
    pub provider_revocation: bool,
    pub silent_push: bool,
    pub notification_push: bool,
    pub web_push: bool,
}

impl ProviderCapabilities {
    pub const fn full() -> Self {
        Self {
            token_registration: true,
            token_rotation: true,
            provider_revocation: true,
            silent_push: true,
            notification_push: true,
            web_push: false,
        }
    }

    pub const fn web_push() -> Self {
        Self {
            token_registration: true,
            token_rotation: true,
            provider_revocation: true,
            silent_push: true,
            notification_push: true,
            web_push: true,
        }
    }

    pub const fn minimal() -> Self {
        Self {
            token_registration: true,
            token_rotation: false,
            provider_revocation: false,
            silent_push: true,
            notification_push: false,
            web_push: false,
        }
    }

    /// True when the provider can satisfy the payload kind at all.
    pub const fn supports_payload(self, payload: PushPayloadKind) -> bool {
        match payload {
            PushPayloadKind::SilentDataOnly => self.silent_push,
            PushPayloadKind::NotificationAndData => self.notification_push,
        }
    }
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self::minimal()
    }
}

/// One discovered provider/channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushProviderDescriptor {
    pub provider_id: String,
    pub channel_id: Option<String>,
    pub kind: PushProviderKind,
    pub configuration: ProviderConfiguration,
    pub health: ProviderHealth,
    pub capabilities: ProviderCapabilities,
    pub revision: u64,
}

impl PushProviderDescriptor {
    pub fn new(kind: PushProviderKind, provider_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            channel_id: None,
            kind,
            configuration: ProviderConfiguration::NotConfigured,
            health: ProviderHealth::Unavailable,
            capabilities: ProviderCapabilities::minimal(),
            revision: 0,
        }
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    pub fn with_configuration(mut self, configuration: ProviderConfiguration) -> Self {
        self.configuration = configuration;
        self
    }

    pub fn with_health(mut self, health: ProviderHealth) -> Self {
        self.health = health;
        self
    }

    pub fn with_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = revision;
        self
    }

    /// A configured, reachable provider. Discovery still re-checks at call time.
    pub const fn is_selectable(&self) -> bool {
        self.configuration.is_ready() && self.health.is_selectable()
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if !identifier(
            &self.provider_id,
            MAX_PUSH_PROVIDER_IDENTIFIER_BYTES,
            b":/@",
        ) {
            return Err(invalid(
                "Push provider identity must be bounded ASCII without control characters",
            ));
        }
        if let Some(channel) = &self.channel_id
            && !identifier(channel, MAX_PUSH_PROVIDER_IDENTIFIER_BYTES, b":@")
        {
            return Err(invalid(
                "Push channel identity must be bounded ASCII without control characters",
            ));
        }
        if self.capabilities.web_push && !self.kind.carries_web_push_credentials() {
            return Err(invalid(
                "Only Web Push and custom channels may claim Web Push credentials",
            ));
        }
        Ok(())
    }

    /// Stable `provider[/channel]` selection key used by an explicit host policy.
    pub fn selection_key(&self) -> String {
        match &self.channel_id {
            Some(channel) => format!("{}/{channel}", self.provider_id),
            None => self.provider_id.clone(),
        }
    }
}

/// An explicit host selection policy. Without one, more than one usable
/// candidate is an ambiguity error rather than a silent first-match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProviderSelectionPolicy {
    /// The host names the exact provider and optional channel. A missing,
    /// unconfigured, unavailable or outage provider fails closed.
    HostSelected {
        provider_id: String,
        channel_id: Option<String>,
    },
    /// Exactly one configured candidate is acceptable; zero or many fail closed.
    #[default]
    SoleCandidate,
}

/// Whether a device token still needs to be uploaded to the application server.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationSyncState {
    /// No device is registered for this profile.
    #[default]
    Unregistered,
    /// The active `token_revision` must be acknowledged by the host server.
    PendingServerSync { revision: u64 },
    /// The host server acknowledged exactly this `token_revision`.
    SyncedAtRevision { revision: u64 },
}

impl RegistrationSyncState {
    pub const fn is_registered(self) -> bool {
        !matches!(self, Self::Unregistered)
    }

    pub const fn pending_revision(self) -> Option<u64> {
        match self {
            Self::PendingServerSync { revision } => Some(revision),
            _ => None,
        }
    }

    pub const fn synced_revision(self) -> Option<u64> {
        match self {
            Self::SyncedAtRevision { revision } => Some(revision),
            _ => None,
        }
    }
}

/// Why a new token was minted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenRotationTrigger {
    /// An explicit caller request, typically after a failed upload.
    CallerRequested,
    /// The provider invalidated the previous token and pushed a new one.
    ProviderNotice,
}

impl TokenRotationTrigger {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CallerRequested => "caller_requested",
            Self::ProviderNotice => "provider_notice",
        }
    }
}

impl fmt::Display for TokenRotationTrigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Who owns a topic subscription.
///
/// `Device` subscriptions survive an account switch; `Account` subscriptions
/// are revoked with the account's epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionOwner {
    Device,
    Account(PushAccountId),
}

impl SubscriptionOwner {
    pub const fn is_device(&self) -> bool {
        matches!(self, Self::Device)
    }

    pub fn account(&self) -> Option<&PushAccountId> {
        match self {
            Self::Device => None,
            Self::Account(account) => Some(account),
        }
    }
}

/// One topic subscription tagged with the registration epoch that owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushSubscription {
    pub topic: PushTopic,
    pub owner: SubscriptionOwner,
    pub registration_epoch: u64,
}

impl PushSubscription {
    pub fn new(topic: PushTopic, owner: SubscriptionOwner, registration_epoch: u64) -> Self {
        Self {
            topic,
            owner,
            registration_epoch,
        }
    }
}

/// Why an account-scoped secret cleanup could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretCleanupBlocker {
    /// The device vault is locked; unlock and retry.
    Locked,
    /// The vault key was invalidated; authenticate again before retrying.
    KeyInvalidated,
}

impl SecretCleanupBlocker {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::KeyInvalidated => "key_invalidated",
        }
    }

    /// The typed, retryable error a logout caller receives.
    pub fn error(self) -> SystemError {
        let message = match self {
            Self::Locked => "Push secret cleanup is pending: the vault is locked",
            Self::KeyInvalidated => "Push secret cleanup is pending: the vault key was invalidated",
        };
        let mut error = SystemError::new(ErrorCode::Unavailable, message);
        error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
        error.recovery = Some(gpui_system::RecoveryAction::Authenticate);
        error
    }
}

/// The durable state of an account logout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TombstoneState {
    /// The logout was accepted and secret deletion is in flight.
    LoggingOut,
    /// Secret deletion did not finish; the namespace must be retried.
    PendingSecretCleanup,
}

/// A durable per-account logout tombstone. While present it blocks token leases,
/// account reuse and incoming routing for that account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountTombstone {
    pub account: PushAccountId,
    pub registration_epoch: u64,
    pub state: TombstoneState,
    /// True when device-scope secrets must also be removed.
    pub device_scope: bool,
    pub recorded_at_ms: u64,
}

/// How the host woke this process for a push.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppDeliveryCondition {
    ForegroundActive,
    Background,
    TerminatedColdLaunch,
}

impl AppDeliveryCondition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ForegroundActive => "foreground_active",
            Self::Background => "background",
            Self::TerminatedColdLaunch => "terminated_cold_launch",
        }
    }

    /// True when the process was resumed solely for the push.
    pub const fn is_cold_launch(self) -> bool {
        matches!(self, Self::TerminatedColdLaunch)
    }

    /// True when a background work handoff is appropriate.
    pub const fn requires_background_work(self) -> bool {
        !matches!(self, Self::ForegroundActive)
    }
}

impl fmt::Display for AppDeliveryCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The payload shape a push carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushPayloadKind {
    /// A user-visible notification plus optional data.
    NotificationAndData,
    /// Data only; never user-visible on its own.
    SilentDataOnly,
}

impl PushPayloadKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotificationAndData => "notification_and_data",
            Self::SilentDataOnly => "silent_data_only",
        }
    }

    pub const fn is_silent(self) -> bool {
        matches!(self, Self::SilentDataOnly)
    }
}

impl fmt::Display for PushPayloadKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What a foreground application does with a notification-and-data push.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundPresentationPolicy {
    /// Do not present while the app is foreground-active.
    SuppressInForeground,
    /// Present a local banner through the presenter boundary.
    #[default]
    PresentBanner,
    /// Hand presentation to a host-owned surface without calling a presenter.
    DelegateToHost,
}

/// Bounded, non-secret notification content used for local presentation.
#[derive(Clone, PartialEq, Eq)]
pub struct PushNotificationSummary {
    title: String,
    body: String,
}

impl PushNotificationSummary {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Result<Self, SystemError> {
        let title = title.into();
        let body = body.into();
        if title.is_empty()
            || title.len() > MAX_PUSH_TITLE_BYTES
            || title.trim() != title
            || title.chars().any(char::is_control)
        {
            return Err(invalid(
                "Push notification title is empty, oversized or invalid",
            ));
        }
        if body.len() > MAX_PUSH_BODY_BYTES {
            return Err(invalid("Push notification body is oversized"));
        }
        Ok(Self { title, body })
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> &str {
        &self.body
    }
}

impl fmt::Debug for PushNotificationSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushNotificationSummary")
            .field("title_bytes", &self.title.len())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// One incoming transport frame before any policy is applied.
///
/// Debug deliberately reports the payload length only: payload bytes may contain
/// account-scoped data and are never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct IncomingPushFrame {
    pub message_id: PushMessageId,
    pub provider_id: String,
    pub channel_id: Option<String>,
    pub kind: PushProviderKind,
    pub target_account: Option<PushAccountId>,
    pub registration_epoch: Option<u64>,
    pub condition: AppDeliveryCondition,
    pub payload_kind: PushPayloadKind,
    pub payload: Vec<u8>,
    pub notification: Option<PushNotificationSummary>,
    pub expires_at_ms: Option<u64>,
    pub activation_route: Option<String>,
}

impl IncomingPushFrame {
    pub fn new(
        message_id: PushMessageId,
        kind: PushProviderKind,
        provider_id: impl Into<String>,
        condition: AppDeliveryCondition,
        payload_kind: PushPayloadKind,
    ) -> Self {
        Self {
            message_id,
            provider_id: provider_id.into(),
            channel_id: None,
            kind,
            target_account: None,
            registration_epoch: None,
            condition,
            payload_kind,
            payload: Vec::new(),
            notification: None,
            expires_at_ms: None,
            activation_route: None,
        }
    }

    pub fn with_channel_id(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = Some(channel_id.into());
        self
    }

    pub fn with_target_account(mut self, account: PushAccountId, registration_epoch: u64) -> Self {
        self.target_account = Some(account);
        self.registration_epoch = Some(registration_epoch);
        self
    }

    pub fn with_registration_epoch(mut self, registration_epoch: u64) -> Self {
        self.registration_epoch = Some(registration_epoch);
        self
    }

    pub fn with_payload(mut self, payload: impl Into<Vec<u8>>) -> Self {
        self.payload = payload.into();
        self
    }

    pub fn with_notification(mut self, notification: PushNotificationSummary) -> Self {
        self.notification = Some(notification);
        self
    }

    pub fn with_expiry(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = Some(expires_at_ms);
        self
    }

    pub fn with_activation_route(mut self, route: impl Into<String>) -> Self {
        self.activation_route = Some(route.into());
        self
    }

    pub fn payload_len(&self) -> usize {
        self.payload.len()
    }
}

impl fmt::Debug for IncomingPushFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("IncomingPushFrame")
            .field("message_id", &self.message_id)
            .field("kind", &self.kind)
            .field("provider_id", &self.provider_id)
            .field("condition", &self.condition)
            .field("payload_kind", &self.payload_kind)
            .field("payload_bytes", &self.payload.len())
            .field("target_account", &self.target_account)
            .field("registration_epoch", &self.registration_epoch)
            .field("expires_at_ms", &self.expires_at_ms)
            .field("has_activation_route", &self.activation_route.is_some())
            .finish()
    }
}

/// Stage 1 outcome. Only [`Self::Accepted`] may create a Stage 2 receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderHandoffOutcome {
    Accepted,
    /// The frame's expiry elapsed before it was routed.
    ExpiredDrop,
    /// The payload exceeded [`MAX_PUSH_PAYLOAD_BYTES`].
    OversizePayload {
        bytes: usize,
        limit: usize,
    },
    /// No discovered provider/channel matches the frame.
    UnknownProvider,
    /// This device has no registration or a device-scoped secret cleanup is
    /// still pending. Frames are dropped rather than attached to nothing.
    DeviceNotRegistered,
    /// The provider's health or configuration forbids accepting the frame.
    ProviderNotReady {
        health: ProviderHealth,
    },
    /// The provider does not carry this payload kind.
    UnsupportedPayloadKind,
    /// A notification-and-data frame carried no notification summary.
    MissingNotificationContent,
    /// The target account is logged out, tombstoned or not the active account.
    AccountMismatchOrLoggedOut,
    /// The frame's epoch does not match the active registration epoch.
    StaleRegistrationEpoch {
        expected: u64,
        received: u64,
    },
    /// The activation route failed bounded deep-link validation.
    InvalidActivationRoute,
    /// A recorded message with this identity suppressed the frame.
    DuplicateSuppressed {
        first_seen_at_ms: u64,
    },
}

impl ProviderHandoffOutcome {
    pub const fn is_accepted(&self) -> bool {
        matches!(self, Self::Accepted)
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::ExpiredDrop => "expired_drop",
            Self::OversizePayload { .. } => "oversize_payload",
            Self::UnknownProvider => "unknown_provider",
            Self::DeviceNotRegistered => "device_not_registered",
            Self::ProviderNotReady { .. } => "provider_not_ready",
            Self::UnsupportedPayloadKind => "unsupported_payload_kind",
            Self::MissingNotificationContent => "missing_notification_content",
            Self::AccountMismatchOrLoggedOut => "account_mismatch_or_logged_out",
            Self::StaleRegistrationEpoch { .. } => "stale_registration_epoch",
            Self::InvalidActivationRoute => "invalid_activation_route",
            Self::DuplicateSuppressed { .. } => "duplicate_suppressed",
        }
    }
}

/// Result of Stage 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHandoff {
    pub message_id: PushMessageId,
    pub kind: PushProviderKind,
    pub outcome: ProviderHandoffOutcome,
}

impl ProviderHandoff {
    pub fn new(
        message_id: PushMessageId,
        kind: PushProviderKind,
        outcome: ProviderHandoffOutcome,
    ) -> Self {
        Self {
            message_id,
            kind,
            outcome,
        }
    }

    pub const fn is_accepted(&self) -> bool {
        self.outcome.is_accepted()
    }
}

/// Which bounded `gpui-jobs` pool a background handoff belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundWorkClass {
    /// Silent data work that may run opportunistically.
    SilentData,
    /// Data work that accompanies a visible notification.
    NotificationData,
}

/// An advisory scheduling hint, never a reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundWorkRecommendation {
    /// Submit as soon as constraints allow.
    RunSoon,
    /// Defer until the app is next foreground-active.
    Defer,
}

/// Advisory background work budget forwarded to `gpui-jobs` (#54).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackgroundWorkBudget {
    /// Advisory execution budget in milliseconds.
    pub advisory_budget_ms: u64,
    /// Advisory lower bound before the work should start.
    pub earliest_after_ms: u64,
    /// The work may be skipped entirely under power or thermal pressure.
    pub skip_allowed: bool,
}

impl BackgroundWorkBudget {
    pub fn new(advisory_budget_ms: u64) -> Result<Self, SystemError> {
        if advisory_budget_ms == 0 || advisory_budget_ms > MAX_BACKGROUND_WORK_BUDGET_MS {
            return Err(invalid(
                "Background work advisory budget is outside its supported bounds",
            ));
        }
        Ok(Self {
            advisory_budget_ms,
            earliest_after_ms: 0,
            skip_allowed: true,
        })
    }
}

impl Default for BackgroundWorkBudget {
    fn default() -> Self {
        Self {
            advisory_budget_ms: DEFAULT_BACKGROUND_WORK_BUDGET_MS,
            earliest_after_ms: 0,
            skip_allowed: true,
        }
    }
}

/// Bounded handoff to the host's `gpui-jobs` coordinator. It never spawns work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundWorkHandoff {
    pub message_id: PushMessageId,
    pub account: Option<PushAccountId>,
    pub registration_epoch: u64,
    pub class: BackgroundWorkClass,
    pub recommendation: BackgroundWorkRecommendation,
    pub budget: BackgroundWorkBudget,
    pub payload_bytes: usize,
}

/// Stage 2 receipt. Constructed only after Stage 1 accepts a unique message and
/// records its identity in the durable dedup journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageReceipt {
    pub message_id: PushMessageId,
    pub kind: PushProviderKind,
    pub condition: AppDeliveryCondition,
    pub payload_kind: PushPayloadKind,
    pub account: Option<PushAccountId>,
    pub registration_epoch: u64,
    pub received_at_ms: u64,
    pub payload_bytes: usize,
    pub background_handoff: Option<BackgroundWorkHandoff>,
}

impl MessageReceipt {
    /// True when a `SilentDataOnly` or non-foreground delivery produced work.
    pub const fn has_background_work(&self) -> bool {
        self.background_handoff.is_some()
    }
}

/// Stage 3 outcome. Strictly separate from the Stage 2 receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalPresentationOutcome {
    /// The payload was silent; presentation does not apply.
    NotApplicableSilentPush,
    /// The foreground policy asked the app not to present.
    SuppressedForegroundPolicy,
    /// The `gpui-permissions` status forbids presentation. The Stage 2 receipt
    /// was still recorded; no presenter was called.
    SuppressedPermissionDenied { status: PermissionStatus },
    /// A presenter accepted the local notification.
    Presented { notification_id: String },
    /// Presentation was handed to a host-owned surface.
    DelegatedToHost,
    /// The presenter returned a typed failure.
    PresenterError { code: ErrorCode },
    /// No accepted Stage 2 message exists for this identity.
    NotAcceptedMessage,
    /// The registration was cleared, switched or tombstoned before this delayed
    /// presentation ran, so the stale notification is not displayed.
    SuppressedRegistrationChanged,
}

impl LocalPresentationOutcome {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NotApplicableSilentPush => "not_applicable_silent_push",
            Self::SuppressedForegroundPolicy => "suppressed_foreground_policy",
            Self::SuppressedPermissionDenied { .. } => "suppressed_permission_denied",
            Self::Presented { .. } => "presented",
            Self::DelegatedToHost => "delegated_to_host",
            Self::PresenterError { .. } => "presenter_error",
            Self::NotAcceptedMessage => "not_accepted_message",
            Self::SuppressedRegistrationChanged => "suppressed_registration_changed",
        }
    }

    /// True when the user could have seen a notification.
    pub const fn was_presented(&self) -> bool {
        matches!(self, Self::Presented { .. })
    }
}

/// Bounded notification content inside the presenter boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalNotification {
    pub message_id: PushMessageId,
    pub title: String,
    pub body: String,
    pub account: Option<PushAccountId>,
}

/// What a presenter boundary reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalPresentationReceipt {
    /// The platform accepted a submission; this is not proof of display.
    Presented { notification_id: String },
    /// A host-owned surface owns visible presentation.
    DelegatedToHost,
}

/// Stage 4 outcome. Activation never runs during Stages 1-3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushActivationOutcome {
    /// A validated activation envelope crossed the handoff sink.
    Routed,
    /// The cold-launch activation was retained until a UI surface attaches.
    QueuedForColdLaunch { depth: usize },
    /// The activation targeted a logged-out, tombstoned or switched-away account.
    RejectedAccountMismatch,
    /// The activation's epoch is stale.
    RejectedStaleEpoch { expected: u64, received: u64 },
    /// The activation carried no route.
    RejectedMissingRoute,
    /// The route exceeded [`MAX_ACTIVATION_ROUTE_BYTES`] or failed validation.
    RejectedInvalidRoute,
    /// No accepted message or queued activation matches this identity.
    RejectedUnknownMessage,
    /// The same message identity was already activated.
    RejectedDuplicateActivation,
    /// The device is unregistered or a device-scoped cleanup is pending.
    RejectedNotRegistered,
    /// The handoff sink returned a typed failure.
    HandoffFailed { code: ErrorCode },
    /// The message was a silent push and carries no user activation.
    RejectedSilentMessage,
}

impl PushActivationOutcome {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Routed => "routed",
            Self::QueuedForColdLaunch { .. } => "queued_for_cold_launch",
            Self::RejectedAccountMismatch => "rejected_account_mismatch",
            Self::RejectedStaleEpoch { .. } => "rejected_stale_epoch",
            Self::RejectedMissingRoute => "rejected_missing_route",
            Self::RejectedInvalidRoute => "rejected_invalid_route",
            Self::RejectedUnknownMessage => "rejected_unknown_message",
            Self::RejectedDuplicateActivation => "rejected_duplicate_activation",
            Self::RejectedNotRegistered => "rejected_not_registered",
            Self::HandoffFailed { .. } => "handoff_failed",
            Self::RejectedSilentMessage => "rejected_silent_message",
        }
    }

    pub const fn is_routed(&self) -> bool {
        matches!(self, Self::Routed)
    }
}

/// A retained cold-launch activation that has not crossed a handoff sink yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingActivation {
    pub sequence: u64,
    pub message_id: PushMessageId,
    pub route: String,
    pub account: Option<PushAccountId>,
    pub registration_epoch: u64,
    pub condition: AppDeliveryCondition,
    pub queued_at_ms: u64,
}

/// The validated activation envelope a host maps to its own activation model.
#[derive(Clone, PartialEq, Eq)]
pub struct PushActivationEnvelope {
    pub message_id: PushMessageId,
    pub route: String,
    pub account: Option<PushAccountId>,
    pub registration_epoch: u64,
    pub condition: AppDeliveryCondition,
}

impl PushActivationEnvelope {
    pub fn route(&self) -> &str {
        &self.route
    }
}

impl fmt::Debug for PushActivationEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushActivationEnvelope")
            .field("message_id", &self.message_id)
            .field("account", &self.account)
            .field("registration_epoch", &self.registration_epoch)
            .field("condition", &self.condition)
            .field("route_bytes", &self.route.len())
            .finish()
    }
}

/// An explicit Stage 4 activation request from a notification tap, a cold-launch
/// payload or a host-owned callback.
#[derive(Clone, PartialEq, Eq)]
pub struct PushActivationRequest {
    pub message_id: PushMessageId,
    pub route: Option<String>,
    pub account: Option<PushAccountId>,
    pub registration_epoch: Option<u64>,
    pub condition: AppDeliveryCondition,
}

impl PushActivationRequest {
    pub fn new(message_id: PushMessageId, condition: AppDeliveryCondition) -> Self {
        Self {
            message_id,
            route: None,
            account: None,
            registration_epoch: None,
            condition,
        }
    }

    pub fn with_route(mut self, route: impl Into<String>) -> Self {
        self.route = Some(route.into());
        self
    }

    pub fn with_account(mut self, account: PushAccountId, registration_epoch: u64) -> Self {
        self.account = Some(account);
        self.registration_epoch = Some(registration_epoch);
        self
    }
}

impl fmt::Debug for PushActivationRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushActivationRequest")
            .field("message_id", &self.message_id)
            .field("account", &self.account)
            .field("registration_epoch", &self.registration_epoch)
            .field("condition", &self.condition)
            .field("has_route", &self.route.is_some())
            .finish()
    }
}

/// Public-safe registration view.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistrationSnapshot {
    pub registered: bool,
    pub kind: Option<PushProviderKind>,
    pub provider_id: Option<String>,
    pub account: Option<PushAccountId>,
    pub token_revision: u64,
    pub registration_epoch: u64,
    pub token_fingerprint: Option<TokenFingerprint>,
    pub sync: RegistrationSyncState,
}

/// Result of a successful device registration or rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRegistration {
    pub token_revision: u64,
    pub token_fingerprint: TokenFingerprint,
    pub registration_epoch: u64,
    pub account: Option<PushAccountId>,
    pub sync: RegistrationSyncState,
}

/// Bounded public counters. They contain no per-message payload or identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushCounters {
    pub duplicate_suppressed: u64,
    pub expired_dropped: u64,
    pub oversize_rejected: u64,
    pub account_rejected: u64,
    pub invalid_route_rejected: u64,
    pub quarantined_payloads: u64,
    pub background_handoffs_queued: u64,
    pub background_handoffs_dropped: u64,
    pub presented: u64,
    pub presentations_suppressed: u64,
    pub activations_routed: u64,
    pub activations_queued: u64,
}

/// Public-safe service snapshot. It never carries registration material.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PushSnapshot {
    pub selected: Option<PushProviderDescriptor>,
    pub registration: RegistrationSnapshot,
    pub subscriptions: Vec<PushSubscription>,
    pub pending_secret_cleanups: Vec<AccountTombstone>,
    pub pending_background_handoffs: usize,
    pub pending_cold_activations: usize,
    pub counters: PushCounters,
}

/// The durable, non-secret push profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushDurableState {
    pub format_version: u32,
    pub registration_epoch: u64,
    pub token_revision: u64,
    pub fingerprint: Option<TokenFingerprint>,
    pub kind: Option<PushProviderKind>,
    pub provider_id: Option<String>,
    pub channel_id: Option<String>,
    pub account: Option<PushAccountId>,
    pub sync: RegistrationSyncState,
    pub subscriptions: Vec<PushSubscription>,
    pub tombstones: Vec<AccountTombstone>,
    pub pending_cold_activations: Vec<PendingActivation>,
    pub next_activation_sequence: u64,
}

impl Default for PushDurableState {
    fn default() -> Self {
        Self {
            format_version: crate::store::PUSH_FORMAT_VERSION,
            registration_epoch: 0,
            token_revision: 0,
            fingerprint: None,
            kind: None,
            provider_id: None,
            channel_id: None,
            account: None,
            sync: RegistrationSyncState::Unregistered,
            subscriptions: Vec::new(),
            tombstones: Vec::new(),
            pending_cold_activations: Vec::new(),
            next_activation_sequence: 1,
        }
    }
}

impl PushDurableState {
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.format_version != crate::store::PUSH_FORMAT_VERSION {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has an unsupported schema version",
            ));
        }
        if self.token_revision > 0 && self.fingerprint.is_none() {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has a revision without a fingerprint",
            ));
        }
        if self.token_revision > 0 && self.kind.is_none() {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has a revision without a provider kind",
            ));
        }
        if self.subscriptions.len() > 256
            || self.tombstones.len() > 64
            || self.pending_cold_activations.len() > MAX_PENDING_COLD_ACTIVATIONS
        {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state exceeds a bounded collection",
            ));
        }
        if let Some(provider) = &self.provider_id
            && !identifier(provider, MAX_PUSH_PROVIDER_IDENTIFIER_BYTES, b":/@")
        {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has an invalid provider identity",
            ));
        }
        if let Some(channel) = &self.channel_id
            && !identifier(channel, MAX_PUSH_PROVIDER_IDENTIFIER_BYTES, b":@")
        {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has an invalid channel identity",
            ));
        }
        if self.token_revision == 0 && self.fingerprint.is_some() {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has a fingerprint without a revision",
            ));
        }
        let mut tombstoned = BTreeSet::new();
        let mut device_scoped_tombstones = 0usize;
        for tombstone in &self.tombstones {
            if !tombstoned.insert(&tombstone.account) {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state repeats an account tombstone",
                ));
            }
            if tombstone.registration_epoch == 0 {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state has a tombstone without a registration epoch",
                ));
            }
            if tombstone.device_scope {
                device_scoped_tombstones = device_scoped_tombstones.saturating_add(1);
            }
        }
        if device_scoped_tombstones > 1 {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Push durable state has more than one device-scoped tombstone",
            ));
        }
        let mut previous_sequence = 0u64;
        for activation in &self.pending_cold_activations {
            if crate::validation::validate_activation_route(&activation.route).is_err() {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state has an unsafe pending activation route",
                ));
            }
            if activation.registration_epoch == 0 {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state has a pending activation without an epoch",
                ));
            }
            if activation.sequence <= previous_sequence
                || activation.sequence >= self.next_activation_sequence
            {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state has a non-monotonic pending activation sequence",
                ));
            }
            previous_sequence = activation.sequence;
        }
        let mut topics = BTreeSet::new();
        for subscription in &self.subscriptions {
            if !topics.insert(&subscription.topic) {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state repeats a topic subscription",
                ));
            }
            if subscription.owner.account() != self.account.as_ref()
                && !subscription.owner.is_device()
            {
                return Err(SystemError::new(
                    ErrorCode::IntegrityFailure,
                    "Push durable state binds a subscription to another account",
                ));
            }
        }
        match self.sync {
            RegistrationSyncState::PendingServerSync { revision }
            | RegistrationSyncState::SyncedAtRevision { revision } => {
                if revision == 0 || revision != self.token_revision {
                    return Err(SystemError::new(
                        ErrorCode::IntegrityFailure,
                        "Push durable state has an inconsistent sync revision",
                    ));
                }
            }
            RegistrationSyncState::Unregistered => {
                if self.token_revision > 0 {
                    return Err(SystemError::new(
                        ErrorCode::IntegrityFailure,
                        "Push durable state has a revision without a sync state",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn tombstone_for(&self, account: &PushAccountId) -> Option<&AccountTombstone> {
        self.tombstones
            .iter()
            .find(|tombstone| &tombstone.account == account)
    }

    pub fn is_tombstoned(&self, account: &PushAccountId) -> bool {
        self.tombstone_for(account).is_some()
    }

    pub fn registration(&self) -> RegistrationSnapshot {
        RegistrationSnapshot {
            registered: self.token_revision > 0,
            kind: self.kind,
            provider_id: self.provider_id.clone(),
            account: self.account.clone(),
            token_revision: self.token_revision,
            registration_epoch: self.registration_epoch,
            token_fingerprint: self.fingerprint.clone(),
            sync: self.sync,
        }
    }
}

/// Public-safe push observations. Events contain fingerprints and metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushEvent {
    ProvidersDiscovered {
        count: u32,
        platform: PushPlatform,
    },
    ProviderSelected {
        kind: PushProviderKind,
        provider_id: String,
        channel_id: Option<String>,
    },
    DeviceRegistered {
        kind: PushProviderKind,
        token_revision: u64,
        token_fingerprint: TokenFingerprint,
        registration_epoch: u64,
    },
    RegistrationSyncAcknowledged {
        revision: u64,
    },
    TokenRotated {
        trigger: TokenRotationTrigger,
        token_revision: u64,
        token_fingerprint: TokenFingerprint,
        registration_epoch: u64,
    },
    DeviceUnregistered {
        token_revision: u64,
        registration_epoch: u64,
    },
    AccountBound {
        account: PushAccountId,
        registration_epoch: u64,
    },
    AccountSwitched {
        previous: Option<PushAccountId>,
        account: PushAccountId,
        registration_epoch: u64,
    },
    AccountLoggedOut {
        account: PushAccountId,
        registration_epoch: u64,
        secrets_deleted: u32,
    },
    SecretCleanupPending {
        account: PushAccountId,
        blocker: SecretCleanupBlocker,
        device_scope: bool,
    },
    SecretCleanupCompleted {
        account: PushAccountId,
    },
    SubscriptionAdded {
        topic: PushTopic,
        owner: SubscriptionOwner,
        registration_epoch: u64,
    },
    SubscriptionRemoved {
        topic: PushTopic,
        registration_epoch: u64,
    },
    ProviderHandoffRejected {
        message_id: PushMessageId,
        outcome: ProviderHandoffOutcome,
    },
    MessageAccepted {
        message_id: PushMessageId,
        condition: AppDeliveryCondition,
        payload_kind: PushPayloadKind,
        registration_epoch: u64,
    },
    BackgroundWorkQueued {
        message_id: PushMessageId,
        class: BackgroundWorkClass,
        depth: usize,
    },
    BackgroundWorkDropped {
        message_id: PushMessageId,
        capacity: usize,
    },
    LocalPresentation {
        message_id: PushMessageId,
        outcome: LocalPresentationOutcome,
    },
    ActivationRouted {
        message_id: PushMessageId,
        registration_epoch: u64,
    },
    ActivationQueued {
        message_id: PushMessageId,
        depth: usize,
    },
    ActivationRejected {
        message_id: PushMessageId,
        outcome: PushActivationOutcome,
    },
    DedupJournalQuarantined {
        bytes: u64,
    },
    OperationFailed {
        operation: PushOperation,
        code: ErrorCode,
    },
    OperationCancelled {
        operation: PushOperation,
        reason: CancelReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_control_characters_and_oversize_values() {
        assert!(PushAccountId::new("box-account").is_ok());
        assert!(PushAccountId::new("").is_err());
        assert!(PushAccountId::new("bad\naccount").is_err());
        assert!(PushAccountId::new("x".repeat(MAX_PUSH_ACCOUNT_ID_BYTES + 1)).is_err());
        assert!(PushTopic::new("orders/eu").is_ok());
        assert!(PushMessageId::new("0:1234%abcd").is_ok());
        assert!(PushMessageId::new("bad message").is_err());
    }

    #[test]
    fn token_fingerprints_are_sha256_prefixes_and_never_the_material() {
        let material = b"raw-device-token-that-must-not-leak";
        let fingerprint = TokenFingerprint::of(material);
        assert_eq!(fingerprint.as_str().len(), TOKEN_FINGERPRINT_HEX_CHARS);
        assert!(!fingerprint.as_str().contains("raw-device"));
        assert_eq!(
            format!("{fingerprint:?}"),
            format!("TokenFingerprint(\"{}\")", fingerprint.as_str())
        );
        assert_eq!(TokenFingerprint::of(material), fingerprint);
        assert_ne!(TokenFingerprint::of(b"other"), fingerprint);
    }

    #[test]
    fn provider_descriptors_are_selectable_only_when_configured_and_reachable() {
        let mut descriptor = PushProviderDescriptor::new(PushProviderKind::Apns, "apns-primary")
            .with_configuration(ProviderConfiguration::Ready)
            .with_health(ProviderHealth::Ready);
        assert!(descriptor.is_selectable());
        assert!(descriptor.validate().is_ok());
        descriptor.health = ProviderHealth::Degraded;
        assert!(descriptor.is_selectable());
        descriptor.health = ProviderHealth::Outage;
        assert!(!descriptor.is_selectable());
        descriptor.health = ProviderHealth::Unavailable;
        assert!(!descriptor.is_selectable());
        descriptor.health = ProviderHealth::Ready;
        descriptor.configuration = ProviderConfiguration::NotConfigured;
        assert!(!descriptor.is_selectable());
    }

    #[test]
    fn durable_state_validation_rejects_inconsistent_sync_and_ownership() {
        let state = PushDurableState::default();
        assert!(state.validate().is_ok());
        let state = PushDurableState {
            sync: RegistrationSyncState::PendingServerSync { revision: 1 },
            ..PushDurableState::default()
        };
        assert!(state.validate().is_err());
        let state = PushDurableState {
            token_revision: 2,
            fingerprint: Some(TokenFingerprint::of(b"token")),
            kind: Some(PushProviderKind::Fcm),
            sync: RegistrationSyncState::SyncedAtRevision { revision: 2 },
            ..PushDurableState::default()
        };
        assert!(state.validate().is_ok());
        assert_eq!(state.registration().token_revision, 2);
        assert!(state.registration().registered);
        let account = PushAccountId::new("account-a").expect("account");
        let mut invalid = state;
        invalid.subscriptions.push(PushSubscription::new(
            PushTopic::new("orders").expect("topic"),
            SubscriptionOwner::Account(account),
            0,
        ));
        assert!(invalid.validate().is_err());
    }
}
