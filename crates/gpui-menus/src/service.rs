//! UI-local command contribution service.
//!
//! All registration, scope and context mutation happens on the owning UI
//! thread. Background handlers run on the service's bounded worker pool and
//! communicate through shared task records and a bounded event stream; they
//! never retain the UI-local service.

use crate::handler::{ContributionBundle, Handler, PendingCommand};
use crate::menu::{ContextMenu, MenuBar, MenuNode};
use crate::task::{Broadcaster, MenuCommandEvent, TaskShared, subscribe_events};
use crate::worker::{WorkerPool, WorkerPoolConfig};
use crate::{
    BackgroundCommandContext, BackgroundCommandOutcome, ButtonCommandBinding, CancelReason,
    Capability, CommandDescriptor, CommandHandle, CommandId, CommandScope, CommandTaskSnapshot,
    ContextExpr, ContextKey, ContextSnapshot, ContextStore, ContextValue, ContributionScope,
    ContributionScopeId, DuplicateCommandDiagnostic, ErrorCode, EventSubscription, ExecutionDomain,
    InvocationContext, InvocationId, KeychordBinding, LazyContribution, LazyContributionId,
    LazyMaterializationState, MenuProvider, MenusOperation, PermissionProvider, PermissionService,
    RegistrationLease, RouteRegistry, SurfaceId, SystemActionBinding, SystemError, SystemServices,
    UiCommandContext, UiCommandOutcome,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
    sync::{Arc, atomic::AtomicUsize},
    time::Instant,
};

const MAX_DUPLICATE_DIAGNOSTICS: usize = 64;
const MAX_TASK_HISTORY: usize = 256;
const TASK_HISTORY_TRIM: usize = 192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegistrationOwner {
    pub scope: ContributionScopeId,
    pub registration: u64,
    pub sequence: u64,
}

#[derive(Debug)]
pub(crate) struct CommandEntry {
    pub descriptor: CommandDescriptor,
    pub handler: Handler,
    pub owner: RegistrationOwner,
    pub in_flight: Arc<AtomicUsize>,
}

