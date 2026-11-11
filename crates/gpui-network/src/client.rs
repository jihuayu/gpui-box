//! Client ownership, scoped revalidation and generation-checked lease
//! release.
//!
//! Account scope keys retain only a deterministic fingerprint. The raw account
//! identifier is never stored, formatted, serialized or emitted in an event.

use crate::service::ServiceInner;
use gpui_system::{ErrorCode, SurfaceId, SystemError};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex, Weak},
};

/// Upper bound on directives queued for one client between drains.
pub const MAX_PENDING_DIRECTIVES: usize = 64;

/// Monotonic client identity within one service session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClientId(pub u64);

/// Monotonic policy generation. Starts at [`PolicyEpoch::INITIAL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PolicyEpoch(pub u64);

impl PolicyEpoch {
    pub const INITIAL: Self = Self(1);

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Default for PolicyEpoch {
    fn default() -> Self {
        Self::INITIAL
    }
}

impl fmt::Display for PolicyEpoch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// A deterministic, non-reversible account fingerprint.
///
/// Construction hashes the raw identifier with FNV-1a 64 and retains only the
/// digest. `Debug`, `Display` and serde therefore cannot disclose the raw
/// account string.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AccountScopeKey {
    fingerprint: u64,
}

impl AccountScopeKey {
    pub fn new(account: &str) -> Result<Self, SystemError> {
        if account.is_empty() || account.len() > 256 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "An account scope key must contain 1 to 256 bytes",
            ));
        }
        if account.chars().any(char::is_control) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "An account scope key cannot contain control characters",
            ));
        }
        Ok(Self {
            fingerprint: fnv1a64(account.as_bytes()),
        })
    }

    pub fn from_account(account: &str) -> Result<Self, SystemError> {
        Self::new(account)
    }

    pub fn try_new(account: &str) -> Result<Self, SystemError> {
        Self::new(account)
    }

    pub const fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    pub fn fingerprint_hex(&self) -> String {
        format!("{:016x}", self.fingerprint)
    }
}

impl fmt::Debug for AccountScopeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "AccountScopeKey({})", self.fingerprint_hex())
    }
}

impl fmt::Display for AccountScopeKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.fingerprint_hex())
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The owner scope of a live client lease.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ClientOwnerScope {
    #[default]
    Application,
    Surface {
        surface: SurfaceId,
        generation: u64,
    },
    Account {
        account_key: AccountScopeKey,
    },
}

impl ClientOwnerScope {
    pub const fn surface(surface: SurfaceId, generation: u64) -> Self {
        Self::Surface {
            surface,
            generation,
        }
    }

    pub const fn account(account_key: AccountScopeKey) -> Self {
        Self::Account { account_key }
    }

    pub const fn is_application(self) -> bool {
        matches!(self, Self::Application)
    }

    pub fn surface_id(self) -> Option<SurfaceId> {
        match self {
            Self::Surface { surface, .. } => Some(surface),
            _ => None,
        }
    }
}

impl fmt::Display for ClientOwnerScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Application => formatter.write_str("application"),
            Self::Surface {
                surface,
                generation,
            } => write!(formatter, "surface({})/{}", surface.0, generation),
            Self::Account { account_key } => write!(formatter, "account({account_key})"),
        }
    }
}

/// Typed reason a client lease stopped being live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientRevocationReason {
    Released,
    SurfaceDisposed,
    StaleGeneration,
    AccountCleared,
    ServiceShutdown,
}

impl ClientRevocationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::SurfaceDisposed => "surface_disposed",
            Self::StaleGeneration => "stale_generation",
            Self::AccountCleared => "account_cleared",
            Self::ServiceShutdown => "service_shutdown",
        }
    }
}

/// A scoped instruction to revalidate or recycle network state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevalidationDirective {
    RecycleIdleConnections,
    DropAllPoolsAndReconnect,
    ReauthenticateProxy,
    RevalidateStaleQueries,
    ThrottleBackgroundTransfers,
    AwaitCaptivePortalClear,
}

impl RevalidationDirective {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecycleIdleConnections => "recycle_idle_connections",
            Self::DropAllPoolsAndReconnect => "drop_all_pools_and_reconnect",
            Self::ReauthenticateProxy => "reauthenticate_proxy",
            Self::RevalidateStaleQueries => "revalidate_stale_queries",
            Self::ThrottleBackgroundTransfers => "throttle_background_transfers",
            Self::AwaitCaptivePortalClear => "await_captive_portal_clear",
        }
    }

    pub const fn is_pool_hygiene(self) -> bool {
        matches!(self, Self::DropAllPoolsAndReconnect)
    }
}

/// One directive delivered to one client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopedRevalidation {
    pub client_id: ClientId,
    pub scope: ClientOwnerScope,
    pub epoch: PolicyEpoch,
    pub directive: RevalidationDirective,
}

impl ScopedRevalidation {
    pub const fn new(
        client_id: ClientId,
        scope: ClientOwnerScope,
        epoch: PolicyEpoch,
        directive: RevalidationDirective,
    ) -> Self {
        Self {
            client_id,
            scope,
            epoch,
            directive,
        }
    }
}

/// Scope filter for revalidation observers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RevalidationScopeFilter {
    #[default]
    All,
    Application,
    Surface {
        surface: SurfaceId,
    },
    Account {
        account_key: AccountScopeKey,
    },
}

impl RevalidationScopeFilter {
    pub fn matches(self, scope: ClientOwnerScope) -> bool {
        match (self, scope) {
            (Self::All, _) => true,
            (Self::Application, ClientOwnerScope::Application) => true,
            (
                Self::Surface { surface },
                ClientOwnerScope::Surface {
                    surface: actual, ..
                },
            ) => surface == actual,
            (
                Self::Account { account_key },
                ClientOwnerScope::Account {
                    account_key: actual,
                },
            ) => account_key == actual,
            _ => false,
        }
    }
}

