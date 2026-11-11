//! Optional GPUI adapter: `App` global, `SystemServices`, surface scope and a
//! bounded-cursor entity model.
//!
//! Installation and entity access run on the owning GPUI UI thread. The core
//! [`AuthService`] remains `Send + Sync`; only UI-safe snapshot reads and
//! interactive launch/cancel helpers are exposed here. Token exchange, refresh,
//! logout and passkey server verification remain background calls.

use crate::{
    AccountId, AuthError, AuthEvent, AuthEventBatch, AuthEventCursor, AuthService, AuthSnapshot,
    BrowserAuthHandle, BrowserAuthRequest, DEFAULT_AUTH_EVENT_CAPACITY, ExecutionOwner,
    PresentationContext,
};
use gpui::{App, Context, Global};
use gpui_system::{
    ErrorCode, OperationContext, SurfaceId, SystemError, SystemServices, UiContext, UiOwner,
    gpui_adapter::EntityScope,
};
use std::{collections::VecDeque, sync::Arc, time::Instant};

impl Global for AuthService {}

/// Install the service into GPUI's `App` global.
pub fn install_auth_service(service: AuthService, cx: &mut App) {
    cx.set_global(service);
}

/// Register the service in the typed `SystemServices` registry.
pub fn install_auth_in_services(
    services: &mut SystemServices,
    service: AuthService,
) -> Result<(), SystemError> {
    services.register(service)
}

/// Install the service as the `App` global and in `SystemServices`.
pub fn init_auth(cx: &mut App, service: AuthService) -> AuthService {
    cx.set_global(service.clone());
    cx.default_global::<SystemServices>()
        .override_provider(service.clone());
    service
}

/// Resolve the service from the `App` global first, then `SystemServices`.
pub fn auth_service(cx: &App) -> Result<AuthService, SystemError> {
    if let Some(service) = cx.try_global::<AuthService>() {
        return Ok(service.clone());
    }
    let services = cx.try_global::<SystemServices>().ok_or_else(|| {
        SystemError::new(
            ErrorCode::Unavailable,
            "AuthService and SystemServices are not installed",
        )
    })?;
    services
        .service::<AuthService>()
        .map(|service| (*service).clone())
}

/// Convenience methods for GPUI applications.
pub trait AuthAppExt {
    fn install_auth_service(&mut self, service: AuthService);
    fn init_auth(&mut self, service: AuthService) -> AuthService;
    fn auth_service(&self) -> Result<AuthService, SystemError>;
}

impl AuthAppExt for App {
    fn install_auth_service(&mut self, service: AuthService) {
        install_auth_service(service, self);
    }

    fn init_auth(&mut self, service: AuthService) -> AuthService {
        init_auth(self, service)
    }

    fn auth_service(&self) -> Result<AuthService, SystemError> {
        auth_service(self)
    }
}

/// Entity-/surface-owned authentication scope.
///
/// Dropping the scope cancels only ceremonies bound to its exact
/// `(surface, generation)` pair. A stale scope never disposes a newer
/// generation, and zero ids are ignored by the service.
pub struct AuthSurfaceScope {
    owner: UiOwner,
    service: AuthService,
    surface: SurfaceId,
    generation: u64,
    ui: Option<UiContext>,
    entity_scope: Option<EntityScope>,
}

impl AuthSurfaceScope {
    /// Create a scope for the owner's current attachment generation. A disposed
    /// owner yields an inert scope whose [`Self::context`] reports expiry.
    pub fn new(owner: UiOwner, service: AuthService) -> Self {
        match owner.context(None) {
            Ok(ui) => Self::with_ui(owner, service, ui),
            Err(_) => Self {
                owner,
                service,
                surface: SurfaceId(0),
                generation: 0,
                ui: None,
                entity_scope: None,
            },
        }
    }

    pub fn try_new(owner: UiOwner, service: AuthService) -> Result<Self, SystemError> {
        let ui = owner.context(None)?;
        Ok(Self::with_ui(owner, service, ui))
    }

    fn with_ui(owner: UiOwner, service: AuthService, ui: UiContext) -> Self {
        Self {
            owner,
            service,
            surface: ui.surface,
            generation: ui.generation,
            ui: Some(ui),
            entity_scope: None,
        }
    }

