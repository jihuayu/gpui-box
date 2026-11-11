//! Resolution and projection for all five command trigger surfaces.

use crate::menu::{
    MenuBar, MenuNode, MenuSubmenu, ResolvedContextMenu, ResolvedMenuBar, ResolvedMenuItem,
    ResolvedMenuNode, ResolvedMenuSection, ResolvedSubmenu, ResolvedTopLevelMenu,
};
use crate::service::{CommandEntry, KeybindingEntry, MenuCommandService, SystemActionEntry};
use crate::{
    ButtonCommandBinding, ButtonCommandState, CancelReason, CommandId, CommandPayload,
    CommandScope, Completion, ContextExpr, ContributionScope, ErrorCode, InvocationContext,
    KeystrokeSpec, MenusOperation, PaletteEntry, PaletteQuery, PermissionKind, PermissionProvider,
    ResolvedKeybinding, RouteRegistry, ServiceResult, SurfaceId, SystemActionBinding,
    SystemActionTarget, SystemError, TriggerSource,
};
use gpui_activation::{ActivationEvent, ActivationPayload};
use std::collections::BTreeMap;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedCommandState {
    pub visible: bool,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub checked: Option<bool>,
}

fn eval_expr(
    service: &crate::service::ServiceInner,
    expression: &ContextExpr,
    context: &InvocationContext,
    fallback_scope: Option<crate::ContributionScopeId>,
) -> bool {
    let scope = context.scope.or(fallback_scope).map(|scope| scope.0);
    service.state.borrow().contexts.evaluate(
        expression,
        context.effective_surface(),
        scope,
        context.overlay.as_ref(),
    )
}

fn eval_key(
    service: &crate::service::ServiceInner,
    key: &crate::ContextKey,
    context: &InvocationContext,
    fallback_scope: Option<crate::ContributionScopeId>,
) -> Option<bool> {
    let scope = context.scope.or(fallback_scope).map(|scope| scope.0);
    service
        .state
        .borrow()
        .contexts
        .get(
            key,
            context.effective_surface(),
            scope,
            context.overlay.as_ref(),
        )
        .and_then(|value| value.as_bool())
}

pub(crate) fn check_capabilities(
    service: &crate::service::ServiceInner,
    descriptor: &crate::CommandDescriptor,
) -> Result<(), SystemError> {
    service.capability(MenusOperation::InvokeCommand).check()?;
    for capability in &descriptor.required_capabilities {
        service.capability(*capability).check()?;
    }
    Ok(())
}

pub(crate) fn check_permissions(
    service: &crate::service::ServiceInner,
    descriptor: &crate::CommandDescriptor,
) -> Result<(), SystemError> {
    if descriptor.required_permissions.is_empty() {
        return Ok(());
    }
    let provider = service.permission_provider().ok_or_else(|| {
        SystemError::new(
            ErrorCode::InvalidConfiguration,
            "Command permissions require a permission provider",
        )
    })?;
    check_permissions_with(provider.as_ref(), &descriptor.required_permissions)
}

pub(crate) fn check_permissions_with(
    provider: &dyn PermissionProvider,
    permissions: &[PermissionKind],
) -> Result<(), SystemError> {
    for kind in permissions {
        let snapshot = provider.query(*kind)?;
        if !snapshot.allows_use() {
            let mut error =
                SystemError::new(ErrorCode::PermissionDenied, "Permission is not authorized");
            error.recovery = snapshot.status.recovery();
            error.retry = snapshot
                .status
                .error()
                .map_or(gpui_system::RetryAdvice::AfterConditionsChange, |error| {
                    error.retry
                });
            return Err(error);
        }
    }
    Ok(())
}

fn disabled_reason_for_permission(
    descriptor: &crate::CommandDescriptor,
    error: &SystemError,
) -> String {
    if error.code == ErrorCode::PermissionDenied
        && let Some(kind) = descriptor.required_permissions.first()
    {
        return format!("Permission required: {}", kind.label());
    }
    error.message.to_owned()
}

