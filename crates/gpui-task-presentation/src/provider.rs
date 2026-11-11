//! Explicit task-presentation provider boundary plus honest unavailable and
//! deterministic fake providers.

use crate::{
    AcknowledgmentPrecision, AttentionRequest, BadgeValue, TaskActionButton,
    TaskPresentationOperation, TaskProgressState,
};
use gpui_system::{
    Availability, Capability, Configuration, Constraint, ErrorCode, Evidence, PermissionState,
    ProviderManifest, Support, SurfaceId, SystemError,
};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    rc::Rc,
};

/// Conservative manifest: this contract crate installs no native provider.
pub const PROVIDER_MANIFEST: [ProviderManifest; 5] = gpui_system::PROVIDER_MANIFEST;

/// Explicit native/provider boundary for taskbar, Dock, badge, attention and
/// thumbbar presentation.
///
/// Implementations are UI-local and must not block. A provider reports what it
/// actually did; it never claims `AppliedConfirmed` for a daemon handoff.
pub trait TaskPresentationProvider {
    fn capability(
        &self,
        operation: TaskPresentationOperation,
        surface: SurfaceId,
    ) -> Capability<TaskPresentationOperation>;

    fn capabilities(&self, surface: SurfaceId) -> Vec<Capability<TaskPresentationOperation>> {
        TaskPresentationOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation, surface))
            .collect()
    }

    /// Apply (or clear with `None`) one surface's progress/state indicator.
    fn apply_surface_progress(
        &self,
        _surface: SurfaceId,
        _generation: u64,
        _progress: Option<&TaskProgressState>,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Task progress presentation is not supported by this provider",
        ))
    }

    fn clear_surface_progress(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        self.apply_surface_progress(surface, generation, None)
            .map(|_| ())
    }

    fn apply_app_badge(&self, _badge: &BadgeValue) -> Result<AcknowledgmentPrecision, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Application badges are not supported by this provider",
        ))
    }

    fn clear_app_badge(&self) -> Result<(), SystemError> {
        Ok(())
    }

    fn apply_surface_badge(
        &self,
        _surface: SurfaceId,
        _generation: u64,
        _badge: &BadgeValue,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Surface badges are not supported by this provider",
        ))
    }

    fn clear_surface_badge(
        &self,
        _surface: SurfaceId,
        _generation: u64,
    ) -> Result<(), SystemError> {
        Ok(())
    }

    fn request_attention(
        &self,
        _request: &AttentionRequest,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "User attention requests are not supported by this provider",
        ))
    }

    fn clear_attention(&self, _surface: SurfaceId, _generation: u64) -> Result<(), SystemError> {
        Ok(())
    }

    fn apply_actions(
        &self,
        _surface: SurfaceId,
        _generation: u64,
        _actions: &[TaskActionButton],
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Thumbbar actions are not supported by this provider",
        ))
    }

    fn clear_actions(&self, _surface: SurfaceId, _generation: u64) -> Result<(), SystemError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any;
}

fn unsupported_capability(
    operation: TaskPresentationOperation,
) -> Capability<TaskPresentationOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: Availability::Unavailable,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: 0,
    }
}

/// A contract-only provider used when no native host adapter is installed.
///
/// Every operation is unsupported and unavailable. It never fabricates a
/// confirmation for progress, badges, attention or actions.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedTaskPresentationProvider;

impl UnsupportedTaskPresentationProvider {
    pub const fn new() -> Self {
        Self
    }
}

impl TaskPresentationProvider for UnsupportedTaskPresentationProvider {
    fn capability(
        &self,
        operation: TaskPresentationOperation,
        _surface: SurfaceId,
    ) -> Capability<TaskPresentationOperation> {
        unsupported_capability(operation)
    }

    fn clear_app_badge(&self) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Application badges are not supported by this provider",
        ))
    }

    fn clear_surface_badge(
        &self,
        _surface: SurfaceId,
        _generation: u64,
    ) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Surface badges are not supported by this provider",
        ))
    }

    fn clear_attention(&self, _surface: SurfaceId, _generation: u64) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "User attention requests are not supported by this provider",
        ))
    }

    fn clear_actions(&self, _surface: SurfaceId, _generation: u64) -> Result<(), SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Thumbbar actions are not supported by this provider",
        ))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn baseline_capability(
    operation: TaskPresentationOperation,
) -> Capability<TaskPresentationOperation> {
    Capability {
        operation,
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::<Constraint>::new(),
        observed_revision: 1,
    }
}

