//! Contribution scopes, RAII registration leases and lazy handles.

use crate::service::ServiceInner;
use crate::{ErrorCode, SurfaceId, SystemError, UiOwner};
use std::{
    cell::Cell,
    fmt,
    rc::{Rc, Weak},
};

/// Identity of one contribution scope. Scope identities are service-local and
/// monotonic; they are never reused.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ContributionScopeId(pub u64);

/// How a contribution is bound to a live host surface.
#[derive(Clone)]
pub enum CommandScope {
    /// Available from any surface and from global triggers.
    Global,
    /// Bound to one surface generation.
    Surface { surface: SurfaceId, generation: u64 },
    /// Bound to a live [`UiOwner`] and its current surface generation.
    UiOwner(UiOwner),
}

impl CommandScope {
    pub const fn global() -> Self {
        Self::Global
    }

    pub const fn surface(surface: SurfaceId, generation: u64) -> Self {
        Self::Surface {
            surface,
            generation,
        }
    }

    pub fn ui_owner(owner: UiOwner) -> Self {
        Self::UiOwner(owner)
    }

    pub(crate) fn binding(&self) -> Result<ScopeBinding, SystemError> {
        match self {
            Self::Global => Ok(ScopeBinding::Global),
            Self::Surface {
                surface,
                generation,
            } => {
                if *generation == 0 {
                    return Err(SystemError::new(
                        ErrorCode::InvalidInput,
                        "A surface scope requires a nonzero generation",
                    ));
                }
                Ok(ScopeBinding::Surface {
                    surface: *surface,
                    generation: *generation,
                })
            }
            Self::UiOwner(owner) => {
                let context = owner.context(None)?;
                Ok(ScopeBinding::UiOwner {
                    owner: owner.clone(),
                    surface: context.surface,
                    generation: context.generation,
                })
            }
        }
    }
}

impl fmt::Debug for CommandScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global => formatter.write_str("CommandScope::Global"),
            Self::Surface {
                surface,
                generation,
            } => formatter
                .debug_struct("CommandScope::Surface")
                .field("surface", surface)
                .field("generation", generation)
                .finish(),
            Self::UiOwner(owner) => match owner.context(None) {
                Ok(context) => formatter
                    .debug_struct("CommandScope::UiOwner")
                    .field("surface", &context.surface)
                    .field("generation", &context.generation)
                    .finish(),
                Err(_) => formatter.write_str("CommandScope::UiOwner(disposed)"),
            },
        }
    }
}

#[derive(Clone)]
pub(crate) enum ScopeBinding {
    Global,
    Surface {
        surface: SurfaceId,
        generation: u64,
    },
    UiOwner {
        owner: UiOwner,
        surface: SurfaceId,
        generation: u64,
    },
}

impl ScopeBinding {
    pub(crate) fn surface(&self) -> Option<SurfaceId> {
        match self {
            Self::Global => None,
            Self::Surface { surface, .. } | Self::UiOwner { surface, .. } => Some(*surface),
        }
    }

    pub(crate) fn check_live(
        &self,
        active_generation: impl Fn(SurfaceId) -> Option<u64>,
    ) -> Result<(), SystemError> {
        match self {
            Self::Global => Ok(()),
            Self::Surface {
                surface,
                generation,
            } => {
                if active_generation(*surface) == Some(*generation) {
                    Ok(())
                } else {
                    Err(SystemError::new(
                        ErrorCode::ContextExpired,
                        "Contribution scope surface generation is no longer live",
                    ))
                }
            }
            Self::UiOwner {
                owner,
                surface,
                generation,
            } => {
                let context = owner.context(None)?;
                if context.surface == *surface
                    && context.generation == *generation
                    && active_generation(*surface) == Some(*generation)
                {
                    Ok(())
                } else {
                    Err(SystemError::new(
                        ErrorCode::ContextExpired,
                        "Contribution scope UI owner generation is no longer live",
                    ))
                }
            }
        }
    }
}

struct ScopeHandle {
    id: ContributionScopeId,
    service: Weak<ServiceInner>,
}

