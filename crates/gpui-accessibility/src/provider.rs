//! Provider contract and a deterministic cross-platform test double.

use crate::evidence::{AssistiveReaderSessionEvidence, AutomationEvidence};
use crate::live::LiveAnnouncement;
use crate::platform::{AccessibilityOperation, AccessibilityPlatform};
use crate::scene::{
    AccessibilityActionReceipt, SemanticActionRequest, SemanticTreeSnapshot, TextSelectionReceipt,
    TextSelectionRequest,
};
use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Support,
    SystemError,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// The native/adaptor surface the service delegates to. Implementations must
/// not block or run native UI work off the owning UI thread.
pub trait AccessibilityProvider {
    fn platform(&self) -> AccessibilityPlatform;

    /// A point-in-time capability snapshot. The service rechecks it before every
    /// operation; a stale snapshot never authorizes work.
    fn capability(&self, operation: AccessibilityOperation) -> Capability<AccessibilityOperation>;

    /// Called after the service accepted a semantic revision. Native adapters
    /// forward the snapshot to their platform bridge here.
    fn commit_snapshot(&self, _snapshot: &SemanticTreeSnapshot) -> Result<(), SystemError> {
        Ok(())
    }

    fn perform_action(
        &self,
        request: &SemanticActionRequest,
        snapshot: &SemanticTreeSnapshot,
    ) -> Result<AccessibilityActionReceipt, SystemError>;

    fn edit_text_selection(
        &self,
        request: &TextSelectionRequest,
        snapshot: &SemanticTreeSnapshot,
    ) -> Result<TextSelectionReceipt, SystemError>;

    fn announce_live(&self, announcement: &LiveAnnouncement) -> Result<(), SystemError>;

    fn record_automation_evidence(
        &self,
        _evidence: &AutomationEvidence,
    ) -> Result<(), SystemError> {
        Ok(())
    }

    fn record_assistive_reader_evidence(
        &self,
        _evidence: &AssistiveReaderSessionEvidence,
    ) -> Result<(), SystemError> {
        Ok(())
    }
}

struct FakeState {
    capabilities: BTreeMap<AccessibilityOperation, Capability<AccessibilityOperation>>,
    committed: Vec<SemanticTreeSnapshot>,
    actions: Vec<SemanticActionRequest>,
    selections: Vec<TextSelectionRequest>,
    announcements: Vec<LiveAnnouncement>,
    automation: Vec<AutomationEvidence>,
    assistive_reader: Vec<AssistiveReaderSessionEvidence>,
    action_error: Option<SystemError>,
    selection_error: Option<SystemError>,
    announce_error: Option<SystemError>,
    commit_error: Option<SystemError>,
}

/// Deterministic provider for headless tests and prototypes. It never contacts
/// a native platform and never claims runtime accessibility support.
pub struct FakeAccessibilityProvider {
    platform: AccessibilityPlatform,
    state: RefCell<FakeState>,
}

impl Default for FakeAccessibilityProvider {
    fn default() -> Self {
        Self::new(AccessibilityPlatform::Windows)
    }
}

impl FakeAccessibilityProvider {
    pub fn new(platform: AccessibilityPlatform) -> Self {
        let capabilities = AccessibilityOperation::ALL
            .into_iter()
            .map(|operation| (operation, default_capability(platform, operation)))
            .collect();
        Self {
            platform,
            state: RefCell::new(FakeState {
                capabilities,
                committed: Vec::new(),
                actions: Vec::new(),
                selections: Vec::new(),
                announcements: Vec::new(),
                automation: Vec::new(),
                assistive_reader: Vec::new(),
                action_error: None,
                selection_error: None,
                announce_error: None,
                commit_error: None,
            }),
        }
    }

    pub fn platform(&self) -> AccessibilityPlatform {
        self.platform
    }

    pub fn with_capability(
        self,
        operation: AccessibilityOperation,
        capability: Capability<AccessibilityOperation>,
    ) -> Self {
        self.set_capability(operation, capability);
        self
    }

    pub fn set_capability(
        &self,
        operation: AccessibilityOperation,
        capability: Capability<AccessibilityOperation>,
    ) {
        self.state
            .borrow_mut()
            .capabilities
            .insert(operation, capability);
    }