pub(crate) fn project_command_state(
    service: &crate::service::ServiceInner,
    descriptor: &crate::CommandDescriptor,
    visible_override: Option<&ContextExpr>,
    enabled_override: Option<&ContextExpr>,
    context: &InvocationContext,
    fallback_scope: Option<crate::ContributionScopeId>,
) -> ProjectedCommandState {
    let mut visible = eval_expr(service, &descriptor.visible_when, context, fallback_scope);
    if let Some(expression) = visible_override {
        visible = visible && eval_expr(service, expression, context, fallback_scope);
    }
    let mut enabled = eval_expr(service, &descriptor.enabled_when, context, fallback_scope);
    if let Some(expression) = enabled_override {
        enabled = enabled && eval_expr(service, expression, context, fallback_scope);
    }
    let mut disabled_reason = if !enabled {
        Some("Command is disabled in the current context".to_owned())
    } else {
        None
    };
    if enabled {
        if let Err(error) = check_capabilities(service, descriptor) {
            enabled = false;
            disabled_reason = Some(error.message.to_owned());
        } else if let Err(error) = check_permissions(service, descriptor) {
            enabled = false;
            disabled_reason = Some(disabled_reason_for_permission(descriptor, &error));
        }
    }
    let checked = descriptor
        .checked_key
        .as_ref()
        .map(|key| eval_key(service, key, context, fallback_scope).unwrap_or(false));
    ProjectedCommandState {
        visible,
        enabled,
        disabled_reason,
        checked,
    }
}

fn require_ui_surface(
    service: &crate::service::ServiceInner,
    context: &InvocationContext,
) -> Result<(SurfaceId, u64), SystemError> {
    let ui = context.operation.ui.as_ref().ok_or_else(|| {
        SystemError::new(
            ErrorCode::InvalidInput,
            "This trigger surface requires a UI context",
        )
    })?;
    context.operation.check_at(Instant::now())?;
    context.operation.require_ui()?;
    if let Some(surface) = context.surface
        && surface != ui.surface
    {
        return Err(SystemError::new(
            ErrorCode::InvalidInput,
            "Invocation target surface does not match the UI context",
        ));
    }
    let active = service
        .surface_generation(ui.surface)
        .ok_or_else(|| SystemError::new(ErrorCode::ContextExpired, "Surface has been disposed"))?;
    if active != ui.generation {
        return Err(SystemError::new(
            ErrorCode::ContextExpired,
            "UI context generation is no longer live",
        ));
    }
    Ok((ui.surface, ui.generation))
}

fn cancellation_of(context: &InvocationContext) -> Option<CancelReason> {
    context.operation.cancellation.reason()
}

fn scope_specificity(
    service: &crate::service::ServiceInner,
    scope: &CommandScope,
    context: &InvocationContext,
) -> Option<u8> {
    match scope {
        CommandScope::Global => Some(1),
        CommandScope::Surface {
            surface,
            generation,
        } => {
            if context.effective_surface() != Some(*surface) {
                return None;
            }
            if service.surface_generation(*surface) != Some(*generation) {
                return None;
            }
            if let Some(ui) = context.ui()
                && ui.generation != *generation
            {
                return None;
            }
            Some(2)
        }
        CommandScope::UiOwner(owner) => {
            let current = owner.context(None).ok()?;
            if context.effective_surface() != Some(current.surface) {
                return None;
            }
            if service.surface_generation(current.surface) != Some(current.generation) {
                return None;
            }
            Some(3)
        }
    }
}

