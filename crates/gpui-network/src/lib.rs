//! System proxy, certificate, connectivity, captive-portal, cost and scoped
//! revalidation contracts.
//!
//! The default build has no GPUI dependency. Every row in the platform matrix
//! is [`Evidence::ContractOnly`]; no native provider is installed by this
//! crate. Interface reachability, captive-portal state, DNS/TLS failures and
//! endpoint availability are deliberately distinct so callers cannot conflate a
//! link being up with an endpoint being usable.
//!
//! The optional `gpui` feature adds the `App` global / `SystemServices` adapter,
//! `NetworkSurfaceScope` and `NetworkEntityModel`. See `docs/S_NETWORK_POLICY.md`.

mod auth;
mod client;
mod model;
mod origin;
mod platform;
mod provider;
mod proxy;
mod retry;
mod service;
mod trust;

pub use auth::{
    MAX_PROXY_AUTH_ATTEMPTS, ProxyAuthChallenge, ProxyAuthHeader, ProxyAuthOutcome,
    ProxyAuthOutcomeKind, ProxyAuthScheme, ProxyCredentialResolver, ProxyCredentialSource,
    SecretsProxyCredentialSource,
};
pub use client::{
    AccountScopeKey, ClientId, ClientOwnerScope, ClientRevocationReason, MAX_PENDING_DIRECTIVES,
    NetworkClientLease, PolicyEpoch, RevalidationDirective, RevalidationScopeFilter,
    ScopedRevalidation,
};
pub use model::{
    CaptivePortalStatus, ConnectionCost, DataSaverMode, DnsFailureKind, EndpointDiagnostic,
    InterfaceReachability, NetworkSnapshot, NetworkWorkloadClass, SignalSource, TlsFailureReason,
    TransportKind, WorkloadBudgetDecision,
};
pub use origin::{EndpointHost, EndpointOrigin, OriginScheme};
pub use platform::{
    NetworkOperation, NetworkPlatform, PlatformOperationSupport, PlatformTarget,
    WebBackendMaturity, WebNetworkPolicy, operation_support, platform_operations,
    platform_support_matrix, web_capability_policy,
};
pub use provider::{
    FakeNetworkProvider, NetworkProvider, NetworkProviderListener, NetworkProviderNotice,
    PROVIDER_MANIFEST, UnavailableNetworkProvider,
};
pub use proxy::{
    BypassPattern, ProxyCandidate, ProxyConfig, ProxyRule, ProxyScheme, validate_proxy_config,
};
pub use retry::{RetryBlockReason, RetryDecision, RetryPolicy, RetryTracker};
pub use service::{
    DEFAULT_NETWORK_EVENT_CAPACITY, NetworkEvent, NetworkService, PolicyUpdate,
    RevalidationObserverLease, compare_epochs,
};
pub use trust::{
    CertificateChainObservation, CertificateObservation, ObservedRootKind, SpkiPinSha256,
    TrustEvaluation, TrustProfile, TrustStorePolicy, VerifiedAnchorKind, evaluate_tls_trust,
    validate_trust_policy,
};

pub use gpui_lifecycle::{AppPhase, LifecycleState};
pub use gpui_secrets::{CredentialKey, SecretAccess, SecretBytes, Secrets};
pub use gpui_system::{
    Availability, CancelReason, CancellationToken, Capability, Completion, Configuration,
    Constraint, ErrorCode, Event, EventEnvelope, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-network package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