    /// Tie the scope to a GPUI entity release callback.
    pub fn for_entity<T: 'static>(owner: UiOwner, service: AuthService, cx: &Context<T>) -> Self {
        let entity_scope = EntityScope::new(owner.clone(), cx);
        let mut scope = Self::new(owner, service);
        scope.entity_scope = Some(entity_scope);
        scope
    }

    pub fn owner(&self) -> &UiOwner {
        &self.owner
    }

    pub fn service(&self) -> &AuthService {
        &self.service
    }

    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// A UI execution context for this exact surface generation.
    pub fn context(&self) -> Result<OperationContext, SystemError> {
        let ui = self.ui.clone().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Auth surface scope has no live UI attachment",
            )
        })?;
        if !ui.is_current() {
            return Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Auth surface scope UI attachment is stale",
            ));
        }
        Ok(OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        })
    }

    pub fn presentation(&self) -> Result<PresentationContext, AuthError> {
        let ui = self.ui.as_ref().ok_or(AuthError::ContextExpired)?;
        if !ui.is_current() {
            return Err(AuthError::ContextExpired);
        }
        PresentationContext::new(self.surface, self.generation)
    }

    pub fn snapshot(&self, now: Instant) -> AuthSnapshot {
        self.service.snapshot(ExecutionOwner::Ui, now)
    }

    /// Launch a warm browser ceremony from this exact surface generation.
    pub fn begin_browser_auth(
        &self,
        request: BrowserAuthRequest,
        now: Instant,
    ) -> Result<BrowserAuthHandle, AuthError> {
        let presentation = self.presentation()?;
        if request.presentation() != presentation {
            return Err(AuthError::InvalidInput(
                "Browser request is bound to a different surface generation",
            ));
        }
        let _ = self.context()?;
        self.service
            .begin_browser_auth(ExecutionOwner::Ui, request, now)
    }

    pub fn cancel_ceremony(
        &self,
        ceremony_id: crate::CeremonyId,
        now: Instant,
    ) -> Result<bool, AuthError> {
        let _ = self.context()?;
        self.service
            .cancel_ceremony(ExecutionOwner::Ui, ceremony_id, now)
    }
}

impl Drop for AuthSurfaceScope {
    fn drop(&mut self) {
        if self.ui.is_some() {
            // Exact generation: an older scope never cancels a newer one.
            self.service
                .dispose_surface_generation(self.surface, self.generation);
        }
        if self.ui.as_ref().is_some_and(UiContext::is_current) {
            self.owner.dispose();
        }
        drop(self.entity_scope.take());
    }
}

impl std::fmt::Debug for AuthSurfaceScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthSurfaceScope")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

/// Alias matching the documented name.
pub type SurfaceAuthScope = AuthSurfaceScope;

/// Bounded drain of an [`AuthModel`] event cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthEventDrain {
    pub resync_required: bool,
    pub dropped_events: u64,
    pub events: Vec<AuthEvent>,
}

/// GPUI entity model over one surface-bound auth scope.
///
/// Create with `cx.new(|cx| AuthModel::new(owner, service, cx))`. The model
/// retains at most [`AuthModel::MAX_RETAINED_EVENTS`] recent events and keeps a
/// monotonic cursor; `drain_events` calls `cx.notify()` when anything changed.
pub struct AuthModel {
    scope: AuthSurfaceScope,
    cursor: AuthEventCursor,
    events: VecDeque<AuthEvent>,
    resync_required: bool,
    dropped_events: u64,
    snapshot: AuthSnapshot,
}

impl AuthModel {
    pub const MAX_RETAINED_EVENTS: usize = DEFAULT_AUTH_EVENT_CAPACITY;

    pub fn new(owner: UiOwner, service: AuthService, cx: &mut Context<Self>) -> Self {
        let scope = AuthSurfaceScope::for_entity(owner, service, cx);
        let snapshot = scope.snapshot(Instant::now());
        let cursor = scope.service().subscribe();
        Self {
            scope,
            cursor,
            events: VecDeque::new(),
            resync_required: false,
            dropped_events: 0,
            snapshot,
        }
    }