fn select_binding(
    service: &crate::service::ServiceInner,
    context: &InvocationContext,
    chord: Option<&KeystrokeSpec>,
    command: Option<&CommandId>,
    materialize: bool,
) -> Option<ResolvedKeybinding> {
    if materialize {
        service.materialize_all();
    }
    let candidates: Vec<KeybindingEntry> = service
        .state
        .borrow()
        .keybindings
        .iter()
        .filter(|entry| {
            chord.is_none_or(|chord| &entry.binding.chord == chord)
                && command.is_none_or(|command| &entry.binding.command_id == command)
        })
        .cloned()
        .collect();
    let mut winner: Option<(u8, i32, u64, ResolvedKeybinding)> = None;
    for entry in candidates {
        let Some(specificity) = scope_specificity(service, &entry.binding.scope, context) else {
            continue;
        };
        let scope = context.scope.or(Some(entry.owner.scope));
        let enabled = service.state.borrow().contexts.evaluate(
            &entry.binding.when,
            context.effective_surface(),
            scope.map(|scope| scope.0),
            context.overlay.as_ref(),
        );
        if !enabled {
            continue;
        }
        let resolved = ResolvedKeybinding {
            chord: entry.binding.chord.clone(),
            command_id: entry.binding.command_id.clone(),
            payload: entry.binding.payload.clone(),
            priority: entry.binding.priority,
            scope_specificity: specificity,
            registration_order: entry.order,
        };
        let score = (specificity, entry.binding.priority, entry.order);
        if winner
            .as_ref()
            .is_none_or(|(s, p, o, _)| score > (*s, *p, *o))
        {
            winner = Some((specificity, entry.binding.priority, entry.order, resolved));
        }
    }
    winner.map(|(_, _, _, resolved)| resolved)
}

pub(crate) fn best_shortcut(
    service: &crate::service::ServiceInner,
    command: &CommandId,
    context: &InvocationContext,
    materialize: bool,
) -> Option<String> {
    select_binding(service, context, None, Some(command), materialize)
        .map(|binding| binding.shortcut_hint())
}

fn resolved_menu_item(
    service: &crate::service::ServiceInner,
    command: &CommandId,
    payload: CommandPayload,
    checked_override: Option<&crate::ContextKey>,
    visible_override: Option<&ContextExpr>,
    enabled_override: Option<&ContextExpr>,
    context: &InvocationContext,
) -> ResolvedMenuItem {
    let ui = context.ui();
    let surface = ui.map(|ui| ui.surface);
    let generation = ui.map(|ui| ui.generation);
    match service.ensure_command(command) {
        Some(entry) => {
            let projection = project_command_state(
                service,
                &entry.descriptor,
                visible_override,
                enabled_override,
                context,
                Some(entry.owner.scope),
            );
            let checked = checked_override.map_or(projection.checked, |key| {
                Some(eval_key(service, key, context, Some(entry.owner.scope)).unwrap_or(false))
            });
            let shortcut_hint = best_shortcut(service, command, context, false);
            ResolvedMenuItem {
                command_id: command.clone(),
                title: entry.descriptor.title.clone(),
                payload,
                visible: projection.visible,
                enabled: projection.enabled,
                checked,
                disabled_reason: projection.disabled_reason,
                shortcut_hint,
                surface,
                generation,
            }
        }
        None => {
            let visible = visible_override
                .is_none_or(|expression| eval_expr(service, expression, context, None));
            ResolvedMenuItem {
                command_id: command.clone(),
                title: command.as_str().to_owned(),
                payload,
                visible,
                enabled: false,
                checked: checked_override.map(|_| false),
                disabled_reason: Some("Command is not registered".to_owned()),
                shortcut_hint: None,
                surface,
                generation,
            }
        }
    }
}

fn node_has_visible_enabled(node: &ResolvedMenuNode) -> bool {
    match node {
        ResolvedMenuNode::CommandItem(item) | ResolvedMenuNode::CheckItem(item) => {
            item.visible && item.enabled
        }
        ResolvedMenuNode::Submenu(submenu) => submenu.visible && submenu.enabled,
        ResolvedMenuNode::Separator => false,
        ResolvedMenuNode::SystemRole(_) => true,
    }
}

impl MenuCommandService {
    // ---- menu surfaces ---------------------------------------------------