impl Clone for CommandEntry {
    fn clone(&self) -> Self {
        Self {
            descriptor: self.descriptor.clone(),
            handler: self.handler.clone(),
            owner: self.owner,
            in_flight: self.in_flight.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum MenuContribution {
    Bar(MenuBar),
    Context(ContextMenu),
}

#[derive(Clone, Debug)]
pub(crate) struct MenuEntry {
    pub owner: RegistrationOwner,
    pub contribution: MenuContribution,
}

#[derive(Clone, Debug)]
pub(crate) struct KeybindingEntry {
    pub owner: RegistrationOwner,
    pub binding: KeychordBinding,
    pub order: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ButtonEntry {
    pub owner: RegistrationOwner,
    pub binding: ButtonCommandBinding,
}

#[derive(Clone, Debug)]
pub(crate) struct SystemActionEntry {
    pub owner: RegistrationOwner,
    pub binding: SystemActionBinding,
}

pub(crate) struct LazyEntry {
    pub owner: RegistrationOwner,
    pub state: LazyMaterializationState,
    pub factory: Option<Box<dyn FnOnce() -> ContributionBundle>>,
    pub menu: Vec<MenuNode>,
}

#[derive(Clone, Debug)]
pub(crate) struct RegistrationRecord {
    pub scope: ContributionScopeId,
}

#[derive(Clone)]
pub(crate) struct ScopeRecord {
    pub binding: crate::scope::ScopeBinding,
    pub persistent: bool,
}

pub(crate) struct SurfaceRecord {
    pub generation: Option<u64>,
}

pub(crate) struct ServiceState {
    pub commands: BTreeMap<CommandId, CommandEntry>,
    pub menus: Vec<MenuEntry>,
    pub keybindings: Vec<KeybindingEntry>,
    pub buttons: Vec<ButtonEntry>,
    pub system_actions: Vec<SystemActionEntry>,
    pub lazy: BTreeMap<LazyContributionId, LazyEntry>,
    pub registrations: BTreeMap<u64, RegistrationRecord>,
    pub scopes: BTreeMap<ContributionScopeId, ScopeRecord>,
    pub contexts: ContextStore,
    pub tasks: BTreeMap<InvocationId, Arc<TaskShared>>,
    pub duplicates: VecDeque<DuplicateCommandDiagnostic>,
    pub surfaces: BTreeMap<u64, SurfaceRecord>,
    pub next_scope: u64,
    pub next_registration: u64,
    pub next_lazy: u64,
    pub next_invocation: u64,
    pub next_order: u64,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self {
            commands: BTreeMap::new(),
            menus: Vec::new(),
            keybindings: Vec::new(),
            buttons: Vec::new(),
            system_actions: Vec::new(),
            lazy: BTreeMap::new(),
            registrations: BTreeMap::new(),
            scopes: BTreeMap::new(),
            contexts: ContextStore::new(),
            tasks: BTreeMap::new(),
            duplicates: VecDeque::new(),
            surfaces: BTreeMap::new(),
            next_scope: 1,
            next_registration: 1,
            next_lazy: 1,
            next_invocation: 1,
            next_order: 1,
        }
    }
}

pub(crate) struct ServiceInner {
    pub provider: Rc<dyn MenuProvider>,
    pub permissions: RefCell<Option<Rc<dyn PermissionProvider>>>,
    pub pool_config: WorkerPoolConfig,
    pub pool: RefCell<WorkerPool>,
    pub state: RefCell<ServiceState>,
    pub events: Arc<Broadcaster>,
    pub background_slots: Arc<AtomicUsize>,
    pub max_background_slots: usize,
}

impl ServiceInner {
    fn new(
        provider: Rc<dyn MenuProvider>,
        permissions: Option<Rc<dyn PermissionProvider>>,
        config: WorkerPoolConfig,
    ) -> Self {
        Self {
            provider,
            permissions: RefCell::new(permissions),
            pool_config: config,
            pool: RefCell::new(WorkerPool::new(config)),
            state: RefCell::new(ServiceState::default()),
            events: Arc::new(Broadcaster::default()),
            background_slots: Arc::new(AtomicUsize::new(0)),
            max_background_slots: config.max_active(),
        }
    }

    pub(crate) fn capability(&self, operation: MenusOperation) -> Capability<MenusOperation> {
        self.provider.capability(operation)
    }

    pub(crate) fn surface_generation(&self, surface: SurfaceId) -> Option<u64> {
        self.state
            .borrow()
            .surfaces
            .get(&surface.0)
            .map_or(Some(1), |record| record.generation)
    }

    pub(crate) fn scope_active(&self, scope: ContributionScopeId) -> bool {
        self.state.borrow().scopes.contains_key(&scope)
    }

    pub(crate) fn registration_active(&self, registration: u64) -> bool {
        self.state
            .borrow()
            .registrations
            .contains_key(&registration)
    }

    pub(crate) fn scope_binding(
        &self,
        scope: ContributionScopeId,
    ) -> Result<crate::scope::ScopeBinding, SystemError> {
        let binding = self
            .state
            .borrow()
            .scopes
            .get(&scope)
            .map(|record| record.binding.clone())
            .ok_or_else(|| {
                SystemError::new(
                    ErrorCode::ContextExpired,
                    "Contribution scope is no longer active",
                )
            })?;
        binding.check_live(|surface| self.surface_generation(surface))?;
        Ok(binding)
    }

    fn new_scope_record(
        &self,
        binding: crate::scope::ScopeBinding,
        persistent: bool,
    ) -> ContributionScopeId {
        let mut state = self.state.borrow_mut();
        let id = ContributionScopeId(state.next_scope);
        state.next_scope = state.next_scope.saturating_add(1);
        state.scopes.insert(
            id,
            ScopeRecord {
                binding,
                persistent,
            },
        );
        id
    }

    /// Begin one registration under an existing scope, or create an ephemeral
    /// global scope for the convenience registration methods.
    pub(crate) fn begin_owner(
        &self,
        scope: Option<&ContributionScope>,
    ) -> Result<RegistrationOwner, SystemError> {
        let scope_id = if let Some(scope) = scope {
            let service = scope.service()?;
            if !std::ptr::eq(Rc::as_ptr(&service), self) {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Contribution scope belongs to another menu service",
                ));
            }
            self.scope_binding(scope.id())?;
            scope.id()
        } else {
            self.new_scope_record(crate::scope::ScopeBinding::Global, false)
        };
        let (registration, sequence) = {
            let mut state = self.state.borrow_mut();
            let sequence = state.next_order;
            state.next_order = state.next_order.saturating_add(1);
            let registration = state.next_registration;
            state.next_registration = state.next_registration.saturating_add(1);
            state
                .registrations
                .insert(registration, RegistrationRecord { scope: scope_id });
            (registration, sequence)
        };
        Ok(RegistrationOwner {
            scope: scope_id,
            registration,
            sequence,
        })
    }

    pub(crate) fn scope_handle(&self, binding: crate::scope::ScopeBinding) -> ContributionScopeId {
        self.new_scope_record(binding, true)
    }

    pub(crate) fn insert_command(
        &self,
        owner: RegistrationOwner,
        pending: PendingCommand,
    ) -> Result<CommandEntry, SystemError> {
        pending.descriptor.validate()?;
        if pending.descriptor.domain != pending.handler.domain() {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Command descriptor domain does not match the registered handler",
            ));
        }
        let duplicate = {
            let mut state = self.state.borrow_mut();
            if let Some(existing) = state.commands.get(&pending.descriptor.id) {
                let diagnostic = DuplicateCommandDiagnostic {
                    command_id: pending.descriptor.id.clone(),
                    existing_scope: existing.owner.scope,
                    conflicting_scope: owner.scope,
                    domain: pending.descriptor.domain,
                };
                while state.duplicates.len() >= MAX_DUPLICATE_DIAGNOSTICS {
                    state.duplicates.pop_front();
                }
                state.duplicates.push_back(diagnostic.clone());
                Some(diagnostic)
            } else {
                None
            }
        };
        if let Some(diagnostic) = duplicate {
            self.events
                .send(MenuCommandEvent::DuplicateCommandRejected(diagnostic));
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Command identity is already registered",
            ));
        }
        let entry = CommandEntry {
            descriptor: pending.descriptor,
            handler: pending.handler,
            owner,
            in_flight: Arc::new(AtomicUsize::new(0)),
        };
        self.state
            .borrow_mut()
            .commands
            .insert(entry.descriptor.id.clone(), entry.clone());
        Ok(entry)
    }

    pub(crate) fn command(&self, id: &CommandId) -> Option<CommandEntry> {
        self.state.borrow().commands.get(id).cloned()
    }

    pub(crate) fn revoke_scope(&self, scope: ContributionScopeId) -> bool {
        let registrations: Vec<u64> = {
            let state = self.state.borrow();
            if !state.scopes.contains_key(&scope) {
                return false;
            }
            state
                .registrations
                .iter()
                .filter(|(_, record)| record.scope == scope)
                .map(|(id, _)| *id)
                .collect()
        };
        {
            let mut state = self.state.borrow_mut();
            state.scopes.remove(&scope);
            for registration in &registrations {
                state.registrations.remove(registration);
                remove_registration_entries(&mut state, *registration);
            }
            state.contexts.clear_scope(scope.0);
        }
        let tasks: Vec<Arc<TaskShared>> = self
            .state
            .borrow()
            .tasks
            .values()
            .filter(|task| {
                task.scope() == Some(scope)
                    || task
                        .registration()
                        .is_some_and(|registration| registrations.contains(&registration))
            })
            .cloned()
            .collect();
        for task in tasks {
            task.cancel(CancelReason::OwnerDisposed);
        }
        true
    }

    pub(crate) fn revoke_registration(&self, registration: u64) -> bool {
        let (record, scope_removed) = {
            let mut state = self.state.borrow_mut();
            let Some(record) = state.registrations.remove(&registration) else {
                return false;
            };
            remove_registration_entries(&mut state, registration);
            let has_siblings = state
                .registrations
                .values()
                .any(|sibling| sibling.scope == record.scope);
            let mut scope_removed = false;
            if !has_siblings
                && state
                    .scopes
                    .get(&record.scope)
                    .is_some_and(|scope| !scope.persistent)
            {
                state.scopes.remove(&record.scope);
                state.contexts.clear_scope(record.scope.0);
                scope_removed = true;
            }
            (record, scope_removed)
        };
        let tasks: Vec<Arc<TaskShared>> = self
            .state
            .borrow()
            .tasks
            .values()
            .filter(|task| {
                task.registration() == Some(registration)
                    || (scope_removed && task.scope() == Some(record.scope))
            })
            .cloned()
            .collect();
        for task in tasks {
            task.cancel(CancelReason::OwnerDisposed);
        }
        true
    }

    pub(crate) fn duplicate_diagnostics(&self) -> Vec<DuplicateCommandDiagnostic> {
        self.state.borrow().duplicates.iter().cloned().collect()
    }

    pub(crate) fn prune_tasks(&self) {
        let mut state = self.state.borrow_mut();
        if state.tasks.len() < MAX_TASK_HISTORY {
            return;
        }
        let terminal: Vec<InvocationId> = state
            .tasks
            .iter()
            .filter(|(_, task)| !task.is_running())
            .map(|(id, _)| *id)
            .collect();
        let excess = state.tasks.len().saturating_sub(TASK_HISTORY_TRIM);
        for id in terminal.into_iter().take(excess) {
            state.tasks.remove(&id);
        }
    }

    pub(crate) fn allocate_invocation(&self) -> InvocationId {
        let mut state = self.state.borrow_mut();
        let id = InvocationId(state.next_invocation);
        state.next_invocation = state.next_invocation.saturating_add(1);
        id
    }

    pub(crate) fn insert_task(&self, task: Arc<TaskShared>) -> InvocationId {
        self.prune_tasks();
        let id = task.id();
        self.state.borrow_mut().tasks.insert(id, task);
        id
    }

    pub(crate) fn task(&self, id: InvocationId) -> Option<Arc<TaskShared>> {
        self.state.borrow().tasks.get(&id).cloned()
    }

    pub(crate) fn task_snapshot(&self, id: InvocationId) -> Option<CommandTaskSnapshot> {
        self.task(id).map(|task| task.snapshot())
    }

    pub(crate) fn task_snapshots(&self) -> Vec<CommandTaskSnapshot> {
        self.state
            .borrow()
            .tasks
            .values()
            .map(|task| task.snapshot())
            .collect()
    }

    pub(crate) fn running_tasks_for(
        &self,
        command: &CommandId,
        registration: Option<u64>,
    ) -> Vec<Arc<TaskShared>> {
        self.state
            .borrow()
            .tasks
            .values()
            .filter(|task| {
                task.command_id() == command
                    && task.is_running()
                    && registration
                        .is_none_or(|registration| task.registration() == Some(registration))
            })
            .cloned()
            .collect()
    }

    pub(crate) fn cancel_invocation(&self, id: InvocationId, reason: CancelReason) -> bool {
        self.task(id).is_some_and(|task| task.cancel(reason))
    }

    pub(crate) fn expire_at(&self, now: Instant) -> usize {
        let tasks: Vec<Arc<TaskShared>> = self.state.borrow().tasks.values().cloned().collect();
        tasks.into_iter().filter(|task| task.expire(now)).count()
    }

    pub(crate) fn permission_provider(&self) -> Option<Rc<dyn PermissionProvider>> {
        self.permissions.borrow().clone()
    }

    pub(crate) fn lazy_state(&self, id: LazyContributionId) -> Option<LazyMaterializationState> {
        self.state.borrow().lazy.get(&id).map(|entry| entry.state)
    }

    pub(crate) fn materialize_lazy(&self, id: LazyContributionId) -> bool {
        let (owner, factory) = {
            let mut state = self.state.borrow_mut();
            let Some(entry) = state.lazy.get_mut(&id) else {
                return false;
            };
            if entry.state != LazyMaterializationState::Unmaterialized {
                return entry.state == LazyMaterializationState::Materialized;
            }
            entry.state = LazyMaterializationState::Materializing;
            let Some(factory) = entry.factory.take() else {
                return false;
            };
            (entry.owner, factory)
        };
        let bundle = factory();
        let still_active = self
            .state
            .borrow()
            .lazy
            .get(&id)
            .is_some_and(|entry| entry.state == LazyMaterializationState::Materializing);
        if !still_active {
            return false;
        }
        for pending in bundle.commands {
            // Duplicate identities are rejected with a diagnostic; the rest of
            // the bundle still materializes.
            let _ = self.insert_command(owner, pending);
        }
        {
            let mut state = self.state.borrow_mut();
            let base = state.next_order;
            state.next_order = state
                .next_order
                .saturating_add(bundle.keybindings.len() as u64);
            for (offset, binding) in bundle.keybindings.into_iter().enumerate() {
                let order = base.saturating_add(offset as u64);
                state.keybindings.push(KeybindingEntry {
                    owner,
                    binding,
                    order,
                });
            }
            for binding in bundle.buttons {
                state.buttons.push(ButtonEntry { owner, binding });
            }
            for binding in bundle.system_actions {
                state
                    .system_actions
                    .push(SystemActionEntry { owner, binding });
            }
        }
        let mut state = self.state.borrow_mut();
        if let Some(entry) = state.lazy.get_mut(&id)
            && entry.state == LazyMaterializationState::Materializing
        {
            entry.menu = bundle.menu;
            entry.state = LazyMaterializationState::Materialized;
            return true;
        }
        false
    }

    pub(crate) fn materialize_all(&self) -> usize {
        let mut materialized = 0;
        loop {
            let next = {
                let state = self.state.borrow();
                state
                    .lazy
                    .iter()
                    .find(|(_, entry)| entry.state == LazyMaterializationState::Unmaterialized)
                    .map(|(id, _)| *id)
            };
            let Some(id) = next else {
                break;
            };
            if self.materialize_lazy(id) {
                materialized += 1;
            } else if self
                .state
                .borrow()
                .lazy
                .get(&id)
                .is_none_or(|entry| entry.state != LazyMaterializationState::Unmaterialized)
            {
                continue;
            } else {
                break;
            }
        }
        materialized
    }

    pub(crate) fn ensure_command(&self, id: &CommandId) -> Option<CommandEntry> {
        if let Some(entry) = self.command(id) {
            return Some(entry);
        }
        loop {
            let next = {
                let state = self.state.borrow();
                state
                    .lazy
                    .iter()
                    .find(|(_, entry)| entry.state == LazyMaterializationState::Unmaterialized)
                    .map(|(lazy, _)| *lazy)
            };
            let Some(lazy) = next else {
                break;
            };
            self.materialize_lazy(lazy);
            if let Some(entry) = self.command(id) {
                return Some(entry);
            }
        }
        None
    }
}