    pub fn service(&self) -> &AuthService {
        self.scope.service()
    }

    pub const fn surface(&self) -> SurfaceId {
        self.scope.surface()
    }

    pub const fn generation(&self) -> u64 {
        self.scope.generation()
    }

    pub fn snapshot(&self) -> &AuthSnapshot {
        &self.snapshot
    }

    pub fn snapshot_now(&self) -> AuthSnapshot {
        self.scope.snapshot(Instant::now())
    }

    /// Refresh the cached snapshot and notify observers.
    pub fn refresh_snapshot(&mut self, cx: &mut Context<Self>) -> AuthSnapshot {
        self.snapshot = self.scope.snapshot(Instant::now());
        cx.notify();
        self.snapshot.clone()
    }

    /// Drain all currently available events without blocking.
    ///
    /// A bounded-ring overflow is reported through `resync` and is sticky until
    /// [`Self::clear_resync_required`] is called.
    pub fn drain_events(&mut self, cx: &mut Context<Self>) -> AuthEventDrain {
        let batch: AuthEventBatch = self.service().events_since(self.cursor);
        self.cursor = batch.next_cursor;
        let mut events = Vec::new();
        if let Some(resync) = batch.resync {
            self.resync_required = true;
            self.dropped_events = self.dropped_events.saturating_add(resync.dropped_events);
        }
        for event in batch.events {
            if self.events.len() == Self::MAX_RETAINED_EVENTS {
                self.events.pop_front();
                self.resync_required = true;
                self.dropped_events = self.dropped_events.saturating_add(1);
            }
            self.events.push_back(event.clone());
            events.push(event);
        }
        if !events.is_empty() || self.resync_required {
            self.snapshot = self.scope.snapshot(Instant::now());
            cx.notify();
        }
        AuthEventDrain {
            resync_required: self.resync_required,
            dropped_events: self.dropped_events,
            events,
        }
    }

    pub const fn resync_required(&self) -> bool {
        self.resync_required
    }

    pub const fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    pub fn clear_resync_required(&mut self) {
        self.resync_required = false;
        self.dropped_events = 0;
    }

    pub fn retained_events(&self) -> impl Iterator<Item = &AuthEvent> {
        self.events.iter()
    }

    pub fn take_retained_events(&mut self) -> Vec<AuthEvent> {
        self.events.drain(..).collect()
    }

    pub fn begin_browser_auth(
        &mut self,
        request: BrowserAuthRequest,
        cx: &mut Context<Self>,
    ) -> Result<BrowserAuthHandle, AuthError> {
        let result = self.scope.begin_browser_auth(request, Instant::now());
        cx.notify();
        result
    }

    pub fn cancel_ceremony(
        &mut self,
        ceremony_id: crate::CeremonyId,
        cx: &mut Context<Self>,
    ) -> Result<bool, AuthError> {
        let result = self.scope.cancel_ceremony(ceremony_id, Instant::now());
        cx.notify();
        result
    }

    pub fn select_primary_account(
        &mut self,
        account_id: &AccountId,
        cx: &mut Context<Self>,
    ) -> Result<(), AuthError> {
        let result = self
            .service()
            .select_primary_account(ExecutionOwner::Ui, account_id);
        self.snapshot = self.scope.snapshot(Instant::now());
        cx.notify();
        result
    }

    pub fn evaluate_expiries(&mut self, cx: &mut Context<Self>) -> usize {
        let changed = self
            .service()
            .evaluate_expiries(ExecutionOwner::Ui, Instant::now())
            .len();
        if changed > 0 {
            self.snapshot = self.scope.snapshot(Instant::now());
            cx.notify();
        }
        changed
    }
}

impl Drop for AuthModel {
    fn drop(&mut self) {
        self.events.clear();
    }
}

impl std::fmt::Debug for AuthModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthModel")
            .field("surface", &self.surface())
            .field("generation", &self.generation())
            .field("accounts", &self.snapshot.accounts.len())
            .field("resync_required", &self.resync_required)
            .finish()
    }
}