    pub fn resolve_menu_bar(
        &self,
        bar: &MenuBar,
        context: &InvocationContext,
    ) -> Result<ResolvedMenuBar, SystemError> {
        let ui = context.ui();
        let menus = bar
            .menus
            .iter()
            .map(|menu| ResolvedTopLevelMenu {
                title: menu.title.clone(),
                sections: menu
                    .sections
                    .iter()
                    .map(|section| ResolvedMenuSection {
                        title: section.title.clone(),
                        nodes: self.resolve_menu_nodes(&section.nodes, context, 0),
                    })
                    .collect(),
            })
            .collect();
        Ok(ResolvedMenuBar {
            surface: ui.map(|ui| ui.surface),
            generation: ui.map(|ui| ui.generation),
            menus,
        })
    }

    fn resolve_menu_nodes(
        &self,
        nodes: &[MenuNode],
        context: &InvocationContext,
        depth: usize,
    ) -> Vec<ResolvedMenuNode> {
        if depth >= 32 {
            return Vec::new();
        }
        nodes
            .iter()
            .map(|node| match node {
                MenuNode::Submenu(submenu) => {
                    ResolvedMenuNode::Submenu(self.resolve_submenu(submenu, context, depth + 1))
                }
                MenuNode::CommandItem(item) => ResolvedMenuNode::CommandItem(resolved_menu_item(
                    &self.0,
                    &item.command,
                    item.payload.clone(),
                    None,
                    item.visible_when.as_ref(),
                    item.enabled_when.as_ref(),
                    context,
                )),
                MenuNode::CheckItem(item) => ResolvedMenuNode::CheckItem(resolved_menu_item(
                    &self.0,
                    &item.command,
                    item.payload.clone(),
                    item.checked_key.as_ref(),
                    item.visible_when.as_ref(),
                    item.enabled_when.as_ref(),
                    context,
                )),
                MenuNode::Separator => ResolvedMenuNode::Separator,
                MenuNode::SystemRole(role) => ResolvedMenuNode::SystemRole(*role),
            })
            .collect()
    }

    fn resolve_submenu(
        &self,
        submenu: &MenuSubmenu,
        context: &InvocationContext,
        depth: usize,
    ) -> ResolvedSubmenu {
        let visible = submenu
            .visible_when
            .as_ref()
            .is_none_or(|expression| eval_expr(&self.0, expression, context, None));
        if !visible {
            return ResolvedSubmenu {
                title: submenu.title.clone(),
                visible: false,
                enabled: false,
                disabled_reason: None,
                nodes: Vec::new(),
            };
        }
        let mut lazy_available = true;
        let mut nodes = submenu.nodes.clone();
        if let Some(lazy) = submenu.lazy {
            lazy_available = self.0.lazy_state(lazy).is_some();
            nodes.extend(self.materialize_submenu(lazy));
        }
        let resolved = self.resolve_menu_nodes(&nodes, context, depth);
        let expression_enabled = submenu
            .enabled_when
            .as_ref()
            .is_none_or(|expression| eval_expr(&self.0, expression, context, None));
        let child_enabled = resolved.iter().any(node_has_visible_enabled);
        let enabled = lazy_available && expression_enabled && child_enabled;
        let disabled_reason = if !lazy_available {
            Some("Contribution is no longer available".to_owned())
        } else if !expression_enabled {
            Some("Menu is disabled in the current context".to_owned())
        } else if !child_enabled {
            Some("No menu item is currently available".to_owned())
        } else {
            None
        };
        ResolvedSubmenu {
            title: submenu.title.clone(),
            visible,
            enabled,
            disabled_reason,
            nodes: resolved,
        }
    }

    pub fn install_menu_bar(
        &self,
        scope: &ContributionScope,
        bar: MenuBar,
        context: InvocationContext,
    ) -> ServiceResult<ResolvedMenuBar> {
        let (surface, generation) = require_ui_surface(&self.0, &context)?;
        if let Some(reason) = cancellation_of(&context) {
            return Ok(Completion::Cancelled(reason));
        }
        self.0.capability(MenusOperation::InstallMenuBar).check()?;
        let resolved = self.resolve_menu_bar(&bar, &context)?;
        let owner = self.register_menu_bar_retained(scope, bar)?;
        if let Err(error) = self.0.provider.install_menu_bar(&resolved) {
            self.0.revoke_registration(owner.registration);
            return Err(error);
        }
        debug_assert_eq!(resolved.surface, Some(surface));
        debug_assert_eq!(resolved.generation, Some(generation));
        Ok(Completion::Completed(resolved))
    }

