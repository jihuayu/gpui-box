use crate::{
    PermissionKind, PermissionOperation, PermissionOutcome, PermissionSnapshot, SettingsHandoff,
    SettingsTarget,
};
use gpui_system::{Capability, Completion, CompletionPort, ErrorCode, RequestId, SystemError};
use std::{
    cell::Cell,
    rc::{Rc, Weak},
};

/// A provider-facing description of one interactive permission prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionPrompt {
    pub kind: PermissionKind,
    pub request: RequestId,
}

/// The single native prompt lane.
///
/// Providers must not block: they retain this completion and call it from the
/// platform UI callback, or call it synchronously when the platform resolves
/// the prompt inline. Duplicate and stale completions are rejected.
pub trait PermissionProvider {
    /// Describe one operation without presenting a prompt or performing a
    /// settings handoff. This snapshot can become stale immediately.
    fn capability(&self, operation: PermissionOperation) -> Capability<PermissionOperation>;

    /// Freshly probe authoritative status. This must have no prompt or
    /// settings side effect and must report missing native configuration as
    /// `ErrorCode::InvalidConfiguration` with `RecoveryAction::RepairConfiguration`.
    fn query(&self, kind: PermissionKind) -> Result<PermissionSnapshot, SystemError>;

    /// Begin one native prompt. Return `Err` only when no prompt was presented.
    /// Exactly one completion is accepted; the service owns the interactive
    /// lane and passes a completion that is safe to hold across app resume.
    fn request_permission(
        &self,
        prompt: PermissionPrompt,
        completion: PermissionPromptCompletion,
    ) -> Result<(), SystemError>;

    /// Present the OS settings destination. The receipt describes the handoff,
    /// not a user toggle. Return `InvalidConfiguration` when the destination is
    /// not declared for this build.
    fn open_settings(&self, target: SettingsTarget) -> Result<SettingsHandoff, SystemError>;

    /// Kinds re-probed by `reconcile_on_resume`. Providers that support resume
    /// reconciliation should list every kind they can authoritatively query.
    fn tracked_kinds(&self) -> Vec<PermissionKind> {
        Vec::new()
    }
}

struct CompletionState {
    completed: Cell<bool>,
}

/// Cloneable provider callback. It stays local to the UI thread because the
/// service is `Rc`-backed and native prompt objects are not `Send`.
#[derive(Clone)]
pub struct PermissionPromptCompletion {
    port: CompletionPort<PermissionOutcome>,
    service: Weak<crate::service::ServiceInner>,
    token: u64,
    kind: PermissionKind,
    state: Rc<CompletionState>,
}

impl PermissionPromptCompletion {
    pub(crate) fn new(
        port: CompletionPort<PermissionOutcome>,
        service: Weak<crate::service::ServiceInner>,
        token: u64,
        kind: PermissionKind,
    ) -> Self {
        Self {
            port,
            service,
            token,
            kind,
            state: Rc::new(CompletionState {
                completed: Cell::new(false),
            }),
        }
    }

    pub fn id(&self) -> RequestId {
        self.port.id()
    }

    pub const fn kind(&self) -> PermissionKind {
        self.kind
    }

    /// Complete the native prompt with authoritative post-prompt status.
    /// Returns false for a duplicate, cancelled/disposed owner, an advanced UI
    /// generation, or a service that no longer exists.
    pub fn complete(&self, snapshot: PermissionSnapshot) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        if snapshot.kind != self.kind {
            if let Some(service) = self.service.upgrade() {
                service.abort_prompt(self.token);
            }
            self.port.complete(Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Permission snapshot does not match the prompt",
            )));
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        let Some(outcome) = service.finish_prompt(self.token, snapshot) else {
            return false;
        };
        self.port.complete(Ok(Completion::Completed(outcome)))
    }

    /// Complete the native prompt with a typed provider failure. Returns false
    /// if the completion is duplicate, stale or no longer owns the prompt lane.
    pub fn fail(&self, error: SystemError) -> bool {
        if self.state.completed.replace(true) {
            return false;
        }
        let Some(service) = self.service.upgrade() else {
            return false;
        };
        if !service.abort_prompt(self.token) {
            return false;
        }
        self.port.complete(Err(error))
    }
}