impl Drop for ScopeHandle {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            service.revoke_scope(self.id);
        }
    }
}

/// RAII contribution scope. Clones share one identity; revoking or dropping the
/// last clone revokes every registration made through the scope.
#[derive(Clone)]
pub struct ContributionScope {
    handle: Rc<ScopeHandle>,
}

impl ContributionScope {
    pub(crate) fn new(id: ContributionScopeId, service: Weak<ServiceInner>) -> Self {
        Self {
            handle: Rc::new(ScopeHandle { id, service }),
        }
    }

    pub fn id(&self) -> ContributionScopeId {
        self.handle.id
    }

    pub fn is_active(&self) -> bool {
        self.handle
            .service
            .upgrade()
            .is_some_and(|service| service.scope_active(self.handle.id))
    }

    /// Revoke this scope and every registration inside it. Idempotent.
    pub fn revoke(&self) -> bool {
        self.handle
            .service
            .upgrade()
            .is_some_and(|service| service.revoke_scope(self.handle.id))
    }

    pub(crate) fn service(&self) -> Result<Rc<ServiceInner>, SystemError> {
        self.handle.service.upgrade().ok_or_else(|| {
            SystemError::new(
                ErrorCode::ContextExpired,
                "Contribution scope service is no longer available",
            )
        })
    }
}

impl fmt::Debug for ContributionScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContributionScope")
            .field("id", &self.handle.id)
            .field("active", &self.is_active())
            .finish()
    }
}

/// RAII registration lease. Dropping it removes only the registrations created
/// through that lease and cancels their in-flight tasks.
#[derive(Debug)]
pub struct RegistrationLease {
    service: Weak<ServiceInner>,
    registration: u64,
    scope: ContributionScopeId,
    active: Cell<bool>,
}

impl RegistrationLease {
    pub(crate) fn new(
        service: Weak<ServiceInner>,
        registration: u64,
        scope: ContributionScopeId,
    ) -> Self {
        Self {
            service,
            registration,
            scope,
            active: Cell::new(true),
        }
    }

    pub const fn scope(&self) -> ContributionScopeId {
        self.scope
    }

    pub const fn registration_id(&self) -> u64 {
        self.registration
    }

    pub fn is_active(&self) -> bool {
        self.active.get()
            && self
                .service
                .upgrade()
                .is_some_and(|service| service.registration_active(self.registration))
    }

    /// Revoke this lease. Returns true exactly once, when the registration was
    /// still active.
    pub fn revoke(&self) -> bool {
        if !self.active.get() {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            self.active.set(false);
            return false;
        };
        let revoked = service.revoke_registration(self.registration);
        if revoked {
            self.active.set(false);
        }
        revoked
    }
}

impl Drop for RegistrationLease {
    fn drop(&mut self) {
        if self.active.get() {
            self.revoke();
        }
    }
}

/// Identity of one deferred contribution bundle.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct LazyContributionId(pub u64);

/// Materialization state of a lazy contribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LazyMaterializationState {
    Unmaterialized,
    Materializing,
    Materialized,
    Revoked,
}

/// Handle to one lazily materialized contribution factory.
#[derive(Debug)]
pub struct LazyContribution {
    id: LazyContributionId,
    registration: u64,
    service: Weak<ServiceInner>,
}

impl LazyContribution {
    pub(crate) fn new(
        id: LazyContributionId,
        registration: u64,
        service: Weak<ServiceInner>,
    ) -> Self {
        Self {
            id,
            registration,
            service,
        }
    }

    pub const fn id(&self) -> LazyContributionId {
        self.id
    }

    pub fn state(&self) -> Option<LazyMaterializationState> {
        self.service
            .upgrade()
            .and_then(|service| service.lazy_state(self.id))
    }

    pub fn is_materialized(&self) -> bool {
        self.state() == Some(LazyMaterializationState::Materialized)
    }

    /// Revoke the lazy contribution and everything it materialized. Idempotent.
    pub fn revoke(&self) -> bool {
        self.service
            .upgrade()
            .is_some_and(|service| service.revoke_registration(self.registration))
    }
}