/// A recorded progress apply/clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressApplyRecord {
    pub surface: SurfaceId,
    pub generation: u64,
    pub progress: Option<TaskProgressState>,
    pub precision: AcknowledgmentPrecision,
}

/// A recorded badge apply/clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadgeApplyRecord {
    pub surface: Option<SurfaceId>,
    pub generation: Option<u64>,
    pub badge: Option<BadgeValue>,
    pub precision: AcknowledgmentPrecision,
}

/// A recorded attention request/clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionApplyRecord {
    pub request: Option<AttentionRequest>,
    pub cleared_surface: Option<(SurfaceId, u64)>,
    pub precision: AcknowledgmentPrecision,
}

/// A recorded thumbbar action apply/clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionApplyRecord {
    pub surface: SurfaceId,
    pub generation: u64,
    pub actions: Vec<TaskActionButton>,
    pub precision: AcknowledgmentPrecision,
}

/// A simulated user click on one provider-rendered thumbbar action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulatedActionClick {
    pub surface: SurfaceId,
    pub action_id: String,
}

struct FakeState {
    capabilities: BTreeMap<TaskPresentationOperation, Capability<TaskPresentationOperation>>,
    surface_capabilities:
        HashMap<(u64, TaskPresentationOperation), Capability<TaskPresentationOperation>>,
    precisions: HashMap<(u64, TaskPresentationOperation), AcknowledgmentPrecision>,
    badge_downgrade_surfaces: HashSet<u64>,
    failures: HashMap<u64, SystemError>,
    progress_history: Vec<ProgressApplyRecord>,
    badge_history: Vec<BadgeApplyRecord>,
    attention_history: Vec<AttentionApplyRecord>,
    action_history: Vec<ActionApplyRecord>,
    progress_calls: HashMap<u64, usize>,
    badge_calls: HashMap<u64, usize>,
    attention_calls: HashMap<u64, usize>,
    action_calls: HashMap<u64, usize>,
    clicks: Vec<SimulatedActionClick>,
    revision: u64,
}

/// Deterministic UI-local provider for headless tests and examples.
///
/// The handle is `Clone`; state is shared through an `Rc<RefCell<_>>`, so a test
/// can inspect call history while the service keeps its own trait-object handle.
#[derive(Clone)]
pub struct FakeTaskPresentationProvider {
    state: Rc<RefCell<FakeState>>,
}

impl fmt::Debug for FakeTaskPresentationProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        formatter
            .debug_struct("FakeTaskPresentationProvider")
            .field("progress_calls", &state.progress_history.len())
            .field("badge_calls", &state.badge_history.len())
            .field("attention_calls", &state.attention_history.len())
            .field("action_calls", &state.action_history.len())
            .field("clicks", &state.clicks.len())
            .finish()
    }
}

