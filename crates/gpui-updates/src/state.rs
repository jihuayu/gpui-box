use crate::{ArtifactVerification, InstalledIdentity, UpdateVerifier, VerifiedUpdate, error};
use gpui_system::{
    CancelReason, Completion, ErrorCode, OperationContext, ServiceResult, SystemError,
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

/// An opaque operation generation. Old attempts and foreign controllers cannot
/// complete a new phase, even when their native callbacks arrive late.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateTicket {
    session: u64,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available { version_code: u32 },
    Downloading { received: u64, total: u64 },
    Verified { version_code: u32 },
    AwaitingUserAction { version_code: u32 },
    Installing { version_code: u32 },
    Completed { version_code: u32 },
    Cancelled(CancelReason),
    Failed { code: ErrorCode, retryable: bool },
}

/// Native handoff acceptance and installed-version confirmation are separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallerReceipt {
    pub version_code: u32,
    pub accepted: bool,
}

pub struct UpdateController {
    installed: InstalledIdentity,
    session: u64,
    generation: u64,
    state: UpdateState,
    offered: Option<VerifiedUpdate>,
    artifact: Option<ArtifactVerification>,
    handoff_context: Option<OperationContext>,
}

impl UpdateController {
    pub fn new(installed: InstalledIdentity) -> Self {
        Self {
            installed,
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            generation: 0,
            state: UpdateState::Idle,
            offered: None,
            artifact: None,
            handoff_context: None,
        }
    }
    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    /// Retry is a fresh check/download. Previously accepted installer handoffs
    /// require reconciliation, preventing an automatic duplicate presentation.
    pub fn begin_check(&mut self) -> Result<UpdateTicket, SystemError> {
        if !matches!(
            self.state,
            UpdateState::Idle
                | UpdateState::UpToDate
                | UpdateState::Available { .. }
                | UpdateState::Completed { .. }
                | UpdateState::Cancelled(_)
                | UpdateState::Failed {
                    retryable: true,
                    ..
                }
        ) {
            return Err(error(
                ErrorCode::Busy,
                "Update workflow requires completion or reconciliation",
            ));
        }
        self.offered = None;
        self.artifact = None;
        self.handoff_context = None;
        self.advance(UpdateState::Checking)
    }