fn remove_registration_entries(state: &mut ServiceState, registration: u64) {
    state
        .commands
        .retain(|_, entry| entry.owner.registration != registration);
    state
        .menus
        .retain(|entry| entry.owner.registration != registration);
    state
        .keybindings
        .retain(|entry| entry.owner.registration != registration);
    state
        .buttons
        .retain(|entry| entry.owner.registration != registration);
    state
        .system_actions
        .retain(|entry| entry.owner.registration != registration);
    state
        .lazy
        .retain(|_, entry| entry.owner.registration != registration);
}

/// Typed registration result: a handle for typed invocation plus the RAII lease.
pub struct CommandRegistration<I, O> {
    handle: CommandHandle<I, O>,
    lease: RegistrationLease,
}

impl<I, O> CommandRegistration<I, O> {
    pub const fn handle(&self) -> &CommandHandle<I, O> {
        &self.handle
    }

    pub const fn lease(&self) -> &RegistrationLease {
        &self.lease
    }

    pub fn into_parts(self) -> (CommandHandle<I, O>, RegistrationLease) {
        (self.handle, self.lease)
    }
}

impl<I, O> std::fmt::Debug for CommandRegistration<I, O> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandRegistration")
            .field("handle", &self.handle)
            .field("lease", &self.lease)
            .finish()
    }
}

