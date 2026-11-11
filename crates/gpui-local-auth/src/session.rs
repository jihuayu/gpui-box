//! Application-owned, account-scoped unlock policy and receipt validation.

use crate::model::invalid_input;
use crate::{
    AccountId, ErrorCode, LocalAuthHostState, LocalVerificationReceipt, RecoveryAction,
    RetryAdvice, SystemError, VerifiedFactor,
};
use gpui_lifecycle::LifecycleState;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

/// Application policy applied after a successful local verification.
///
/// The policy is owned by the application, never by the provider. A receipt is
/// accepted only once and only while it is fresh enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlockPolicy {
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub max_receipt_age: Duration,
    pub relock_on_background: bool,
    pub relock_on_session_lock: bool,
    pub relock_on_account_switch: bool,
    pub allow_device_credential_fallback: bool,
}

impl Default for UnlockPolicy {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(3600),
            max_receipt_age: Duration::from_secs(120),
            relock_on_background: true,
            relock_on_session_lock: true,
            relock_on_account_switch: true,
            allow_device_credential_fallback: true,
        }
    }
}

impl UnlockPolicy {
    /// Build a policy with the default relock flags while validating durations.
    pub fn new(
        idle_timeout: Duration,
        max_lifetime: Duration,
        max_receipt_age: Duration,
    ) -> Result<Self, SystemError> {
        let policy = Self {
            idle_timeout,
            max_lifetime,
            max_receipt_age,
            ..Self::default()
        };
        policy.validate()?;
        Ok(policy)
    }

    /// Durations must be non-zero and the idle timeout may not outlive the
    /// maximum unlock lifetime.
    pub fn validate(&self) -> Result<(), SystemError> {
        if self.idle_timeout.is_zero()
            || self.max_lifetime.is_zero()
            || self.max_receipt_age.is_zero()
        {
            return Err(invalid_input("Unlock policy durations must be non-zero"));
        }
        if self.idle_timeout > self.max_lifetime {
            return Err(invalid_input(
                "Unlock idle timeout cannot exceed its maximum lifetime",
            ));
        }
        Ok(())
    }
}

/// Why an account is currently locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockReason {
    NeverUnlocked,
    IdleTimeout,
    MaxLifetime,
    ExplicitLock,
    AppBackgrounded,
    SessionLocked,
    AccountSwitched,
    Invalidated,
}

/// The current state of one account's unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountUnlockStatus {
    Unlocked {
        verified_at: Instant,
        expires_at: Instant,
        remaining: Duration,
        factor: VerifiedFactor,
    },
    Locked {
        reason: LockReason,
    },
}

/// A successful account unlock grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnlockGrant {
    pub verified_at: Instant,
    pub expires_at: Instant,
    pub factor: VerifiedFactor,
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ReceiptFingerprint {
    surface: u64,
    generation: u64,
    revision: u64,
}