    pub fn checked(
        &mut self,
        ticket: UpdateTicket,
        update: Option<VerifiedUpdate>,
    ) -> Result<bool, SystemError> {
        if !self.current(ticket) || self.state != UpdateState::Checking {
            return Ok(false);
        }
        let Some(update) = update else {
            self.state = UpdateState::UpToDate;
            return Ok(true);
        };
        let metadata = update.metadata();
        if metadata.application_id != self.installed.application_id
            || metadata.version_code <= self.installed.version_code
            || metadata.artifact.signer_sha256 != self.installed.signer_sha256
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Verified update does not match this installed controller",
            ));
        }
        self.state = UpdateState::Available {
            version_code: metadata.version_code,
        };
        self.offered = Some(update);
        Ok(true)
    }

    pub fn begin_download(&mut self) -> Result<UpdateTicket, SystemError> {
        if !matches!(self.state, UpdateState::Available { .. }) {
            return Err(error(
                ErrorCode::InvalidInput,
                "An available update is required",
            ));
        }
        let total = self
            .offered
            .as_ref()
            .ok_or_else(|| error(ErrorCode::IntegrityFailure, "Update offer is missing"))?
            .metadata()
            .artifact
            .size;
        self.advance(UpdateState::Downloading { received: 0, total })
    }

    pub fn progress(&mut self, ticket: UpdateTicket, received: u64) -> Result<bool, SystemError> {
        if !self.current(ticket) {
            return Ok(false);
        }
        let UpdateState::Downloading {
            received: previous,
            total,
        } = self.state
        else {
            return Ok(false);
        };
        if received < previous || received > total {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update progress is non-monotonic or exceeds authenticated size",
            ));
        }
        self.state = UpdateState::Downloading { received, total };
        Ok(true)
    }

    pub fn artifact_verified(
        &mut self,
        ticket: UpdateTicket,
        receipt: ArtifactVerification,
    ) -> Result<bool, SystemError> {
        if !self.current(ticket) || !matches!(self.state, UpdateState::Downloading { .. }) {
            return Ok(false);
        }
        let offered = self
            .offered
            .as_ref()
            .ok_or_else(|| error(ErrorCode::IntegrityFailure, "Update offer is missing"))?;
        let metadata = offered.metadata();
        if receipt.payload_sha256 != offered.payload_sha256()
            || receipt.artifact_sha256 != metadata.artifact.sha256
            || receipt.size != metadata.artifact.size
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Artifact verification receipt belongs to another offer",
            ));
        }
        self.state = UpdateState::Verified {
            version_code: metadata.version_code,
        };
        self.artifact = Some(receipt);
        Ok(true)
    }

    /// Call immediately before presenting the native installer. The provider must
    /// independently recheck install-source authorization and APK identity/signer,
    /// bind the inspected private file to its grant and report actual handoff.
    pub fn begin_installer_handoff(
        &mut self,
        verifier: &UpdateVerifier,
        now: u64,
        context: &OperationContext,
    ) -> ServiceResult<UpdateTicket> {
        context.check_at(std::time::Instant::now())?;
        context.require_ui()?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        if !matches!(self.state, UpdateState::Verified { .. }) || self.artifact.is_none() {
            return Err(error(
                ErrorCode::InvalidInput,
                "Verified artifact is required for installer handoff",
            ));
        }
        let offered = self
            .offered
            .as_ref()
            .ok_or_else(|| error(ErrorCode::IntegrityFailure, "Update offer is missing"))?;
        verifier.revalidate(offered, now)?;
        if now < offered.verified_at() || now >= offered.metadata().expires_at {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Update offer expired before installer handoff",
            ));
        }
        let version_code = offered.metadata().version_code;
        self.handoff_context = Some(context.clone());
        Ok(Completion::Completed(self.advance(
            UpdateState::AwaitingUserAction { version_code },
        )?))
    }

    pub fn installer_handoff(
        &mut self,
        ticket: UpdateTicket,
        accepted: bool,
    ) -> Option<InstallerReceipt> {
        if !self.current(ticket) {
            return None;
        }
        if self
            .handoff_context
            .as_ref()
            .is_none_or(|context| context.check_at(std::time::Instant::now()).is_err())
        {
            return None;
        }
        let UpdateState::AwaitingUserAction { version_code } = self.state else {
            return None;
        };
        self.state = if accepted {
            UpdateState::Installing { version_code }
        } else {
            UpdateState::Cancelled(CancelReason::User)
        };
        Some(InstallerReceipt {
            version_code,
            accepted,
        })
    }

    /// A native installer cancellation/failure receipt after presentation must
    /// match that attempt. This enables a fresh check without replaying handoff.
    pub fn installer_declined(&mut self, ticket: UpdateTicket, reason: CancelReason) -> bool {
        if !self.current(ticket)
            || !matches!(
                self.state,
                UpdateState::AwaitingUserAction { .. } | UpdateState::Installing { .. }
            )
        {
            return false;
        }
        self.state = UpdateState::Cancelled(reason);
        self.offered = None;
        self.artifact = None;
        true
    }

    /// Reconciliation uses a fresh OS package query after relaunch/resume. A
    /// success callback or accepted installer presentation cannot mark completion.
    pub fn confirm_installed(&mut self, identity: &InstalledIdentity) -> Result<bool, SystemError> {
        let version_code = match self.state {
            UpdateState::Installing { version_code }
            | UpdateState::AwaitingUserAction { version_code } => version_code,
            _ => return Ok(false),
        };
        if identity.application_id != self.installed.application_id
            || identity.signer_sha256 != self.installed.signer_sha256
        {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Installed identity does not match update policy",
            ));
        }
        if identity.version_code != version_code {
            return Ok(false);
        }
        self.installed = identity.clone();
        self.state = UpdateState::Completed { version_code };
        self.offered = None;
        self.artifact = None;
        self.handoff_context = None;
        Ok(true)
    }

    /// Cancellation cannot undo a submitted installer. Its result must be
    /// reconciled or explicitly declined by the native provider.
    pub fn cancel(&mut self, reason: CancelReason) -> bool {
        if matches!(
            self.state,
            UpdateState::AwaitingUserAction { .. }
                | UpdateState::Installing { .. }
                | UpdateState::Completed { .. }
                | UpdateState::Idle
                | UpdateState::UpToDate
                | UpdateState::Cancelled(_)
                | UpdateState::Failed { .. }
        ) {
            return false;
        }
        self.state = UpdateState::Cancelled(reason);
        self.offered = None;
        self.artifact = None;
        true
    }

    pub fn fail(&mut self, ticket: UpdateTicket, code: ErrorCode, retryable: bool) -> bool {
        if !self.current(ticket)
            || !matches!(
                self.state,
                UpdateState::Checking | UpdateState::Downloading { .. }
            )
        {
            return false;
        }
        self.state = UpdateState::Failed { code, retryable };
        self.offered = None;
        self.artifact = None;
        true
    }

    fn current(&self, ticket: UpdateTicket) -> bool {
        ticket.session == self.session && ticket.generation == self.generation
    }
    fn advance(&mut self, state: UpdateState) -> Result<UpdateTicket, SystemError> {
        self.generation = self.generation.checked_add(1).ok_or_else(|| {
            error(
                ErrorCode::Unavailable,
                "Update operation generation exhausted",
            )
        })?;
        self.state = state;
        Ok(UpdateTicket {
            session: self.session,
            generation: self.generation,
        })
    }
}