/// UI-local menu and command contribution service.
#[derive(Clone)]
pub struct MenuCommandService(pub(crate) Rc<ServiceInner>);

impl MenuCommandService {
    pub fn new(provider: Rc<dyn MenuProvider>) -> Self {
        Self::with_config(provider, None, WorkerPoolConfig::default())
    }

    pub fn from_provider(provider: impl MenuProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn with_worker_pool(
        provider: Rc<dyn MenuProvider>,
        config: WorkerPoolConfig,
    ) -> Result<Self, SystemError> {
        config.validate()?;
        Ok(Self::with_config(provider, None, config))
    }

    pub fn with_permission_service(
        provider: Rc<dyn MenuProvider>,
        permissions: PermissionService,
    ) -> Self {
        Self::with_permissions(provider, permissions.provider().clone())
    }

    pub fn with_permissions(
        provider: Rc<dyn MenuProvider>,
        permissions: Rc<dyn PermissionProvider>,
    ) -> Self {
        Self::with_config(provider, Some(permissions), WorkerPoolConfig::default())
    }

    fn with_config(
        provider: Rc<dyn MenuProvider>,
        permissions: Option<Rc<dyn PermissionProvider>>,
        config: WorkerPoolConfig,
    ) -> Self {
        Self(Rc::new(ServiceInner::new(provider, permissions, config)))
    }

    pub fn provider(&self) -> &Rc<dyn MenuProvider> {
        &self.0.provider
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn set_permission_provider(&self, provider: Rc<dyn PermissionProvider>) {
        *self.0.permissions.borrow_mut() = Some(provider);
    }

    pub fn set_permission_service(&self, service: PermissionService) {
        self.set_permission_provider(service.provider().clone());
    }

    pub fn permission_provider(&self) -> Option<Rc<dyn PermissionProvider>> {
        self.0.permission_provider()
    }

    pub fn worker_pool_config(&self) -> WorkerPoolConfig {
        self.0.pool_config
    }

    pub fn capability(&self, operation: MenusOperation) -> Capability<MenusOperation> {
        self.0.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<MenusOperation>> {
        MenusOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    pub fn subscribe(&self) -> EventSubscription<MenuCommandEvent> {
        subscribe_events(&self.0.events)
    }

    pub fn duplicate_diagnostics(&self) -> Vec<DuplicateCommandDiagnostic> {
        self.0.duplicate_diagnostics()
    }

    pub fn command_ids(&self) -> Vec<CommandId> {
        self.0.state.borrow().commands.keys().cloned().collect()
    }

    pub fn is_registered(&self, id: &CommandId) -> bool {
        self.0.state.borrow().commands.contains_key(id)
    }

    pub fn registered_menu_bars(&self) -> Vec<MenuBar> {
        self.0
            .state
            .borrow()
            .menus
            .iter()
            .filter_map(|entry| match &entry.contribution {
                MenuContribution::Bar(bar) => Some(bar.clone()),
                MenuContribution::Context(_) => None,
            })
            .collect()
    }

    pub fn registered_context_menus(&self) -> Vec<ContextMenu> {
        self.0
            .state
            .borrow()
            .menus
            .iter()
            .filter_map(|entry| match &entry.contribution {
                MenuContribution::Context(menu) => Some(menu.clone()),
                MenuContribution::Bar(_) => None,
            })
            .collect()
    }

    pub fn command_descriptor(&self, id: &CommandId) -> Option<CommandDescriptor> {
        self.0
            .state
            .borrow()
            .commands
            .get(id)
            .map(|entry| entry.descriptor.clone())
    }

    /// Validate a late native menu callback against the live surface
    /// generation. Providers may keep their own generation bookkeeping, but
    /// this is the authoritative service check before a callback may invoke a
    /// command.
    pub fn validate_menu_callback(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        match self.0.surface_generation(surface) {
            Some(active) if active == generation => Ok(()),
            Some(_) => Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Menu callback belongs to an expired surface generation",
            )),
            None => Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Menu callback targets a disposed surface",
            )),
        }
    }