    pub fn install_menu_bar_owned(
        &self,
        scope: &ContributionScope,
        bar: MenuBar,
        context: InvocationContext,
    ) -> ServiceResult<ResolvedMenuBar> {
        self.install_menu_bar(scope, bar, context)
    }

    pub fn present_context_menu(
        &self,
        scope: &ContributionScope,
        menu: crate::ContextMenu,
        context: InvocationContext,
    ) -> ServiceResult<ResolvedContextMenu> {
        let (surface, generation) = require_ui_surface(&self.0, &context)?;
        if let Some(reason) = cancellation_of(&context) {
            return Ok(Completion::Cancelled(reason));
        }
        self.0
            .capability(MenusOperation::PresentContextMenu)
            .check()?;
        let nodes = self.resolve_menu_nodes(&menu.nodes, &context, 0);
        let resolved = ResolvedContextMenu {
            title: menu.title.clone(),
            surface: Some(surface),
            generation: Some(generation),
            nodes,
        };
        let owner = self.register_context_menu_retained(scope, menu)?;
        let anchor = context.operation.ui.as_ref().and_then(|ui| ui.anchor);
        if let Err(error) = self
            .0
            .provider
            .present_context_menu(&resolved, surface, generation, anchor)
        {
            self.0.revoke_registration(owner.registration);
            return Err(error);
        }
        Ok(Completion::Completed(resolved))
    }