impl Default for FakeTaskPresentationProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeTaskPresentationProvider {
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(FakeState {
                capabilities: TaskPresentationOperation::ALL
                    .into_iter()
                    .map(|operation| (operation, baseline_capability(operation)))
                    .collect(),
                surface_capabilities: HashMap::new(),
                precisions: HashMap::new(),
                badge_downgrade_surfaces: HashSet::new(),
                failures: HashMap::new(),
                progress_history: Vec::new(),
                badge_history: Vec::new(),
                attention_history: Vec::new(),
                action_history: Vec::new(),
                progress_calls: HashMap::new(),
                badge_calls: HashMap::new(),
                attention_calls: HashMap::new(),
                action_calls: HashMap::new(),
                clicks: Vec::new(),
                revision: 1,
            })),
        }
    }

    pub fn set_support(&self, operation: TaskPresentationOperation, support: Support) {
        let mut state = self.state.borrow_mut();
        let revision = state.revision.saturating_add(1);
        state.revision = revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
            capability.observed_revision = revision;
        }
    }

    pub fn set_availability(
        &self,
        operation: TaskPresentationOperation,
        availability: Availability,
    ) {
        let mut state = self.state.borrow_mut();
        let revision = state.revision.saturating_add(1);
        state.revision = revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.availability = availability;
            capability.observed_revision = revision;
        }
    }

    /// Mark one operation unsupported and unavailable.
    pub fn set_operation_unsupported(&self, operation: TaskPresentationOperation) {
        self.set_support(operation, Support::Unsupported);
        self.set_availability(operation, Availability::Unavailable);
    }

    pub fn set_permission(
        &self,
        operation: TaskPresentationOperation,
        permission: PermissionState,
    ) {
        let mut state = self.state.borrow_mut();
        let revision = state.revision.saturating_add(1);
        state.revision = revision;
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.permission = permission;
            capability.observed_revision = revision;
        }
    }

    pub fn set_surface_availability(
        &self,
        surface: SurfaceId,
        operation: TaskPresentationOperation,
        availability: Availability,
    ) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .surface_capabilities
            .get(&(surface.0, operation))
            .cloned()
            .unwrap_or_else(|| {
                state
                    .capabilities
                    .get(&operation)
                    .cloned()
                    .unwrap_or_else(|| baseline_capability(operation))
            });
        capability.availability = availability;
        state.revision = state.revision.saturating_add(1);
        capability.observed_revision = state.revision;
        state
            .surface_capabilities
            .insert((surface.0, operation), capability);
    }

    pub fn set_surface_permission(
        &self,
        surface: SurfaceId,
        operation: TaskPresentationOperation,
        permission: PermissionState,
    ) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .surface_capabilities
            .get(&(surface.0, operation))
            .cloned()
            .unwrap_or_else(|| {
                state
                    .capabilities
                    .get(&operation)
                    .cloned()
                    .unwrap_or_else(|| baseline_capability(operation))
            });
        capability.permission = permission;
        state.revision = state.revision.saturating_add(1);
        capability.observed_revision = state.revision;
        state
            .surface_capabilities
            .insert((surface.0, operation), capability);
    }

    pub fn deny_permission(&self, surface: SurfaceId, operation: TaskPresentationOperation) {
        self.set_surface_permission(surface, operation, PermissionState::Denied);
    }

    pub fn simulate_surface_loss(&self, surface: SurfaceId, operation: TaskPresentationOperation) {
        self.set_surface_availability(surface, operation, Availability::Unavailable);
    }

    /// Set the precision returned for successful applies on `surface`.
    pub fn set_precision(
        &self,
        operation: TaskPresentationOperation,
        surface: SurfaceId,
        precision: AcknowledgmentPrecision,
    ) {
        self.state
            .borrow_mut()
            .precisions
            .insert((surface.0, operation), precision);
    }

    /// Surface-first alias for [`Self::set_precision`].
    pub fn set_surface_precision(
        &self,
        surface: SurfaceId,
        operation: TaskPresentationOperation,
        precision: AcknowledgmentPrecision,
    ) {
        self.set_precision(operation, surface, precision);
    }

    /// A surface that only supports a boolean/dot badge.
    pub fn set_badge_downgrade(&self, surface: SurfaceId, enabled: bool) {
        let mut state = self.state.borrow_mut();
        if enabled {
            state.badge_downgrade_surfaces.insert(surface.0);
        } else {
            state.badge_downgrade_surfaces.remove(&surface.0);
        }
    }

    /// Force every apply on `surface` to fail with the supplied error.
    pub fn set_failure(&self, surface: SurfaceId, error: Option<SystemError>) {
        let mut state = self.state.borrow_mut();
        match error {
            Some(error) => {
                state.failures.insert(surface.0, error);
            }
            None => {
                state.failures.remove(&surface.0);
            }
        }
    }

    pub fn capabilities_for(
        &self,
        surface: SurfaceId,
    ) -> Vec<Capability<TaskPresentationOperation>> {
        TaskPresentationOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation, surface))
            .collect()
    }

    fn precision(
        &self,
        surface: SurfaceId,
        operation: TaskPresentationOperation,
        default: AcknowledgmentPrecision,
    ) -> AcknowledgmentPrecision {
        self.state
            .borrow()
            .precisions
            .get(&(surface.0, operation))
            .cloned()
            .unwrap_or(default)
    }

    pub fn progress_history(&self) -> Vec<ProgressApplyRecord> {
        self.state.borrow().progress_history.clone()
    }

    pub fn badge_history(&self) -> Vec<BadgeApplyRecord> {
        self.state.borrow().badge_history.clone()
    }

    pub fn attention_history(&self) -> Vec<AttentionApplyRecord> {
        self.state.borrow().attention_history.clone()
    }

    pub fn action_history(&self) -> Vec<ActionApplyRecord> {
        self.state.borrow().action_history.clone()
    }

    pub fn progress_calls(&self, surface: SurfaceId) -> usize {
        self.state
            .borrow()
            .progress_calls
            .get(&surface.0)
            .copied()
            .unwrap_or(0)
    }

    pub fn badge_calls(&self, surface: SurfaceId) -> usize {
        self.state
            .borrow()
            .badge_calls
            .get(&surface.0)
            .copied()
            .unwrap_or(0)
    }

    pub fn attention_calls(&self, surface: SurfaceId) -> usize {
        self.state
            .borrow()
            .attention_calls
            .get(&surface.0)
            .copied()
            .unwrap_or(0)
    }

    pub fn action_calls(&self, surface: SurfaceId) -> usize {
        self.state
            .borrow()
            .action_calls
            .get(&surface.0)
            .copied()
            .unwrap_or(0)
    }

    pub fn last_progress(&self, surface: SurfaceId) -> Option<ProgressApplyRecord> {
        self.state
            .borrow()
            .progress_history
            .iter()
            .rev()
            .find(|record| record.surface.0 == surface.0)
            .cloned()
    }

    /// Simulate a user thumbbar click. The service still performs all stale and
    /// state validation when the click is routed back.
    pub fn simulate_action_click(&self, surface: SurfaceId, action_id: impl Into<String>) {
        self.state.borrow_mut().clicks.push(SimulatedActionClick {
            surface,
            action_id: action_id.into(),
        });
    }

    pub fn clicked_actions(&self) -> Vec<SimulatedActionClick> {
        self.state.borrow().clicks.clone()
    }
}