    pub fn command_task(&self, id: InvocationId) -> Option<CommandTaskSnapshot> {
        self.0.task_snapshot(id)
    }

    pub fn tasks(&self) -> Vec<CommandTaskSnapshot> {
        self.0.task_snapshots()
    }

    pub fn cancel_invocation(&self, id: InvocationId, reason: CancelReason) -> bool {
        self.0.cancel_invocation(id, reason)
    }

    pub fn expire_invocations_at(&self, now: Instant) -> usize {
        self.0.expire_at(now)
    }

    pub fn check_deadlines(&self) -> usize {
        self.0.expire_at(Instant::now())
    }

    // ---- context ---------------------------------------------------------

    pub fn set_global_context(&self, key: ContextKey, value: ContextValue) -> u64 {
        self.0.state.borrow_mut().contexts.set_global(key, value)
    }

    pub fn set_surface_context(
        &self,
        surface: SurfaceId,
        key: ContextKey,
        value: ContextValue,
    ) -> u64 {
        self.0
            .state
            .borrow_mut()
            .contexts
            .set_surface(surface, key, value)
    }

    pub fn set_scope_context(
        &self,
        scope: ContributionScopeId,
        key: ContextKey,
        value: ContextValue,
    ) -> u64 {
        self.0
            .state
            .borrow_mut()
            .contexts
            .set_scope(scope.0, key, value)
    }

    pub fn remove_global_context(&self, key: &ContextKey) -> bool {
        self.0.state.borrow_mut().contexts.remove_global(key)
    }

    pub fn clear_surface_context(&self, surface: SurfaceId) -> bool {
        self.0.state.borrow_mut().contexts.clear_surface(surface)
    }

    pub fn clear_scope_context(&self, scope: ContributionScopeId) -> bool {
        self.0.state.borrow_mut().contexts.clear_scope(scope.0)
    }

    pub fn context_revision(&self) -> u64 {
        self.0.state.borrow().contexts.revision()
    }

    pub fn context_snapshot(&self) -> ContextSnapshot {
        self.0.state.borrow().contexts.snapshot()
    }

    pub fn context_snapshot_for(
        &self,
        surface: Option<SurfaceId>,
        scope: Option<ContributionScopeId>,
    ) -> ContextSnapshot {
        self.0
            .state
            .borrow()
            .contexts
            .snapshot_for(surface, scope.map(|scope| scope.0))
    }

