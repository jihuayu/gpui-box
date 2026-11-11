//! Live `gpui-menus` resolution for status-item context menus.
//!
//! `gpui-menus` exposes menu-bar resolution as the authoritative projection
//! path. A tray attaches a [`gpui_menus::ContextMenu`], which is structurally a
//! single-section menu subtree. This adapter projects that subtree through
//! `MenuCommandService::resolve_menu_bar` without registering a second command
//! registry or duplicating enabled/hidden/checked state.

use gpui_menus::{
    ContextMenu, InvocationContext, MenuBar, MenuCommandService, MenuSection, ResolvedContextMenu,
    TopLevelMenu,
};
use gpui_system::SystemError;

/// Resolve a context menu against live command and context state.
pub trait TrayContextMenuResolver {
    /// Evaluate every node and command in `menu` for `context`.
    ///
    /// The result keeps invisible and disabled items so a host can animate or
    /// cache them; activation paths must still reject them.
    fn resolve_context_menu(
        &self,
        menu: &ContextMenu,
        context: &InvocationContext,
    ) -> Result<ResolvedContextMenu, SystemError>;
}

impl TrayContextMenuResolver for MenuCommandService {
    fn resolve_context_menu(
        &self,
        menu: &ContextMenu,
        context: &InvocationContext,
    ) -> Result<ResolvedContextMenu, SystemError> {
        let title = menu.title.clone().unwrap_or_default();
        let bar = MenuBar::new()
            .with_menu(TopLevelMenu::new(title).with_section(MenuSection::new(menu.nodes.clone())));
        let resolved = self.resolve_menu_bar(&bar, context)?;
        let nodes = resolved
            .menus
            .into_iter()
            .next()
            .and_then(|menu| menu.sections.into_iter().next())
            .map(|section| section.nodes)
            .unwrap_or_default();
        Ok(ResolvedContextMenu {
            title: menu.title.clone(),
            surface: resolved.surface,
            generation: resolved.generation,
            nodes,
        })
    }
}
