//! Screen-owned local-unlock flow model shared by gallery and settings surfaces.

use crate::{
    AccountId, LocalAuthReadiness, LocalAuthSnapshot, SystemError, UnlockPolicy,
    VerificationOutcome, VerificationPolicy, VerificationPurpose, VerificationRequest,
};

/// The screen state for one account's local-unlock flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalUnlockFlowState {
    /// No successful verification is active for this screen.
    Locked,
    /// A non-interactive readiness probe is in flight.
    CheckingAvailability,
    /// A native local-auth prompt is active.
    Prompting,
    /// This screen holds a successful local verification.
    Unlocked,
    /// Enrollment, lockout, hardware or policy recovery is required.
    RecoveryRequired,
}

/// Account-scoped local-unlock flow owned by a screen, not by a provider.
///
/// The model couples the account, application unlock policy and requested
/// verification policy without owning native UI or a `gpui-box-components` type.
#[derive(Debug, Clone)]
pub struct LocalUnlockFlowModel {
    account: AccountId,
    unlock_policy: UnlockPolicy,
    verification_policy: VerificationPolicy,
    state: LocalUnlockFlowState,
}

impl LocalUnlockFlowModel {
    pub fn new(
        account: AccountId,
        unlock_policy: UnlockPolicy,
        verification_policy: VerificationPolicy,
    ) -> Result<Self, SystemError> {
        unlock_policy.validate()?;
        Ok(Self {
            account,
            unlock_policy,
            verification_policy,
            state: LocalUnlockFlowState::Locked,
        })
    }

    pub fn account(&self) -> &AccountId {
        &self.account
    }

    pub fn unlock_policy(&self) -> &UnlockPolicy {
        &self.unlock_policy
    }

    pub fn verification_policy(&self) -> VerificationPolicy {
        self.verification_policy
    }

    pub fn state(&self) -> LocalUnlockFlowState {
        self.state
    }

    pub fn is_prompting(&self) -> bool {
        self.state == LocalUnlockFlowState::Prompting
    }

    pub fn is_unlocked(&self) -> bool {
        self.state == LocalUnlockFlowState::Unlocked
    }

    /// Begin the non-interactive readiness phase from a recoverable state.
    pub fn begin_availability_check(&mut self) {
        if matches!(
            self.state,
            LocalUnlockFlowState::Locked | LocalUnlockFlowState::RecoveryRequired
        ) {
            self.state = LocalUnlockFlowState::CheckingAvailability;
        }
    }

    /// Apply a readiness probe. Only `Available` allows the flow to prompt.
    pub fn apply_availability(&mut self, snapshot: &LocalAuthSnapshot) {
        self.state = if matches!(snapshot.readiness, LocalAuthReadiness::Available { .. }) {
            LocalUnlockFlowState::Locked
        } else {
            LocalUnlockFlowState::RecoveryRequired
        };
    }

    /// Enter the prompting state. Returns false when the flow is not ready.
    pub fn begin_prompt(&mut self) -> bool {
        if self.state == LocalUnlockFlowState::Locked {
            self.state = LocalUnlockFlowState::Prompting;
            true
        } else {
            false
        }
    }

    /// Apply the interactive outcome. A user cancellation returns to `Locked`;
    /// enrollment, lockout, hardware or policy failures require recovery.
    pub fn apply_outcome(&mut self, outcome: &VerificationOutcome) {
        self.state = match outcome {
            VerificationOutcome::Verified(_) => LocalUnlockFlowState::Unlocked,
            VerificationOutcome::UserCancelled => LocalUnlockFlowState::Locked,
            VerificationOutcome::NotEnrolled { .. }
            | VerificationOutcome::LockedOut { .. }
            | VerificationOutcome::HardwareMissing
            | VerificationOutcome::PolicyFallbackRequired => LocalUnlockFlowState::RecoveryRequired,
        };
    }

    /// Build the verification request for this screen's policy.
    pub fn verification_request(&self, purpose: VerificationPurpose) -> VerificationRequest {
        VerificationRequest::new(self.verification_policy, purpose)
    }

    /// Drop the local screen's unlocked state.
    pub fn lock(&mut self) {
        self.state = LocalUnlockFlowState::Locked;
    }

    /// Leave recovery so the screen can probe again.
    pub fn recover(&mut self) {
        self.state = LocalUnlockFlowState::Locked;
    }
}
