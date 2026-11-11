//! Capability-gated menu provider contract and deterministic fake provider.

use crate::{
    Availability, Capability, Configuration, Constraint, ErrorCode, Evidence, LogicalRect,
    PermissionState, ResolvedContextMenu, ResolvedKeybinding, ResolvedMenuBar, Support, SurfaceId,
    SystemError,
};
use std::{cell::RefCell, collections::BTreeMap};

/// Operations exposed by a menu provider. All are snapshots; the service
/// re-validates at execution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MenusOperation {
    InstallMenuBar,
    PresentContextMenu,
    SyncKeybindings,
    QueryPalette,
    InvokeCommand,
}

impl MenusOperation {
    pub const ALL: [Self; 5] = [
        Self::InstallMenuBar,
        Self::PresentContextMenu,
        Self::SyncKeybindings,
        Self::QueryPalette,
        Self::InvokeCommand,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::InstallMenuBar => "Install menu bar",
            Self::PresentContextMenu => "Present context menu",
            Self::SyncKeybindings => "Sync keybindings",
            Self::QueryPalette => "Query palette",
            Self::InvokeCommand => "Invoke command",
        }
    }
}

/// Provider boundary for native or component menu presentation.
///
/// Providers do not own command execution, context evaluation or permissions.
/// The service resolves a contribution first and then hands the provider a
/// platform-neutral projection. All methods run on the owning UI thread.
pub trait MenuProvider {
    fn capability(&self, operation: MenusOperation) -> Capability<MenusOperation>;

    fn capabilities(&self) -> Vec<Capability<MenusOperation>> {
        MenusOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn install_menu_bar(&self, menu: &ResolvedMenuBar) -> Result<(), SystemError>;

    fn present_context_menu(
        &self,
        menu: &ResolvedContextMenu,
        surface: SurfaceId,
        generation: u64,
        anchor: Option<LogicalRect>,
    ) -> Result<(), SystemError>;

    fn sync_keybindings(&self, bindings: &[ResolvedKeybinding]) -> Result<(), SystemError>;
}

/// One context-menu presentation recorded by [`FakeMenuProvider`].
#[derive(Debug, Clone, PartialEq)]
pub struct PresentedContextMenu {
    pub menu: ResolvedContextMenu,
    pub surface: SurfaceId,
    pub generation: u64,
    pub anchor: Option<LogicalRect>,
}

#[derive(Default)]
struct FakeState {
    capabilities: BTreeMap<MenusOperation, Capability<MenusOperation>>,
    failures: BTreeMap<MenusOperation, SystemError>,
    installed: Vec<ResolvedMenuBar>,
    presented: Vec<PresentedContextMenu>,
    synced: Vec<Vec<ResolvedKeybinding>>,
    generations: BTreeMap<u64, u64>,
    callback_attempts: Vec<(SurfaceId, u64)>,
    stale_callback_rejections: Vec<(SurfaceId, u64)>,
    revision: u64,
}

/// Deterministic provider for headless tests and prototypes.
///
/// It records installed menu bars, context-menu presentations (surface,
/// generation and anchor), synced keybindings, callback generations and
/// injected failures. It performs no native work and never sleeps.
#[derive(Default)]
pub struct FakeMenuProvider {
    state: RefCell<FakeState>,
}

impl FakeMenuProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn capability(&self, operation: MenusOperation) -> Capability<MenusOperation> {
        MenuProvider::capability(self, operation)
    }