impl From<ClientOwnerScope> for RevalidationScopeFilter {
    fn from(scope: ClientOwnerScope) -> Self {
        match scope {
            ClientOwnerScope::Application => Self::Application,
            ClientOwnerScope::Surface { surface, .. } => Self::Surface { surface },
            ClientOwnerScope::Account { account_key } => Self::Account { account_key },
        }
    }
}

impl From<Option<ClientOwnerScope>> for RevalidationScopeFilter {
    fn from(scope: Option<ClientOwnerScope>) -> Self {
        scope.map_or(Self::All, Self::from)
    }
}

struct ClientState {
    active: bool,
    revocation: Option<ClientRevocationReason>,
    observed_epoch: PolicyEpoch,
    directives: VecDeque<ScopedRevalidation>,
    pending_stale_revalidation: bool,
}

pub(crate) struct ClientInner {
    id: ClientId,
    scope: ClientOwnerScope,
    service: Weak<ServiceInner>,
    state: Mutex<ClientState>,
}

impl ClientInner {
    pub(crate) fn new(
        id: ClientId,
        scope: ClientOwnerScope,
        epoch: PolicyEpoch,
        service: Weak<ServiceInner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id,
            scope,
            service,
            state: Mutex::new(ClientState {
                active: true,
                revocation: None,
                observed_epoch: epoch,
                directives: VecDeque::new(),
                pending_stale_revalidation: false,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ClientState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn push_directive(
        &self,
        directive: RevalidationDirective,
        epoch: PolicyEpoch,
    ) -> bool {
        let mut state = self.lock();
        if !state.active {
            return false;
        }
        if state
            .directives
            .back()
            .is_some_and(|queued| queued.directive == directive && queued.epoch == epoch)
        {
            return true;
        }
        if state.directives.len() == MAX_PENDING_DIRECTIVES {
            state.directives.pop_front();
        }
        state.directives.push_back(ScopedRevalidation {
            client_id: self.id,
            scope: self.scope,
            epoch,
            directive,
        });
        true
    }

    pub(crate) fn defer_stale_revalidation(&self) -> bool {
        let mut state = self.lock();
        if !state.active {
            return false;
        }
        state.pending_stale_revalidation = true;
        true
    }

    fn has_pending_stale_revalidation(&self) -> bool {
        self.lock().pending_stale_revalidation
    }

    pub(crate) fn take_pending_stale_revalidation(&self) -> bool {
        let mut state = self.lock();
        if !state.active || !state.pending_stale_revalidation {
            return false;
        }
        state.pending_stale_revalidation = false;
        true
    }

    pub(crate) const fn scope_value(&self) -> ClientOwnerScope {
        self.scope
    }

    pub(crate) const fn client_id(&self) -> ClientId {
        self.id
    }

    pub(crate) fn revoke(&self, reason: ClientRevocationReason) -> bool {
        let mut state = self.lock();
        if !state.active {
            return false;
        }
        state.active = false;
        state.revocation = Some(reason);
        state.directives.clear();
        state.pending_stale_revalidation = false;
        true
    }

    fn release(&self, reason: ClientRevocationReason) -> bool {
        let service = {
            let mut state = self.lock();
            if !state.active {
                return false;
            }
            state.active = false;
            state.revocation = Some(reason);
            state.directives.clear();
            state.pending_stale_revalidation = false;
            self.service.clone()
        };
        if let Some(service) = service.upgrade() {
            service.client_released(self.id, reason);
        }
        true
    }
}

/// A live client lease. Dropping it releases the client automatically.
pub struct NetworkClientLease {
    inner: Arc<ClientInner>,
}

impl NetworkClientLease {
    pub(crate) fn new(inner: Arc<ClientInner>) -> Self {
        Self { inner }
    }

    pub fn client_id(&self) -> ClientId {
        self.inner.id
    }

    pub fn scope(&self) -> ClientOwnerScope {
        self.inner.scope
    }

    pub fn observed_epoch(&self) -> PolicyEpoch {
        self.inner.lock().observed_epoch
    }

    /// Advance the acknowledged policy epoch. Returns true when the client is
    /// live and the acknowledged epoch actually advanced.
    pub fn acknowledge_epoch(&self, epoch: PolicyEpoch) -> bool {
        let mut state = self.inner.lock();
        if !state.active || epoch <= state.observed_epoch {
            return false;
        }
        state.observed_epoch = epoch;
        true
    }

    pub fn is_active(&self) -> bool {
        self.inner.lock().active
    }

    pub fn revocation_reason(&self) -> Option<ClientRevocationReason> {
        self.inner.lock().revocation
    }

    pub fn drain_directives(&self) -> Vec<ScopedRevalidation> {
        let mut state = self.inner.lock();
        state.directives.drain(..).collect()
    }

    pub fn pending_directive_count(&self) -> usize {
        self.inner.lock().directives.len()
    }

    pub fn has_pending_stale_revalidation(&self) -> bool {
        self.inner.has_pending_stale_revalidation()
    }

    /// Release the lease. Returns true exactly once.
    pub fn release(&self) -> bool {
        self.inner.release(ClientRevocationReason::Released)
    }
}

impl Drop for NetworkClientLease {
    fn drop(&mut self) {
        // `release` only locks the client state and then removes the client
        // from the service registry; it never panics on a poisoned lock.
        let _ = self.inner.release(ClientRevocationReason::Released);
    }
}

impl fmt::Debug for NetworkClientLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NetworkClientLease")
            .field("client_id", &self.inner.id)
            .field("scope", &self.inner.scope)
            .field("active", &self.is_active())
            .finish()
    }
}
