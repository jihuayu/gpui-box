//! Lifecycle-aware tray and menu-bar status items with scoped RAII leases.
//!
//! The default build has no GPUI dependency and installs no native provider.
//! Status items are only reported as effective after an explicit
//! [`TrayProvider`] granted a native token; a grant that becomes stale during
//! the provider call is removed before any lease is registered.
//!
//! The optional `gpui` feature adds the `App` global / `SystemServices` adapter
//! and the `gpui_adapter::TraySurfaceScope`. See `docs/S_TRAY.md`.

mod menu;
mod model;
mod platform;
mod provider;
mod service;

pub use menu::TrayContextMenuResolver;
pub use model::{
    CloseInterceptionOutcome, ColorSchemeMatch, EffectiveStatusSurface, HighContrastMatch,
    IconPayload, MAX_ALTERNATIVE_IDENTIFIER_BYTES, MAX_ICON_ASSET_BYTES, MAX_ICON_DIMENSION,
    MAX_STATUS_ITEM_KEY_BYTES, MAX_STATUS_TEXT_BYTES, MobileAlternativeSurface,
    ResidencyBrokenReason, ResidencyFallbackReason, ResidencyRecoveryAction, ScaleBucket,
    ShellAbsenceFallback, StaleCallbackReason, StatusIconSet, StatusIconVariant, StatusItemId,
    StatusItemKey, StatusItemRole, StatusItemSpec, TrayClickAction, TrayClickTrigger,
    TrayLeaseScope, TrayOperation, TrayResidencyPolicy, ValidatedStatusItemSpec, WindowClosePolicy,
};
pub use platform::{
    PlatformOperationSupport, PlatformTarget, operation_support, platform_operations,
    platform_support_matrix,
};
pub use provider::{
    FakeCreateEvent, FakeCreateHook, FakeHostedItem, FakeTrayProvider, PROVIDER_MANIFEST,
    ProviderItemGrant, ShellTrayStatus, TrayClickEvent, TrayCreateRequest, TrayItemProjection,
    TrayItemUpdate, TrayMenuItemClick, TrayProvider, TrayProviderListener, TrayProviderNotice,
    UnsupportedTrayProvider,
};
pub use service::{
    DEFAULT_TRAY_EVENT_CAPACITY, StatusItemLease, StatusItemSnapshot, TrayClickOutcome, TrayEvent,
    TrayService,
};

pub use gpui_lifecycle::{
    AppPhase, Appearance, LifecycleState, PreferenceValue, SystemPreferences,
};
pub use gpui_menus::{
    CommandId, CommandPayload, ContextMenu, MenuCommandService, ResolvedContextMenu,
    ResolvedMenuItem, SystemActionTarget,
};
pub use gpui_system::{
    Availability, CancelReason, CancellationToken, Capability, Completion, Configuration,
    Constraint, ErrorCode, Event, EventEnvelope, EventSubscription, Evidence, OperationContext,
    PermissionState, Platform, ProviderManifest, RecoveryAction, Request, RetryAdvice,
    ServiceResult, Support, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
};

/// The coordinated upstream GPUI revision used by this distribution.
pub const UPSTREAM_GPUI_REVISION: &str = "f8cfc7ed9e57503def9479138894f3139f5400cc";

/// The gpui-tray package version.
pub const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "gpui")]
pub mod gpui_adapter;