impl TaskPresentationProvider for FakeTaskPresentationProvider {
    fn capability(
        &self,
        operation: TaskPresentationOperation,
        surface: SurfaceId,
    ) -> Capability<TaskPresentationOperation> {
        let state = self.state.borrow();
        state
            .surface_capabilities
            .get(&(surface.0, operation))
            .cloned()
            .or_else(|| state.capabilities.get(&operation).cloned())
            .unwrap_or_else(|| unsupported_capability(operation))
    }

    fn apply_surface_progress(
        &self,
        surface: SurfaceId,
        generation: u64,
        progress: Option<&TaskProgressState>,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        if let Some(error) = self.state.borrow().failures.get(&surface.0).cloned() {
            return Err(error);
        }
        let precision = self.precision(
            surface,
            TaskPresentationOperation::PresentTaskProgress,
            AcknowledgmentPrecision::AppliedConfirmed,
        );
        let mut state = self.state.borrow_mut();
        *state.progress_calls.entry(surface.0).or_default() += 1;
        state.progress_history.push(ProgressApplyRecord {
            surface,
            generation,
            progress: progress.cloned(),
            precision: precision.clone(),
        });
        Ok(precision)
    }

    fn clear_surface_progress(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        self.apply_surface_progress(surface, generation, None)?;
        Ok(())
    }

    fn apply_app_badge(&self, badge: &BadgeValue) -> Result<AcknowledgmentPrecision, SystemError> {
        self.apply_badge(None, None, badge)
    }

    fn clear_app_badge(&self) -> Result<(), SystemError> {
        self.apply_badge(None, None, &BadgeValue::Clear)?;
        Ok(())
    }

    fn apply_surface_badge(
        &self,
        surface: SurfaceId,
        generation: u64,
        badge: &BadgeValue,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        self.apply_badge(Some(surface), Some(generation), badge)
    }