    fn reject_stale_item(
        &self,
        item: &ResolvedMenuItem,
        context: &InvocationContext,
    ) -> Result<(), SystemError> {
        if let Some(surface) = item.surface {
            let live = self.0.surface_generation(surface);
            if item.generation.is_some() && live != item.generation {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Menu item belongs to an expired surface generation",
                ));
            }
            if let Some(ui) = context.ui()
                && (ui.surface != surface || Some(ui.generation) != item.generation)
            {
                return Err(SystemError::new(
                    ErrorCode::ContextExpired,
                    "Menu item does not match the current UI generation",
                ));
            }
        }
        if !item.visible {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Menu item is not visible",
            ));
        }
        if !item.enabled {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Menu item is disabled",
            ));
        }
        Ok(())
    }

    pub fn activate_menu_item(
        &self,
        item: &ResolvedMenuItem,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        if let Err(error) = self.reject_stale_item(item, &context) {
            return crate::invoke::failed_request(&context, error);
        }
        self.invoke_command(
            &item.command_id,
            item.payload.clone(),
            context.with_trigger(TriggerSource::Menu),
        )
    }

    pub fn activate_context_menu_item(
        &self,
        item: &ResolvedMenuItem,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        if let Err(error) = self.reject_stale_item(item, &context) {
            return crate::invoke::failed_request(&context, error);
        }
        self.invoke_command(
            &item.command_id,
            item.payload.clone(),
            context.with_trigger(TriggerSource::ContextMenu),
        )
    }

    // ---- keybindings -----------------------------------------------------

    pub fn resolve_keychord(
        &self,
        chord: &KeystrokeSpec,
        context: &InvocationContext,
    ) -> Option<ResolvedKeybinding> {
        select_binding(&self.0, context, Some(chord), None, true)
    }

    pub fn sync_keybindings(
        &self,
        context: InvocationContext,
    ) -> ServiceResult<Vec<ResolvedKeybinding>> {
        require_ui_surface(&self.0, &context)?;
        if let Some(reason) = cancellation_of(&context) {
            return Ok(Completion::Cancelled(reason));
        }
        self.0.capability(MenusOperation::SyncKeybindings).check()?;
        self.0.materialize_all();
        let chords: BTreeMap<String, KeystrokeSpec> = self
            .0
            .state
            .borrow()
            .keybindings
            .iter()
            .map(|entry| (entry.binding.chord.to_string(), entry.binding.chord.clone()))
            .collect();
        let mut resolved = BTreeMap::new();
        for (label, chord) in chords {
            if let Some(winner) = select_binding(&self.0, &context, Some(&chord), None, false) {
                resolved.insert(label, winner);
            }
        }
        let bindings: Vec<ResolvedKeybinding> = resolved.into_values().collect();
        self.0.provider.sync_keybindings(&bindings)?;
        Ok(Completion::Completed(bindings))
    }

    pub fn dispatch_keychord(
        &self,
        chord: &KeystrokeSpec,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        if let Err(error) = require_ui_surface(&self.0, &context) {
            return crate::invoke::failed_request(&context, error);
        }
        let Some(binding) = self.resolve_keychord(chord, &context) else {
            return crate::invoke::failed_request(
                &context,
                SystemError::new(ErrorCode::Unavailable, "No keybinding matches the chord"),
            );
        };
        self.invoke_command(
            &binding.command_id,
            binding.payload,
            context.with_trigger(TriggerSource::Keybinding),
        )
    }

    // ---- palette ---------------------------------------------------------

    pub fn query_palette(
        &self,
        query: PaletteQuery,
        context: InvocationContext,
    ) -> ServiceResult<Vec<PaletteEntry>> {
        require_ui_surface(&self.0, &context)?;
        if let Some(reason) = cancellation_of(&context) {
            return Ok(Completion::Cancelled(reason));
        }
        self.0.capability(MenusOperation::QueryPalette).check()?;
        self.0.materialize_all();
        let limit = query.limit.min(crate::palette::MAX_PALETTE_LIMIT);
        if limit == 0 {
            return Ok(Completion::Completed(Vec::new()));
        }
        let commands: Vec<CommandEntry> =
            self.0.state.borrow().commands.values().cloned().collect();
        let mut entries = Vec::new();
        for entry in commands {
            if let Some(category) = &query.category
                && entry
                    .descriptor
                    .category
                    .as_deref()
                    .is_none_or(|value| !value.eq_ignore_ascii_case(category))
            {
                continue;
            }
            let Some(score) = crate::palette::palette_score(
                &query.text,
                &entry.descriptor.title,
                &entry.descriptor.id,
            ) else {
                continue;
            };
            let projection = project_command_state(
                &self.0,
                &entry.descriptor,
                None,
                None,
                &context,
                Some(entry.owner.scope),
            );
            if !projection.visible {
                continue;
            }
            entries.push(PaletteEntry {
                command_id: entry.descriptor.id.clone(),
                title: entry.descriptor.title.clone(),
                category: entry.descriptor.category.clone(),
                summary: entry.descriptor.summary.clone(),
                score,
                shortcut_hint: best_shortcut(&self.0, &entry.descriptor.id, &context, false),
                enabled: projection.enabled,
                disabled_reason: projection.disabled_reason,
            });
        }
        entries.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.title.cmp(&right.title))
                .then_with(|| left.command_id.cmp(&right.command_id))
        });
        entries.truncate(limit);
        Ok(Completion::Completed(entries))
    }

    pub fn invoke_palette_entry(
        &self,
        entry: &PaletteEntry,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        self.invoke_command(
            &entry.command_id,
            CommandPayload::Unit,
            context.with_trigger(TriggerSource::Palette),
        )
    }

    // ---- buttons ---------------------------------------------------------

    pub fn project_button(
        &self,
        binding: &ButtonCommandBinding,
        context: &InvocationContext,
    ) -> Result<ButtonCommandState, SystemError> {
        let Some(entry) = self.0.ensure_command(&binding.command_id) else {
            return Err(SystemError::new(
                ErrorCode::Unavailable,
                "Button command is not registered",
            ));
        };
        let projection = project_command_state(
            &self.0,
            &entry.descriptor,
            Some(&binding.visible_when),
            Some(&binding.enabled_when),
            context,
            Some(entry.owner.scope),
        );
        let label = binding
            .label
            .clone()
            .unwrap_or_else(|| entry.descriptor.title.clone());
        Ok(ButtonCommandState {
            accessible_name: label.clone(),
            label,
            summary: entry.descriptor.summary.clone().unwrap_or_default(),
            accessible_checked: projection.checked,
            visible: projection.visible,
            enabled: projection.enabled,
            disabled_reason: projection.disabled_reason,
            shortcut_hint: best_shortcut(&self.0, &binding.command_id, context, false),
        })
    }

    pub fn button_states(
        &self,
        context: &InvocationContext,
    ) -> Result<Vec<ButtonCommandState>, SystemError> {
        self.0.materialize_all();
        let bindings: Vec<ButtonCommandBinding> = self
            .0
            .state
            .borrow()
            .buttons
            .iter()
            .map(|entry| entry.binding.clone())
            .collect();
        let mut states = Vec::new();
        for binding in bindings {
            if let Ok(state) = self.project_button(&binding, context) {
                states.push(state);
            }
        }
        Ok(states)
    }

    pub fn invoke_button(
        &self,
        binding: &ButtonCommandBinding,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        if let Err(error) = require_ui_surface(&self.0, &context) {
            return crate::invoke::failed_request(&context, error);
        }
        let fallback_scope = self
            .0
            .ensure_command(&binding.command_id)
            .map(|entry| entry.owner.scope);
        if !eval_expr(&self.0, &binding.visible_when, &context, fallback_scope) {
            return crate::invoke::failed_request(
                &context,
                SystemError::new(ErrorCode::InvalidInput, "Button is not visible"),
            );
        }
        if !eval_expr(&self.0, &binding.enabled_when, &context, fallback_scope) {
            return crate::invoke::failed_request(
                &context,
                SystemError::new(ErrorCode::InvalidInput, "Button is disabled"),
            );
        }
        self.invoke_command(
            &binding.command_id,
            binding.payload.clone(),
            context.with_trigger(TriggerSource::Button),
        )
    }

    // ---- system actions and activations ----------------------------------

    pub fn dispatch_system_action(
        &self,
        action_id: &str,
        parameters: &BTreeMap<String, String>,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        self.dispatch_action_with_trigger(
            action_id,
            parameters,
            context,
            TriggerSource::SystemAction,
        )
    }

    fn dispatch_action_with_trigger(
        &self,
        action_id: &str,
        parameters: &BTreeMap<String, String>,
        context: InvocationContext,
        trigger: TriggerSource,
    ) -> gpui_system::Request<CommandPayload> {
        if action_id.is_empty() || action_id.len() > 128 {
            return crate::invoke::failed_request(
                &context,
                SystemError::new(ErrorCode::InvalidInput, "System action identity is invalid"),
            );
        }
        self.0.materialize_all();
        let target = SystemActionTarget::ActionId(action_id.to_owned());
        self.dispatch_system_binding(&target, parameters, context, trigger)
    }

    pub fn dispatch_activation(
        &self,
        event: &ActivationEvent,
        routes: &RouteRegistry,
        context: InvocationContext,
    ) -> gpui_system::Request<CommandPayload> {
        let target = match event.payload() {
            ActivationPayload::Route(target) | ActivationPayload::Notification { target, .. } => {
                target.clone()
            }
            ActivationPayload::Activate => {
                return self.dispatch_action_with_trigger(
                    "activate",
                    &BTreeMap::new(),
                    context,
                    TriggerSource::Activation,
                );
            }
            ActivationPayload::OpenUrl(_) => {
                return crate::invoke::failed_request(
                    &context,
                    SystemError::new(
                        ErrorCode::Unsupported,
                        "URL activation is not a menu command route",
                    ),
                );
            }
        };
        if let Err(error) = routes.validate(&target) {
            return crate::invoke::failed_request(&context, error);
        }
        let parameters = target.parameters().clone();
        self.dispatch_system_binding(
            &SystemActionTarget::Activation(target),
            &parameters,
            context,
            TriggerSource::Activation,
        )
    }

    fn dispatch_system_binding(
        &self,
        target: &SystemActionTarget,
        parameters: &BTreeMap<String, String>,
        context: InvocationContext,
        trigger: TriggerSource,
    ) -> gpui_system::Request<CommandPayload> {
        let bindings: Vec<SystemActionEntry> = self.0.state.borrow().system_actions.to_vec();
        let mut winner: Option<(u8, u64, SystemActionBinding)> = None;
        for entry in bindings {
            if !target_matches(&entry.binding.target, target) {
                continue;
            }
            let Some(specificity) = registration_specificity(&self.0, entry.owner.scope, &context)
            else {
                continue;
            };
            let scope = context.scope.or(Some(entry.owner.scope));
            let enabled = self.0.state.borrow().contexts.evaluate(
                &entry.binding.when,
                context.effective_surface(),
                scope.map(|scope| scope.0),
                context.overlay.as_ref(),
            );
            if !enabled {
                continue;
            }
            let score = (specificity, entry.owner.sequence);
            if winner
                .as_ref()
                .is_none_or(|(current_specificity, order, _)| {
                    score.0 > *current_specificity
                        || (score.0 == *current_specificity && score.1 > *order)
                })
            {
                winner = Some((score.0, score.1, entry.binding));
            }
        }
        let Some((_, _, binding)) = winner else {
            return crate::invoke::failed_request(
                &context,
                SystemError::new(ErrorCode::Unavailable, "No system action binding matches"),
            );
        };
        let payload = match binding.apply_parameters(parameters) {
            Ok(payload) => payload,
            Err(error) => return crate::invoke::failed_request(&context, error),
        };
        self.invoke_command(&binding.command_id, payload, context.with_trigger(trigger))
    }
}