    pub fn evaluate(&self, expression: &ContextExpr, context: &InvocationContext) -> bool {
        let state = self.0.state.borrow();
        state.contexts.evaluate(
            expression,
            context.effective_surface(),
            context.scope.map(|scope| scope.0),
            context.overlay.as_ref(),
        )
    }

    // ---- scopes, surfaces and leases ------------------------------------

    pub fn create_scope(&self, scope: CommandScope) -> Result<ContributionScope, SystemError> {
        let binding = scope.binding()?;
        binding.check_live(|surface| self.0.surface_generation(surface))?;
        let id = self.0.scope_handle(binding);
        Ok(ContributionScope::new(id, Rc::downgrade(&self.0)))
    }

    pub fn revoke_scope(&self, scope: ContributionScopeId) -> bool {
        self.0.revoke_scope(scope)
    }

    pub fn surface_generation(&self, surface: SurfaceId) -> Option<u64> {
        self.0.surface_generation(surface)
    }

    pub fn register_surface(&self, surface: SurfaceId, generation: u64) -> Result<(), SystemError> {
        if generation == 0 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "A surface generation must be nonzero",
            ));
        }
        self.0.state.borrow_mut().surfaces.insert(
            surface.0,
            SurfaceRecord {
                generation: Some(generation),
            },
        );
        Ok(())
    }

    pub fn advance_surface_generation(&self, surface: SurfaceId) -> Result<u64, SystemError> {
        let current = self.0.surface_generation(surface).ok_or_else(|| {
            SystemError::new(ErrorCode::ContextExpired, "Surface has been disposed")
        })?;
        let next = current.checked_add(1).ok_or_else(|| {
            SystemError::new(ErrorCode::PlatformFailure, "Surface generation overflowed")
        })?;
        self.0.state.borrow_mut().surfaces.insert(
            surface.0,
            SurfaceRecord {
                generation: Some(next),
            },
        );
        self.revoke_surface_scopes(surface);
        self.cancel_surface_tasks(surface);
        self.0.state.borrow_mut().contexts.clear_surface(surface);
        Ok(next)
    }

    /// Alias used by hosts that advance a surface generation.
    pub fn advance_generation(&self, surface: SurfaceId) -> Result<u64, SystemError> {
        self.advance_surface_generation(surface)
    }

    pub fn dispose_surface(&self, surface: SurfaceId) {
        self.0
            .state
            .borrow_mut()
            .surfaces
            .insert(surface.0, SurfaceRecord { generation: None });
        self.revoke_surface_scopes(surface);
        self.cancel_surface_tasks(surface);
        self.0.state.borrow_mut().contexts.clear_surface(surface);
    }

    fn revoke_surface_scopes(&self, surface: SurfaceId) {
        let scopes: Vec<ContributionScopeId> = {
            let state = self.0.state.borrow();
            state
                .scopes
                .iter()
                .filter(|(_, record)| record.binding.surface() == Some(surface))
                .map(|(id, _)| *id)
                .collect()
        };
        for scope in scopes {
            self.0.revoke_scope(scope);
        }
    }

    fn cancel_surface_tasks(&self, surface: SurfaceId) {
        let tasks: Vec<Arc<TaskShared>> = self
            .0
            .state
            .borrow()
            .tasks
            .values()
            .filter(|task| task.surface() == Some(surface))
            .cloned()
            .collect();
        for task in tasks {
            task.cancel(CancelReason::OwnerDisposed);
        }
    }

    // ---- typed registration ---------------------------------------------

    pub fn register_ui<I, O>(
        &self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<O, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_ui_in_scoped(None, descriptor, handler)
    }

    pub fn register_ui_in<I, O>(
        &self,
        scope: &ContributionScope,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<O, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_ui_in_scoped(Some(scope), descriptor, handler)
    }

    fn register_ui_in_scoped<I, O>(
        &self,
        scope: Option<&ContributionScope>,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<O, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        if descriptor.domain != ExecutionDomain::Ui {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "register_ui requires an ExecutionDomain::Ui descriptor",
            ));
        }
        let pending = PendingCommand {
            descriptor,
            handler: Handler::Ui(crate::handler::ui_handler(handler)),
        };
        self.finish_registration(scope, pending, |id, domain| CommandHandle::new(id, domain))
    }

    pub fn register_ui_deferred<I, O>(
        &self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<UiCommandOutcome<O>, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_ui_deferred_in_scoped(None, descriptor, handler)
    }

    pub fn register_ui_deferred_in<I, O>(
        &self,
        scope: &ContributionScope,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<UiCommandOutcome<O>, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_ui_deferred_in_scoped(Some(scope), descriptor, handler)
    }

    fn register_ui_deferred_in_scoped<I, O>(
        &self,
        scope: Option<&ContributionScope>,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, &mut UiCommandContext) -> Result<UiCommandOutcome<O>, SystemError> + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        if descriptor.domain != ExecutionDomain::Ui {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "register_ui_deferred requires an ExecutionDomain::Ui descriptor",
            ));
        }
        let pending = PendingCommand {
            descriptor,
            handler: Handler::Ui(crate::handler::ui_deferred_handler(handler)),
        };
        self.finish_registration(scope, pending, |id, domain| CommandHandle::new(id, domain))
    }

    pub fn register_background<I, O>(
        &self,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, BackgroundCommandContext) -> Result<O, SystemError> + Send + Sync + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_background_in_scoped(None, descriptor, handler)
    }

    pub fn register_background_in<I, O>(
        &self,
        scope: &ContributionScope,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, BackgroundCommandContext) -> Result<O, SystemError> + Send + Sync + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        self.register_background_in_scoped(Some(scope), descriptor, handler)
    }

    fn register_background_in_scoped<I, O>(
        &self,
        scope: Option<&ContributionScope>,
        descriptor: CommandDescriptor,
        handler: impl Fn(I, BackgroundCommandContext) -> Result<O, SystemError> + Send + Sync + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + 'static,
    {
        if descriptor.domain != ExecutionDomain::Background {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "register_background requires an ExecutionDomain::Background descriptor",
            ));
        }
        let pending = PendingCommand {
            descriptor,
            handler: Handler::Background(crate::handler::background_handler(handler)),
        };
        self.finish_registration(scope, pending, |id, domain| CommandHandle::new(id, domain))
    }

    pub fn register_background_deferred<I, O>(
        &self,
        descriptor: CommandDescriptor,
        handler: impl Fn(
            I,
            BackgroundCommandContext,
        ) -> Result<BackgroundCommandOutcome<O>, SystemError>
        + Send
        + Sync
        + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + Send + 'static,
    {
        self.register_background_deferred_in_scoped(None, descriptor, handler)
    }

    pub fn register_background_deferred_in<I, O>(
        &self,
        scope: &ContributionScope,
        descriptor: CommandDescriptor,
        handler: impl Fn(
            I,
            BackgroundCommandContext,
        ) -> Result<BackgroundCommandOutcome<O>, SystemError>
        + Send
        + Sync
        + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + Send + 'static,
    {
        self.register_background_deferred_in_scoped(Some(scope), descriptor, handler)
    }

    fn register_background_deferred_in_scoped<I, O>(
        &self,
        scope: Option<&ContributionScope>,
        descriptor: CommandDescriptor,
        handler: impl Fn(
            I,
            BackgroundCommandContext,
        ) -> Result<BackgroundCommandOutcome<O>, SystemError>
        + Send
        + Sync
        + 'static,
    ) -> Result<CommandRegistration<I, O>, SystemError>
    where
        I: serde::de::DeserializeOwned + 'static,
        O: serde::Serialize + Send + 'static,
    {
        if descriptor.domain != ExecutionDomain::Background {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "register_background_deferred requires an ExecutionDomain::Background descriptor",
            ));
        }
        let pending = PendingCommand {
            descriptor,
            handler: Handler::Background(crate::handler::background_deferred_handler(handler)),
        };
        self.finish_registration(scope, pending, |id, domain| CommandHandle::new(id, domain))
    }

    fn finish_registration<I, O>(
        &self,
        scope: Option<&ContributionScope>,
        pending: PendingCommand,
        handle: impl FnOnce(CommandId, ExecutionDomain) -> CommandHandle<I, O>,
    ) -> Result<CommandRegistration<I, O>, SystemError> {
        let id = pending.descriptor.id.clone();
        let domain = pending.descriptor.domain;
        let owner = self.0.begin_owner(scope)?;
        match self.0.insert_command(owner, pending) {
            Ok(_) => Ok(CommandRegistration {
                handle: handle(id, domain),
                lease: RegistrationLease::new(
                    Rc::downgrade(&self.0),
                    owner.registration,
                    owner.scope,
                ),
            }),
            Err(error) => {
                self.0.revoke_registration(owner.registration);
                Err(error)
            }
        }
    }

    // ---- non-command contributions --------------------------------------

    pub fn register_keybinding(
        &self,
        scope: &ContributionScope,
        binding: KeychordBinding,
    ) -> Result<RegistrationLease, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        let order = {
            let mut state = self.0.state.borrow_mut();
            let order = state.next_order;
            state.next_order = state.next_order.saturating_add(1);
            order
        };
        self.0.state.borrow_mut().keybindings.push(KeybindingEntry {
            owner,
            binding,
            order,
        });
        Ok(RegistrationLease::new(
            Rc::downgrade(&self.0),
            owner.registration,
            owner.scope,
        ))
    }

    pub fn register_button(
        &self,
        scope: &ContributionScope,
        binding: ButtonCommandBinding,
    ) -> Result<RegistrationLease, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        self.0
            .state
            .borrow_mut()
            .buttons
            .push(ButtonEntry { owner, binding });
        Ok(RegistrationLease::new(
            Rc::downgrade(&self.0),
            owner.registration,
            owner.scope,
        ))
    }

    pub fn register_system_action(
        &self,
        scope: &ContributionScope,
        binding: SystemActionBinding,
        routes: &RouteRegistry,
    ) -> Result<RegistrationLease, SystemError> {
        binding.validate(routes)?;
        let owner = self.0.begin_owner(Some(scope))?;
        self.0
            .state
            .borrow_mut()
            .system_actions
            .push(SystemActionEntry { owner, binding });
        Ok(RegistrationLease::new(
            Rc::downgrade(&self.0),
            owner.registration,
            owner.scope,
        ))
    }

    pub fn register_menu_bar(
        &self,
        scope: &ContributionScope,
        bar: MenuBar,
    ) -> Result<RegistrationLease, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        self.0.state.borrow_mut().menus.push(MenuEntry {
            owner,
            contribution: MenuContribution::Bar(bar),
        });
        Ok(RegistrationLease::new(
            Rc::downgrade(&self.0),
            owner.registration,
            owner.scope,
        ))
    }

    pub fn register_context_menu(
        &self,
        scope: &ContributionScope,
        menu: ContextMenu,
    ) -> Result<RegistrationLease, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        self.0.state.borrow_mut().menus.push(MenuEntry {
            owner,
            contribution: MenuContribution::Context(menu),
        });
        Ok(RegistrationLease::new(
            Rc::downgrade(&self.0),
            owner.registration,
            owner.scope,
        ))
    }

    pub fn register_lazy(
        &self,
        scope: &ContributionScope,
        factory: impl FnOnce() -> ContributionBundle + 'static,
    ) -> Result<(LazyContribution, RegistrationLease), SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        let id = {
            let mut state = self.0.state.borrow_mut();
            let id = LazyContributionId(state.next_lazy);
            state.next_lazy = state.next_lazy.saturating_add(1);
            state.lazy.insert(
                id,
                LazyEntry {
                    owner,
                    state: LazyMaterializationState::Unmaterialized,
                    factory: Some(Box::new(factory)),
                    menu: Vec::new(),
                },
            );
            id
        };
        Ok((
            LazyContribution::new(id, owner.registration, Rc::downgrade(&self.0)),
            RegistrationLease::new(Rc::downgrade(&self.0), owner.registration, owner.scope),
        ))
    }

    pub fn lazy_state(&self, id: LazyContributionId) -> Option<LazyMaterializationState> {
        self.0.lazy_state(id)
    }

    pub fn materialize_lazy(&self, id: LazyContributionId) -> bool {
        self.0.materialize_lazy(id)
    }

    pub fn materialize_all(&self) -> usize {
        self.0.materialize_all()
    }

    /// Internal helper for menu resolution: materialize one lazy submenu.
    pub(crate) fn materialize_submenu(&self, id: LazyContributionId) -> Vec<MenuNode> {
        if !self.0.materialize_lazy(id) {
            return Vec::new();
        }
        self.0
            .state
            .borrow()
            .lazy
            .get(&id)
            .map(|entry| entry.menu.clone())
            .unwrap_or_default()
    }

    pub(crate) fn register_menu_bar_retained(
        &self,
        scope: &ContributionScope,
        bar: MenuBar,
    ) -> Result<RegistrationOwner, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        self.0.state.borrow_mut().menus.push(MenuEntry {
            owner,
            contribution: MenuContribution::Bar(bar),
        });
        Ok(owner)
    }

    pub(crate) fn register_context_menu_retained(
        &self,
        scope: &ContributionScope,
        menu: ContextMenu,
    ) -> Result<RegistrationOwner, SystemError> {
        let owner = self.0.begin_owner(Some(scope))?;
        self.0.state.borrow_mut().menus.push(MenuEntry {
            owner,
            contribution: MenuContribution::Context(menu),
        });
        Ok(owner)
    }
}