/// Shared handle used by background completion helpers.
pub type SharedAuthService = Arc<AuthService>;

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use gpui::{AppContext, TestAppContext};
    use gpui_secrets::{
        CredentialProvider, DeleteReceipt, ProviderResult, SecretAccess, SecretBytes,
        SecretOperation, SecretPolicy,
    };
    use gpui_system::Capability;
    use std::{collections::BTreeMap, sync::Mutex};

    #[derive(Default)]
    struct MemoryCredentials(Mutex<BTreeMap<String, Vec<u8>>>);

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
                support: gpui_system::Support::Native,
                availability: gpui_system::Availability::Available,
                configuration: gpui_system::Configuration::Ready,
                permission: gpui_system::PermissionState::NotRequired,
                evidence: gpui_system::Evidence::ContractOnly,
                constraints: Vec::new(),
                observed_revision: 1,
            })
            .collect()
        }

        fn validate_policy(&self, _: SecretPolicy) -> Result<(), SystemError> {
            Ok(())
        }

        fn put(&self, target: &str, value: &SecretBytes) -> ProviderResult<()> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(target.to_owned(), value.expose().to_vec());
            Ok(SecretAccess::Available(()))
        }

        fn get(&self, target: &str) -> ProviderResult<Option<SecretBytes>> {
            let value = self
                .0
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
            let existed = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(target)
                .is_some();
            Ok(SecretAccess::Available(DeleteReceipt { existed }))
        }

        fn list(&self, prefix: &str) -> ProviderResult<Vec<String>> {
            Ok(SecretAccess::Available(
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .keys()
                    .filter(|target| target.starts_with(prefix))
                    .cloned()
                    .collect(),
            ))
        }
    }

    #[gpui::test]
    fn installs_global_and_system_services(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_system::gpui_adapter::install_system_services(SystemServices::new(), cx);
            let service = crate::AuthService::with_fakes(
                crate::AuthPlatform::Linux,
                crate::FakeBrowserAuthProvider::new(49_152),
                crate::FakeTokenExchangeBackend::new(
                    AccountId::new("account-1").expect("account"),
                    crate::ProviderId::new("idp.example").expect("provider"),
                ),
                crate::FakePasskeyProvider::new(),
                crate::FakePasskeyServerVerifier::new(
                    crate::ProviderId::new("idp.example").expect("provider"),
                ),
                gpui_secrets::Secrets::with_provider(
                    "gpui-auth-test",
                    Arc::new(MemoryCredentials::default()),
                )
                .expect("secrets"),
            )
            .expect("service");
            let installed = cx.init_auth(service.clone());
            assert!(installed.shares_state_with(&service));
            assert!(cx.auth_service().is_ok());
            let registered = cx
                .try_global::<SystemServices>()
                .expect("services")
                .service::<AuthService>()
                .expect("registry");
            assert!(service.shares_state_with(&registered));
        });
    }

    #[gpui::test]
    fn entity_scope_disposes_only_its_generation(cx: &mut TestAppContext) {
        let (entity, owner) = cx.update(|cx| {
            let service = crate::AuthService::with_fakes(
                crate::AuthPlatform::Linux,
                crate::FakeBrowserAuthProvider::new(49_152),
                crate::FakeTokenExchangeBackend::new(
                    AccountId::new("account-1").expect("account"),
                    crate::ProviderId::new("idp.example").expect("provider"),
                ),
                crate::FakePasskeyProvider::new(),
                crate::FakePasskeyServerVerifier::new(
                    crate::ProviderId::new("idp.example").expect("provider"),
                ),
                gpui_secrets::Secrets::with_provider(
                    "gpui-auth-test",
                    Arc::new(MemoryCredentials::default()),
                )
                .expect("secrets"),
            )
            .expect("service");
            let owner = UiOwner::new(SurfaceId(41));
            let entity = cx.new(|cx| AuthModel::new(owner.clone(), service, cx));
            (entity, owner)
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                assert_eq!(model.surface(), SurfaceId(41));
                assert_eq!(AuthModel::MAX_RETAINED_EVENTS, 64);
                let drain = model.drain_events(cx);
                assert!(drain.events.is_empty());
                let _ = model.refresh_snapshot(cx);
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
    }

    fn test_service() -> AuthService {
        crate::AuthService::with_fakes(
            crate::AuthPlatform::Linux,
            crate::FakeBrowserAuthProvider::new(49_152),
            crate::FakeTokenExchangeBackend::new(
                AccountId::new("account-1").expect("account"),
                crate::ProviderId::new("idp.example").expect("provider"),
            ),
            crate::FakePasskeyProvider::new(),
            crate::FakePasskeyServerVerifier::new(
                crate::ProviderId::new("idp.example").expect("provider"),
            ),
            gpui_secrets::Secrets::with_provider(
                "gpui-auth-test",
                Arc::new(MemoryCredentials::default()),
            )
            .expect("secrets"),
        )
        .expect("service")
    }

    fn browser_request(surface: u64, generation: u64, seed: u64) -> BrowserAuthRequest {
        BrowserAuthRequest::new(
            crate::ProviderId::new("idp.example").expect("provider"),
            "gpui-box-client",
            crate::AuthEndpointUrl::parse("https://idp.example/authorize").expect("endpoint"),
            crate::RedirectTarget::loopback(
                crate::LoopbackHost::Ipv4,
                Some(49_152),
                "/oauth/callback",
            )
            .expect("redirect"),
            crate::AuthScopes::new([crate::AuthScope::new("openid").expect("scope")])
                .expect("scopes"),
            crate::OAuthState::new(format!("state-{seed:016x}")).expect("state"),
            crate::PkcePair::from_verifier(
                crate::PkceVerifier::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
                    .expect("verifier"),
            ),
            PresentationContext::new(SurfaceId(surface), generation).expect("presentation"),
        )
        .expect("request")
    }

    #[gpui::test]
    fn surface_scope_drop_is_generation_safe(cx: &mut TestAppContext) {
        let (first, second, owner, service, ceremony_id) = cx.update(|_| {
            let service = test_service();
            let owner = UiOwner::new(SurfaceId(51));
            let first = AuthSurfaceScope::new(owner.clone(), service.clone());
            let handle = first
                .begin_browser_auth(browser_request(51, 1, 1), Instant::now())
                .expect("launch");
            let ceremony_id = handle.ceremony_id();
            owner.advance_generation().expect("advance");
            let second = AuthSurfaceScope::new(owner.clone(), service.clone());
            (first, second, owner, service, ceremony_id)
        });
        assert_eq!(service.active_ceremony_count(), 1);
        drop(first);
        assert_eq!(service.active_ceremony_count(), 0);
        assert!(
            service
                .snapshot(ExecutionOwner::Ui, Instant::now())
                .is_empty()
        );
        assert!(
            !service
                .cancel_ceremony(ExecutionOwner::Ui, ceremony_id, Instant::now())
                .expect("unknown")
        );
        assert!(
            owner.context(None).is_ok(),
            "stale scope must not dispose owner"
        );
        drop(second);
        assert!(owner.context(None).is_err());
    }

    #[gpui::test]
    fn model_drains_events_and_updates_snapshot(cx: &mut TestAppContext) {
        let (entity, service, owner) = cx.update(|cx| {
            let service = test_service();
            let owner = UiOwner::new(SurfaceId(52));
            let entity = cx.new(|cx| AuthModel::new(owner.clone(), service.clone(), cx));
            (entity, service, owner)
        });
        cx.update(|_| {
            let handle = service
                .begin_browser_auth(
                    ExecutionOwner::Ui,
                    browser_request(52, 1, 2),
                    Instant::now(),
                )
                .expect("launch");
            service
                .cancel_ceremony(ExecutionOwner::Ui, handle.ceremony_id(), Instant::now())
                .expect("cancel");
        });
        cx.update(|cx| {
            entity.update(cx, |model, cx| {
                let drain = model.drain_events(cx);
                assert!(drain.events.len() >= 2, "launch + cancel");
                assert!(!drain.resync_required);
                assert_eq!(model.retained_events().count(), drain.events.len());
                assert!(model.snapshot().is_empty());
            });
        });
        drop(entity);
        cx.update(|_| {});
        assert!(owner.context(None).is_err());
    }
}