    fn clear_surface_badge(&self, surface: SurfaceId, generation: u64) -> Result<(), SystemError> {
        self.apply_badge(Some(surface), Some(generation), &BadgeValue::Clear)?;
        Ok(())
    }

    fn request_attention(
        &self,
        request: &AttentionRequest,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        if let Some(error) = self
            .state
            .borrow()
            .failures
            .get(&request.surface.0)
            .cloned()
        {
            return Err(error);
        }
        let precision = self.precision(
            request.surface,
            TaskPresentationOperation::RequestUserAttention,
            AcknowledgmentPrecision::AppliedConfirmed,
        );
        let mut state = self.state.borrow_mut();
        *state.attention_calls.entry(request.surface.0).or_default() += 1;
        state.attention_history.push(AttentionApplyRecord {
            request: Some(request.clone()),
            cleared_surface: None,
            precision: precision.clone(),
        });
        Ok(precision)
    }

    fn clear_attention(&self, surface: SurfaceId, generation: u64) -> Result<(), SystemError> {
        let mut state = self.state.borrow_mut();
        *state.attention_calls.entry(surface.0).or_default() += 1;
        state.attention_history.push(AttentionApplyRecord {
            request: None,
            cleared_surface: Some((surface, generation)),
            precision: AcknowledgmentPrecision::AppliedConfirmed,
        });
        Ok(())
    }

    fn apply_actions(
        &self,
        surface: SurfaceId,
        generation: u64,
        actions: &[TaskActionButton],
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        if let Some(error) = self.state.borrow().failures.get(&surface.0).cloned() {
            return Err(error);
        }
        let precision = self.precision(
            surface,
            TaskPresentationOperation::InvokeTaskAction,
            AcknowledgmentPrecision::AppliedConfirmed,
        );
        let mut state = self.state.borrow_mut();
        *state.action_calls.entry(surface.0).or_default() += 1;
        state.action_history.push(ActionApplyRecord {
            surface,
            generation,
            actions: actions.to_vec(),
            precision: precision.clone(),
        });
        Ok(precision)
    }

    fn clear_actions(&self, surface: SurfaceId, generation: u64) -> Result<(), SystemError> {
        let mut state = self.state.borrow_mut();
        *state.action_calls.entry(surface.0).or_default() += 1;
        state.action_history.push(ActionApplyRecord {
            surface,
            generation,
            actions: Vec::new(),
            precision: AcknowledgmentPrecision::AppliedConfirmed,
        });
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl FakeTaskPresentationProvider {
    fn apply_badge(
        &self,
        surface: Option<SurfaceId>,
        generation: Option<u64>,
        badge: &BadgeValue,
    ) -> Result<AcknowledgmentPrecision, SystemError> {
        let surface_id = surface.unwrap_or(crate::APPLICATION_SURFACE);
        if let Some(error) = self.state.borrow().failures.get(&surface_id.0).cloned() {
            return Err(error);
        }
        let operation = TaskPresentationOperation::PresentAppBadge;
        let downgrade = surface.is_some_and(|surface| {
            self.state
                .borrow()
                .badge_downgrade_surfaces
                .contains(&surface.0)
        });
        let (applied, precision) = if downgrade {
            match badge.downgraded_to_dot() {
                Some(downgraded) => (
                    downgraded,
                    AcknowledgmentPrecision::DowngradedApplied {
                        note: "The surface only supports a boolean/dot badge".to_owned(),
                    },
                ),
                None => (
                    badge.clone(),
                    self.precision(
                        surface_id,
                        operation,
                        AcknowledgmentPrecision::AppliedConfirmed,
                    ),
                ),
            }
        } else {
            (
                badge.clone(),
                self.precision(
                    surface_id,
                    operation,
                    AcknowledgmentPrecision::AcceptedByHost,
                ),
            )
        };
        let mut state = self.state.borrow_mut();
        *state.badge_calls.entry(surface_id.0).or_default() += 1;
        state.badge_history.push(BadgeApplyRecord {
            surface,
            generation,
            badge: Some(applied),
            precision: precision.clone(),
        });
        Ok(precision)
    }
}
