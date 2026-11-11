use crate::{
    PermissionKind, PermissionOperation, PermissionPrompt, PermissionPromptCompletion,
    PermissionProvider, PermissionSnapshot, PermissionStatus, SettingsHandoff, SettingsTarget,
    invalid_configuration,
};
use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Support,
    SystemError,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
};

struct PendingPrompt {
    prompt: PermissionPrompt,
    completion: PermissionPromptCompletion,
}

#[derive(Default)]
struct FakeState {
    statuses: BTreeMap<PermissionKind, PermissionStatus>,
    configuration_errors: BTreeMap<PermissionKind, SystemError>,
    settings_errors: BTreeMap<SettingsTarget, SystemError>,
    settings_handoffs: BTreeMap<SettingsTarget, SettingsHandoff>,
    pending: VecDeque<PendingPrompt>,
    auto_complete: Option<PermissionStatus>,
    revision: u64,
    requested: Vec<PermissionKind>,
    opened: Vec<SettingsTarget>,
}

/// Deterministic, UI-local provider for headless tests and prototypes.
///
/// It never contacts a native platform. Status changes and prompt completion
/// are explicit, so tests can reproduce denial, settings recovery, resume and
/// generation races without sleeping.
#[derive(Default)]
pub struct FakePermissionProvider {
    state: RefCell<FakeState>,
}

impl FakePermissionProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_status(self, kind: PermissionKind, status: PermissionStatus) -> Self {
        self.set_status(kind, status);
        self
    }

    pub fn with_configuration_error(self, kind: PermissionKind) -> Self {
        self.set_configuration_error(
            kind,
            invalid_configuration("Native permission configuration is missing"),
        );
        self
    }

    pub fn with_settings_error(self, target: SettingsTarget) -> Self {
        self.set_settings_error(
            target,
            invalid_configuration("Settings destination is not configured"),
        );
        self
    }

    pub fn with_settings_handoff(self, target: SettingsTarget, handoff: SettingsHandoff) -> Self {
        self.set_settings_handoff(target, handoff);
        self
    }

    /// Simulate an OS settings change or a provider-side status transition.
    /// This never emits a service event by itself; call `reconcile_on_resume`.
    pub fn set_status(&self, kind: PermissionKind, status: PermissionStatus) {
        let mut state = self.state.borrow_mut();
        state.statuses.insert(kind, status);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_configuration_error(&self, kind: PermissionKind, error: SystemError) {
        self.state
            .borrow_mut()
            .configuration_errors
            .insert(kind, error);
    }

    pub fn clear_configuration_error(&self, kind: PermissionKind) {
        self.state.borrow_mut().configuration_errors.remove(&kind);
    }

    pub fn set_settings_error(&self, target: SettingsTarget, error: SystemError) {
        self.state
            .borrow_mut()
            .settings_errors
            .insert(target, error);
    }

    pub fn set_settings_handoff(&self, target: SettingsTarget, handoff: SettingsHandoff) {
        self.state
            .borrow_mut()
            .settings_handoffs
            .insert(target, handoff);
    }

    pub fn set_auto_complete(&self, status: Option<PermissionStatus>) {
        self.state.borrow_mut().auto_complete = status;
    }

    pub fn current_status(&self, kind: PermissionKind) -> PermissionStatus {
        self.query(kind)
            .map_or(PermissionStatus::Unavailable, |snapshot| snapshot.status)
    }

    pub fn pending_prompts(&self) -> Vec<PermissionPrompt> {
        self.state
            .borrow()
            .pending
            .iter()
            .map(|pending| pending.prompt)
            .collect()
    }

    pub fn pending_prompt_count(&self) -> usize {
        self.state.borrow().pending.len()
    }

    /// Clone the oldest pending completion without removing it. Useful for
    /// duplicate/late callback tests.
    pub fn next_completion(&self) -> Option<PermissionPromptCompletion> {
        self.state
            .borrow()
            .pending
            .front()
            .map(|pending| pending.completion.clone())
    }

    pub fn requested_kinds(&self) -> Vec<PermissionKind> {
        self.state.borrow().requested.clone()
    }

    pub fn opened_targets(&self) -> Vec<SettingsTarget> {
        self.state.borrow().opened.clone()
    }

    pub fn complete_next(&self, status: PermissionStatus) -> bool {
        let (completion, snapshot) = {
            let mut state = self.state.borrow_mut();
            let Some(pending) = state.pending.pop_front() else {
                return false;
            };
            state.statuses.insert(pending.prompt.kind, status);
            state.revision = state.revision.saturating_add(1);
            let snapshot = PermissionSnapshot::new(pending.prompt.kind, status, state.revision);
            (pending.completion, snapshot)
        };
        completion.complete(snapshot)
    }

    pub fn deny_next(&self, can_request_again: bool) -> bool {
        self.complete_next(PermissionStatus::Denied { can_request_again })
    }

    pub fn fail_next(&self, error: SystemError) -> bool {
        let completion = {
            let mut state = self.state.borrow_mut();
            state.pending.pop_front().map(|pending| pending.completion)
        };
        completion.is_some_and(|completion| completion.fail(error))
    }
}