    pub fn set_capability(
        &self,
        operation: MenusOperation,
        capability: Capability<MenusOperation>,
    ) {
        let mut state = self.state.borrow_mut();
        state.capabilities.insert(operation, capability);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_support(&self, operation: MenusOperation, support: Support) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .capabilities
            .remove(&operation)
            .unwrap_or_else(|| default_capability(operation));
        capability.support = support;
        capability.observed_revision = state.revision.saturating_add(1);
        state.capabilities.insert(operation, capability);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_availability(&self, operation: MenusOperation, availability: Availability) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .capabilities
            .remove(&operation)
            .unwrap_or_else(|| default_capability(operation));
        capability.availability = availability;
        capability.observed_revision = state.revision.saturating_add(1);
        state.capabilities.insert(operation, capability);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_configuration(&self, operation: MenusOperation, configuration: Configuration) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .capabilities
            .remove(&operation)
            .unwrap_or_else(|| default_capability(operation));
        capability.configuration = configuration;
        capability.observed_revision = state.revision.saturating_add(1);
        state.capabilities.insert(operation, capability);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_permission(&self, operation: MenusOperation, permission: PermissionState) {
        let mut state = self.state.borrow_mut();
        let mut capability = state
            .capabilities
            .remove(&operation)
            .unwrap_or_else(|| default_capability(operation));
        capability.permission = permission;
        capability.observed_revision = state.revision.saturating_add(1);
        state.capabilities.insert(operation, capability);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_operation_unsupported(&self, operation: MenusOperation) {
        self.set_support(operation, Support::Unsupported);
        self.set_availability(operation, Availability::Unavailable);
    }

    pub fn fail_operation(&self, operation: MenusOperation, error: SystemError) {
        let mut state = self.state.borrow_mut();
        state.failures.insert(operation, error);
        state.revision = state.revision.saturating_add(1);
    }

    pub fn set_failure(&self, operation: MenusOperation, error: SystemError) {
        self.fail_operation(operation, error);
    }

    pub fn clear_failure(&self, operation: MenusOperation) {
        self.state.borrow_mut().failures.remove(&operation);
    }

    pub fn set_surface_generation(&self, surface: SurfaceId, generation: u64) {
        self.state
            .borrow_mut()
            .generations
            .insert(surface.0, generation);
    }

    pub fn installed_menu_bars(&self) -> Vec<ResolvedMenuBar> {
        self.state.borrow().installed.clone()
    }

    pub fn presented_context_menus(&self) -> Vec<PresentedContextMenu> {
        self.state.borrow().presented.clone()
    }

    pub fn synced_keybindings(&self) -> Vec<Vec<ResolvedKeybinding>> {
        self.state.borrow().synced.clone()
    }

    pub fn callback_attempts(&self) -> Vec<(SurfaceId, u64)> {
        self.state.borrow().callback_attempts.clone()
    }

    pub fn stale_callback_rejections(&self) -> Vec<(SurfaceId, u64)> {
        self.state.borrow().stale_callback_rejections.clone()
    }

    pub fn revision(&self) -> u64 {
        self.state.borrow().revision
    }

    /// Validate a late native callback against the latest recorded generation.
    /// A stale callback is recorded and rejected before it can reach a command.
    pub fn record_callback(&self, surface: SurfaceId, generation: u64) -> Result<(), SystemError> {
        let mut state = self.state.borrow_mut();
        state.callback_attempts.push((surface, generation));
        if state.generations.get(&surface.0).copied() == Some(generation) {
            return Ok(());
        }
        state.stale_callback_rejections.push((surface, generation));
        Err(SystemError::new(
            ErrorCode::ContextExpired,
            "Native menu callback belongs to an expired surface generation",
        ))
    }

    fn fail_if_configured(&self, operation: MenusOperation) -> Result<(), SystemError> {
        self.state
            .borrow()
            .failures
            .get(&operation)
            .cloned()
            .map_or(Ok(()), Err)
    }
}

fn default_capability(operation: MenusOperation) -> Capability<MenusOperation> {
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

impl MenuProvider for FakeMenuProvider {
    fn capability(&self, operation: MenusOperation) -> Capability<MenusOperation> {
        self.state
            .borrow()
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| default_capability(operation))
    }

    fn install_menu_bar(&self, menu: &ResolvedMenuBar) -> Result<(), SystemError> {
        self.fail_if_configured(MenusOperation::InstallMenuBar)?;
        let mut state = self.state.borrow_mut();
        state.installed.push(menu.clone());
        if let (Some(surface), Some(generation)) = (menu.surface, menu.generation) {
            state.generations.insert(surface.0, generation);
        }
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }

    fn present_context_menu(
        &self,
        menu: &ResolvedContextMenu,
        surface: SurfaceId,
        generation: u64,
        anchor: Option<LogicalRect>,
    ) -> Result<(), SystemError> {
        self.fail_if_configured(MenusOperation::PresentContextMenu)?;
        let mut state = self.state.borrow_mut();
        state.presented.push(PresentedContextMenu {
            menu: menu.clone(),
            surface,
            generation,
            anchor,
        });
        state.generations.insert(surface.0, generation);
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }

    fn sync_keybindings(&self, bindings: &[ResolvedKeybinding]) -> Result<(), SystemError> {
        self.fail_if_configured(MenusOperation::SyncKeybindings)?;
        let mut state = self.state.borrow_mut();
        state.synced.push(bindings.to_vec());
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }
}