impl From<&LocalVerificationReceipt> for ReceiptFingerprint {
    fn from(receipt: &LocalVerificationReceipt) -> Self {
        Self {
            surface: receipt.surface().0,
            generation: receipt.generation(),
            revision: receipt.revision(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UnlockState {
    verified_at: Instant,
    idle_expires_at: Instant,
    max_expires_at: Instant,
    factor: VerifiedFactor,
}

#[derive(Debug, Clone, Copy)]
struct AccountEntry {
    state: Option<UnlockState>,
    reason: LockReason,
}

impl AccountEntry {
    const fn locked(reason: LockReason) -> Self {
        Self {
            state: None,
            reason,
        }
    }
}

/// Account-scoped unlock tracking for one application.
#[derive(Debug, Clone, Default)]
pub struct AccountUnlockManager {
    policy: UnlockPolicy,
    active: Option<AccountId>,
    accounts: BTreeMap<AccountId, AccountEntry>,
    used_receipts: BTreeSet<ReceiptFingerprint>,
}

impl AccountUnlockManager {
    pub fn new(policy: UnlockPolicy) -> Result<Self, SystemError> {
        policy.validate()?;
        Ok(Self {
            policy,
            active: None,
            accounts: BTreeMap::new(),
            used_receipts: BTreeSet::new(),
        })
    }

    pub fn policy(&self) -> &UnlockPolicy {
        &self.policy
    }

    pub fn active_account(&self) -> Option<&AccountId> {
        self.active.as_ref()
    }

    /// Track the active account. Switching accounts relocks the previous one
    /// according to [`UnlockPolicy::relock_on_account_switch`].
    pub fn set_active_account(&mut self, account: Option<AccountId>) {
        let previous = self.active.clone();
        if self.policy.relock_on_account_switch
            && let Some(previous) = previous.filter(|previous| Some(previous) != account.as_ref())
        {
            self.lock_account_with(&previous, LockReason::AccountSwitched);
        }
        self.active = account;
    }

    /// Validate and consume a verification receipt.
    ///
    /// A receipt is rejected when it is from the future, older than
    /// [`UnlockPolicy::max_receipt_age`], already used for this service or a
    /// device-credential fallback when the policy forbids fallback.
    pub fn grant_unlock(
        &mut self,
        account: &AccountId,
        receipt: &LocalVerificationReceipt,
        now: Instant,
    ) -> Result<UnlockGrant, SystemError> {
        if receipt.factor() == VerifiedFactor::DeviceCredentialFallback
            && !self.policy.allow_device_credential_fallback
        {
            let mut error = SystemError::new(
                ErrorCode::PermissionDenied,
                "Device-credential fallback is disabled by the unlock policy",
            );
            error.recovery = Some(RecoveryAction::Authenticate);
            error.retry = RetryAdvice::AfterConditionsChange;
            return Err(error);
        }
        let Some(age) = now.checked_duration_since(receipt.verified_at()) else {
            return Err(invalid_receipt(
                "Verification receipt is not yet valid at this instant",
            ));
        };
        if age > self.policy.max_receipt_age {
            return Err(stale_receipt("Verification receipt is too old for unlock"));
        }
        let fingerprint = ReceiptFingerprint::from(receipt);
        if self.used_receipts.contains(&fingerprint) {
            return Err(stale_receipt("Verification receipt was already consumed"));
        }
        let Some(idle_expires_at) = now.checked_add(self.policy.idle_timeout) else {
            return Err(invalid_receipt("Unlock expiry is out of range"));
        };
        let Some(max_expires_at) = receipt.verified_at().checked_add(self.policy.max_lifetime)
        else {
            return Err(invalid_receipt("Unlock expiry is out of range"));
        };
        if max_expires_at <= now {
            return Err(stale_receipt(
                "Verification receipt exceeded the maximum unlock lifetime",
            ));
        }
        let expires_at = idle_expires_at.min(max_expires_at);
        self.used_receipts.insert(fingerprint);
        self.accounts.insert(
            account.clone(),
            AccountEntry {
                state: Some(UnlockState {
                    verified_at: receipt.verified_at(),
                    idle_expires_at,
                    max_expires_at,
                    factor: receipt.factor(),
                }),
                reason: LockReason::NeverUnlocked,
            },
        );
        Ok(UnlockGrant {
            verified_at: receipt.verified_at(),
            expires_at,
            factor: receipt.factor(),
            generation: receipt.generation(),
            revision: receipt.revision(),
        })
    }

    /// Resolve the current status, recording an idle or lifetime expiry.
    pub fn status(&mut self, account: &AccountId, now: Instant) -> AccountUnlockStatus {
        let Some(entry) = self.accounts.get_mut(account) else {
            return AccountUnlockStatus::Locked {
                reason: LockReason::NeverUnlocked,
            };
        };
        let Some(state) = entry.state else {
            return AccountUnlockStatus::Locked {
                reason: entry.reason,
            };
        };
        let reason = if now >= state.max_expires_at {
            Some(LockReason::MaxLifetime)
        } else if now >= state.idle_expires_at {
            Some(LockReason::IdleTimeout)
        } else {
            None
        };
        if let Some(reason) = reason {
            entry.state = None;
            entry.reason = reason;
            return AccountUnlockStatus::Locked { reason };
        }
        let expires_at = state.idle_expires_at.min(state.max_expires_at);
        let remaining = expires_at
            .checked_duration_since(now)
            .unwrap_or(Duration::ZERO);
        AccountUnlockStatus::Unlocked {
            verified_at: state.verified_at,
            expires_at,
            remaining,
            factor: state.factor,
        }
    }

    /// Record user activity. The idle expiry moves forward but is always capped
    /// by the receipt's maximum lifetime and never revives a locked session.
    pub fn touch_activity(&mut self, account: &AccountId, now: Instant) -> AccountUnlockStatus {
        let current = self.status(account, now);
        if !matches!(current, AccountUnlockStatus::Unlocked { .. }) {
            return current;
        }
        let idle_timeout = self.policy.idle_timeout;
        if let Some(entry) = self.accounts.get_mut(account)
            && let Some(state) = entry.state.as_mut()
        {
            let candidate = now
                .checked_add(idle_timeout)
                .unwrap_or(state.max_expires_at)
                .min(state.max_expires_at);
            if candidate > state.idle_expires_at {
                state.idle_expires_at = candidate;
            }
        }
        self.status(account, now)
    }

    /// Explicitly lock one account.
    pub fn lock_account(&mut self, account: &AccountId) {
        self.lock_account_with(account, LockReason::ExplicitLock);
    }

    /// Explicitly lock every tracked account.
    pub fn lock_all(&mut self) {
        self.lock_all_with(LockReason::ExplicitLock);
    }

    /// Re-enrollment invalidated every previously protected key or secret.
    pub fn invalidate_enrollment(&mut self) {
        self.lock_all_with(LockReason::Invalidated);
    }

    /// Re-enrollment invalidated one account's protected material.
    pub fn invalidate_account(&mut self, account: &AccountId) {
        self.lock_account_with(account, LockReason::Invalidated);
    }

    /// Relock according to background and session-lock policy, then expire any
    /// remaining idle or lifetime grants at `now`.
    pub fn reconcile_host_state(&mut self, state: &LocalAuthHostState, now: Instant) {
        if self.policy.relock_on_session_lock && state.is_session_locked() {
            self.lock_all_with(LockReason::SessionLocked);
        } else if self.policy.relock_on_background && !state.is_foreground() {
            self.lock_all_with(LockReason::AppBackgrounded);
        }
        let accounts: Vec<AccountId> = self.accounts.keys().cloned().collect();
        for account in accounts {
            let _ = self.status(&account, now);
        }
    }

    /// Reconcile a merged `gpui-lifecycle` state against this policy.
    pub fn reconcile_lifecycle(&mut self, state: &LifecycleState, now: Instant) {
        let host = LocalAuthHostState::from_lifecycle(state);
        self.reconcile_host_state(&host, now);
    }

    /// Audit whether a receipt fingerprint has already been consumed.
    pub fn receipt_consumed(&self, receipt: &LocalVerificationReceipt) -> bool {
        self.used_receipts
            .contains(&ReceiptFingerprint::from(receipt))
    }

    fn lock_account_with(&mut self, account: &AccountId, reason: LockReason) {
        self.accounts
            .insert(account.clone(), AccountEntry::locked(reason));
    }

    fn lock_all_with(&mut self, reason: LockReason) {
        for entry in self.accounts.values_mut() {
            entry.state = None;
            entry.reason = reason;
        }
    }
}

fn invalid_receipt(message: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::InvalidInput, message);
    error.retry = RetryAdvice::ModifyRequest;
    error
}

fn stale_receipt(message: &'static str) -> SystemError {
    let mut error = SystemError::new(ErrorCode::ResourceExpired, message);
    error.recovery = Some(RecoveryAction::Authenticate);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}