    pub fn capability_for(
        &self,
        operation: AccessibilityOperation,
    ) -> Capability<AccessibilityOperation> {
        self.state
            .borrow()
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| unsupported_capability(self.platform, operation))
    }

    pub fn set_support(&self, operation: AccessibilityOperation, support: Support) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
        }
    }

    pub fn set_availability(&self, operation: AccessibilityOperation, availability: Availability) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.availability = availability;
        }
    }

    pub fn set_configuration(
        &self,
        operation: AccessibilityOperation,
        configuration: Configuration,
    ) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.configuration = configuration;
        }
    }

    pub fn set_permission(&self, operation: AccessibilityOperation, permission: PermissionState) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.permission = permission;
        }
    }

    pub fn set_operation_unsupported(&self, operation: AccessibilityOperation) {
        self.set_support(operation, Support::Unsupported);
        self.set_availability(operation, Availability::Unknown);
    }

    pub fn set_operation_unavailable(&self, operation: AccessibilityOperation) {
        self.set_support(operation, Support::Conditional);
        self.set_availability(operation, Availability::Unavailable);
    }

    pub fn set_operation_configuration_required(&self, operation: AccessibilityOperation) {
        self.set_support(operation, Support::Conditional);
        self.set_configuration(operation, Configuration::Required);
    }

    pub fn set_action_error(&self, error: Option<SystemError>) {
        self.state.borrow_mut().action_error = error;
    }

    pub fn set_selection_error(&self, error: Option<SystemError>) {
        self.state.borrow_mut().selection_error = error;
    }

    pub fn set_announce_error(&self, error: Option<SystemError>) {
        self.state.borrow_mut().announce_error = error;
    }

    pub fn set_commit_error(&self, error: Option<SystemError>) {
        self.state.borrow_mut().commit_error = error;
    }

    pub fn committed_scenes(&self) -> Vec<SemanticTreeSnapshot> {
        self.state.borrow().committed.clone()
    }

    pub fn last_committed_scene(&self) -> Option<SemanticTreeSnapshot> {
        self.state.borrow().committed.last().cloned()
    }

    pub fn dispatched_actions(&self) -> Vec<SemanticActionRequest> {
        self.state.borrow().actions.clone()
    }

    pub fn recorded_selections(&self) -> Vec<TextSelectionRequest> {
        self.state.borrow().selections.clone()
    }

    pub fn recorded_announcements(&self) -> Vec<LiveAnnouncement> {
        self.state.borrow().announcements.clone()
    }

    pub fn automation_evidence(&self) -> Vec<AutomationEvidence> {
        self.state.borrow().automation.clone()
    }

    pub fn assistive_reader_evidence(&self) -> Vec<AssistiveReaderSessionEvidence> {
        self.state.borrow().assistive_reader.clone()
    }

    pub fn clear_recordings(&self) {
        let mut state = self.state.borrow_mut();
        state.committed.clear();
        state.actions.clear();
        state.selections.clear();
        state.announcements.clear();
        state.automation.clear();
        state.assistive_reader.clear();
    }
}

impl AccessibilityProvider for FakeAccessibilityProvider {
    fn platform(&self) -> AccessibilityPlatform {
        self.platform
    }

    fn capability(&self, operation: AccessibilityOperation) -> Capability<AccessibilityOperation> {
        self.capability_for(operation)
    }

    fn commit_snapshot(&self, snapshot: &SemanticTreeSnapshot) -> Result<(), SystemError> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.commit_error {
            return Err(error.clone());
        }
        state.committed.push(snapshot.clone());
        Ok(())
    }

    fn perform_action(
        &self,
        request: &SemanticActionRequest,
        _snapshot: &SemanticTreeSnapshot,
    ) -> Result<AccessibilityActionReceipt, SystemError> {
        request.validate().map_err(SystemError::from)?;
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.action_error {
            return Err(error.clone());
        }
        state.actions.push(request.clone());
        Ok(AccessibilityActionReceipt {
            scene: request.scene,
            target: request.target.clone(),
            action: request.action,
            accepted: true,
        })
    }

    fn edit_text_selection(
        &self,
        request: &TextSelectionRequest,
        _snapshot: &SemanticTreeSnapshot,
    ) -> Result<TextSelectionReceipt, SystemError> {
        request.validate()?;
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.selection_error {
            return Err(error.clone());
        }
        let receipt = TextSelectionReceipt::from_request(request)?;
        state.selections.push(request.clone());
        Ok(receipt)
    }

    fn announce_live(&self, announcement: &LiveAnnouncement) -> Result<(), SystemError> {
        let mut state = self.state.borrow_mut();
        if let Some(error) = &state.announce_error {
            return Err(error.clone());
        }
        state.announcements.push(announcement.clone());
        Ok(())
    }

    fn record_automation_evidence(&self, evidence: &AutomationEvidence) -> Result<(), SystemError> {
        evidence.validate()?;
        self.state.borrow_mut().automation.push(evidence.clone());
        Ok(())
    }

    fn record_assistive_reader_evidence(
        &self,
        evidence: &AssistiveReaderSessionEvidence,
    ) -> Result<(), SystemError> {
        evidence.validate()?;
        self.state
            .borrow_mut()
            .assistive_reader
            .push(evidence.clone());
        Ok(())
    }
}

fn default_capability(
    platform: AccessibilityPlatform,
    operation: AccessibilityOperation,
) -> Capability<AccessibilityOperation> {
    Capability {
        operation,
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: vec![Constraint::Provider(platform.bridge().label())],
        observed_revision: 0,
    }
}

fn unsupported_capability(
    platform: AccessibilityPlatform,
    operation: AccessibilityOperation,
) -> Capability<AccessibilityOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: Availability::Unknown,
        configuration: Configuration::Unknown,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: vec![Constraint::Provider(platform.bridge().label())],
        observed_revision: 0,
    }
}

/// Convenience registration value for tests that want shared ownership.
pub fn fake_provider(platform: AccessibilityPlatform) -> Rc<FakeAccessibilityProvider> {
    Rc::new(FakeAccessibilityProvider::new(platform))
}