impl std::fmt::Debug for MenuCommandService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MenuCommandService")
            .field("commands", &self.command_ids().len())
            .field("tasks", &self.tasks().len())
            .field("worker_pool", &self.0.pool_config)
            .finish()
    }
}

/// Access the registered service from a typed registry.
pub fn menu_service(services: &SystemServices) -> Result<Rc<MenuCommandService>, SystemError> {
    services.service::<MenuCommandService>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandId, FakeMenuProvider};

    fn command(id: &str) -> CommandId {
        CommandId::new(id).expect("id")
    }

    #[test]
    fn ephemeral_scope_is_removed_with_its_registration() {
        let service = MenuCommandService::from_provider(FakeMenuProvider::new());
        let descriptor = CommandDescriptor::ui(command("file.save"), "Save").expect("descriptor");
        let registration = service
            .register_ui(descriptor, |_: (), _| Ok(()))
            .expect("registration");
        assert!(service.is_registered(&command("file.save")));
        assert!(registration.lease().revoke());
        assert!(!service.is_registered(&command("file.save")));
        assert!(service.0.state.borrow().scopes.is_empty());
    }

    #[test]
    fn duplicate_registration_preserves_the_first_owner() {
        let service = MenuCommandService::from_provider(FakeMenuProvider::new());
        let descriptor = CommandDescriptor::ui(command("file.save"), "Save").expect("descriptor");
        let _first = service
            .register_ui(descriptor.clone(), |_: (), _| Ok(()))
            .expect("first");
        let error = service
            .register_ui(descriptor, |_: (), _| Ok(()))
            .expect_err("duplicate");
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert_eq!(service.duplicate_diagnostics().len(), 1);
        assert!(service.is_registered(&command("file.save")));
    }

    #[test]
    fn worker_pool_config_is_bounded() {
        assert!(WorkerPoolConfig::new(0, 0).is_err());
        assert!(WorkerPoolConfig::new(1, 0).is_ok());
        assert_eq!(WorkerPoolConfig::new(2, 3).expect("config").max_active(), 5);
        assert!(WorkerPoolConfig::new(65, 0).is_err());
        assert!(WorkerPoolConfig::new(1, 1025).is_err());
    }

    #[test]
    fn command_scope_ids_are_unique() {
        let service = MenuCommandService::from_provider(FakeMenuProvider::new());
        let first = service.create_scope(CommandScope::Global).expect("scope");
        let second = service.create_scope(CommandScope::Global).expect("scope");
        assert_ne!(first.id(), second.id());
        assert!(first.is_active());
        assert!(first.revoke());
        assert!(!first.is_active());
    }
}