fn registration_specificity(
    service: &crate::service::ServiceInner,
    scope: crate::ContributionScopeId,
    context: &InvocationContext,
) -> Option<u8> {
    let binding = service
        .state
        .borrow()
        .scopes
        .get(&scope)
        .map(|record| record.binding.clone())?;
    match binding {
        crate::scope::ScopeBinding::Global => Some(1),
        crate::scope::ScopeBinding::Surface {
            surface,
            generation,
        } => {
            if context.effective_surface() != Some(surface) {
                return None;
            }
            (service.surface_generation(surface) == Some(generation)).then_some(2)
        }
        crate::scope::ScopeBinding::UiOwner {
            owner,
            surface,
            generation,
        } => {
            let current = owner.context(None).ok()?;
            if context.effective_surface() != Some(surface)
                || current.surface != surface
                || current.generation != generation
            {
                return None;
            }
            (service.surface_generation(surface) == Some(generation)).then_some(3)
        }
    }
}

fn target_matches(binding: &SystemActionTarget, requested: &SystemActionTarget) -> bool {
    match (binding, requested) {
        (SystemActionTarget::ActionId(left), SystemActionTarget::ActionId(right)) => left == right,
        (SystemActionTarget::Activation(left), SystemActionTarget::Activation(right)) => {
            left.route() == right.route() && left.version() == right.version()
        }
        _ => false,
    }
}

/// Convenience for tests: build an invocation context for a UI owner.
pub fn invocation_context(
    owner: &crate::UiOwner,
    trigger: TriggerSource,
) -> Result<InvocationContext, SystemError> {
    let operation = gpui_system::OperationContext {
        ui: Some(owner.context(None)?),
        ..gpui_system::OperationContext::default()
    };
    Ok(InvocationContext::new(operation).with_trigger(trigger))
}

/// Build a context-menu invocation context with an explicit anchor.
pub fn context_menu_context(
    owner: &crate::UiOwner,
    anchor: gpui_system::LogicalRect,
) -> Result<InvocationContext, SystemError> {
    let operation = gpui_system::OperationContext {
        ui: Some(owner.context(Some(anchor))?),
        ..gpui_system::OperationContext::default()
    };
    Ok(InvocationContext::new(operation).with_trigger(TriggerSource::ContextMenu))
}