impl PermissionProvider for FakePermissionProvider {
    fn capability(&self, operation: PermissionOperation) -> Capability<PermissionOperation> {
        let interactive = operation.is_interactive();
        let state = self.state.borrow();
        let (support, availability, configuration, permission, revision, constraints) =
            match operation {
                PermissionOperation::Query(kind) | PermissionOperation::Request(kind) => {
                    let configuration_error = state.configuration_errors.contains_key(&kind);
                    let status = state
                        .statuses
                        .get(&kind)
                        .copied()
                        .unwrap_or(PermissionStatus::NotDetermined);
                    let availability = if status == PermissionStatus::Unavailable {
                        Availability::Unavailable
                    } else {
                        Availability::Available
                    };
                    let configuration = if configuration_error {
                        Configuration::Required
                    } else {
                        Configuration::Ready
                    };
                    let support = if configuration_error {
                        Support::Conditional
                    } else {
                        Support::Native
                    };
                    (
                        support,
                        availability,
                        configuration,
                        status.permission_state(),
                        state.revision,
                        if interactive {
                            vec![Constraint::ForegroundOwner]
                        } else {
                            Vec::new()
                        },
                    )
                }
                PermissionOperation::OpenSettings(target) => {
                    let unavailable =
                        state.settings_handoffs.get(&target) == Some(&SettingsHandoff::Unavailable);
                    let configuration_error = state.settings_errors.contains_key(&target);
                    let configuration = if configuration_error {
                        Configuration::Required
                    } else {
                        Configuration::Ready
                    };
                    let support = if configuration_error {
                        Support::Conditional
                    } else {
                        Support::Native
                    };
                    (
                        support,
                        if unavailable {
                            Availability::Unavailable
                        } else {
                            Availability::Available
                        },
                        configuration,
                        PermissionState::NotRequired,
                        state.revision,
                        Vec::new(),
                    )
                }
            };
        Capability {
            operation,
            support,
            availability,
            configuration,
            permission,
            evidence: Evidence::ContractOnly,
            constraints,
            observed_revision: revision,
        }
    }

    fn query(&self, kind: PermissionKind) -> Result<PermissionSnapshot, SystemError> {
        let state = self.state.borrow();
        if let Some(error) = state.configuration_errors.get(&kind) {
            return Err(error.clone());
        }
        let status = state
            .statuses
            .get(&kind)
            .copied()
            .unwrap_or(PermissionStatus::NotDetermined);
        Ok(PermissionSnapshot::new(kind, status, state.revision))
    }

    fn request_permission(
        &self,
        prompt: PermissionPrompt,
        completion: PermissionPromptCompletion,
    ) -> Result<(), SystemError> {
        let auto = self.state.borrow().auto_complete;
        {
            let mut state = self.state.borrow_mut();
            state.requested.push(prompt.kind);
            if auto.is_none() {
                state.pending.push_back(PendingPrompt {
                    prompt,
                    completion: completion.clone(),
                });
            }
        }
        if let Some(status) = auto {
            self.set_status(prompt.kind, status);
            let snapshot = self.query(prompt.kind)?;
            completion.complete(snapshot);
        }
        Ok(())
    }

    fn open_settings(&self, target: SettingsTarget) -> Result<SettingsHandoff, SystemError> {
        let mut state = self.state.borrow_mut();
        state.opened.push(target);
        if let Some(error) = state.settings_errors.get(&target) {
            return Err(error.clone());
        }
        Ok(state
            .settings_handoffs
            .get(&target)
            .copied()
            .unwrap_or(SettingsHandoff::Opened))
    }

    fn tracked_kinds(&self) -> Vec<PermissionKind> {
        let state = self.state.borrow();
        let mut kinds: BTreeSet<PermissionKind> = state.statuses.keys().copied().collect();
        kinds.extend(state.configuration_errors.keys().copied());
        kinds.into_iter().collect()
    }
}
