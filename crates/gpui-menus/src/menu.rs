//! Native menu structure, system roles and resolved projections.
//!
//! A [`MenuBar`] describes contributions, not native handles. Resolving it
//! evaluates live context, permissions and capabilities, materializes lazy
//! submenu contributions and produces a platform-neutral [`ResolvedMenuBar`]
//! that a provider or component can render.

use crate::{
    CommandId, CommandPayload, ContextExpr, ContextKey, ContributionScopeId, LazyContributionId,
    PermissionKind,
};
use gpui_system::SurfaceId;

/// Platform-recognized menu roles. Hosts map these to native menu equivalents
/// where the platform provides one; other platforms may render them as normal
/// commands or omit unsupported roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SystemMenuRole {
    About,
    Preferences,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Close,
    Minimize,
    Zoom,
    FullScreen,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl SystemMenuRole {
    pub const fn label(self) -> &'static str {
        match self {
            Self::About => "About",
            Self::Preferences => "Preferences",
            Self::Services => "Services",
            Self::Hide => "Hide",
            Self::HideOthers => "Hide Others",
            Self::ShowAll => "Show All",
            Self::Quit => "Quit",
            Self::Close => "Close",
            Self::Minimize => "Minimize",
            Self::Zoom => "Zoom",
            Self::FullScreen => "Toggle Full Screen",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select All",
        }
    }
}

/// One menu bar contribution.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuBar {
    pub menus: Vec<TopLevelMenu>,
}

impl MenuBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_menu(mut self, menu: TopLevelMenu) -> Self {
        self.menus.push(menu);
        self
    }

    pub fn push_menu(&mut self, menu: TopLevelMenu) {
        self.menus.push(menu);
    }
}

/// One top-level menu such as `File`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevelMenu {
    pub title: String,
    pub sections: Vec<MenuSection>,
}

impl TopLevelMenu {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            sections: Vec::new(),
        }
    }

    pub fn with_section(mut self, section: MenuSection) -> Self {
        self.sections.push(section);
        self
    }

    pub fn with_nodes(self, nodes: Vec<MenuNode>) -> Self {
        self.with_section(MenuSection::new(nodes))
    }
}

/// A grouped section inside a top-level menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSection {
    pub title: Option<String>,
    pub nodes: Vec<MenuNode>,
}

impl MenuSection {
    pub fn new(nodes: Vec<MenuNode>) -> Self {
        Self { title: None, nodes }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// One source menu node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuNode {
    Submenu(MenuSubmenu),
    CommandItem(MenuCommandItem),
    CheckItem(MenuCheckItem),
    Separator,
    SystemRole(SystemMenuRole),
}

impl MenuNode {
    pub fn command(command: CommandId) -> Self {
        Self::CommandItem(MenuCommandItem {
            command,
            payload: CommandPayload::Unit,
            visible_when: None,
            enabled_when: None,
        })
    }

    pub fn check(command: CommandId, checked_key: ContextKey) -> Self {
        Self::CheckItem(MenuCheckItem {
            command,
            payload: CommandPayload::Unit,
            checked_key: Some(checked_key),
            visible_when: None,
            enabled_when: None,
        })
    }

    pub fn submenu(title: impl Into<String>, nodes: Vec<MenuNode>) -> Self {
        Self::Submenu(MenuSubmenu {
            title: title.into(),
            nodes,
            lazy: None,
            visible_when: None,
            enabled_when: None,
        })
    }

    pub fn lazy_submenu(title: impl Into<String>, lazy: LazyContributionId) -> Self {
        Self::Submenu(MenuSubmenu {
            title: title.into(),
            nodes: Vec::new(),
            lazy: Some(lazy),
            visible_when: None,
            enabled_when: None,
        })
    }

    pub const fn separator() -> Self {
        Self::Separator
    }

    pub const fn system_role(role: SystemMenuRole) -> Self {
        Self::SystemRole(role)
    }

    pub fn with_payload(mut self, payload: CommandPayload) -> Self {
        match &mut self {
            Self::CommandItem(item) => item.payload = payload,
            Self::CheckItem(item) => item.payload = payload,
            Self::Submenu(_) | Self::Separator | Self::SystemRole(_) => {}
        }
        self
    }

    pub fn visible_when(mut self, expression: ContextExpr) -> Self {
        match &mut self {
            Self::Submenu(item) => item.visible_when = Some(expression),
            Self::CommandItem(item) => item.visible_when = Some(expression),
            Self::CheckItem(item) => item.visible_when = Some(expression),
            Self::Separator | Self::SystemRole(_) => {}
        }
        self
    }

    pub fn enabled_when(mut self, expression: ContextExpr) -> Self {
        match &mut self {
            Self::Submenu(item) => item.enabled_when = Some(expression),
            Self::CommandItem(item) => item.enabled_when = Some(expression),
            Self::CheckItem(item) => item.enabled_when = Some(expression),
            Self::Separator | Self::SystemRole(_) => {}
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSubmenu {
    pub title: String,
    pub nodes: Vec<MenuNode>,
    pub lazy: Option<LazyContributionId>,
    pub visible_when: Option<ContextExpr>,
    pub enabled_when: Option<ContextExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuCommandItem {
    pub command: CommandId,
    pub payload: CommandPayload,
    pub visible_when: Option<ContextExpr>,
    pub enabled_when: Option<ContextExpr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuCheckItem {
    pub command: CommandId,
    pub payload: CommandPayload,
    pub checked_key: Option<ContextKey>,
    pub visible_when: Option<ContextExpr>,
    pub enabled_when: Option<ContextExpr>,
}

/// A context-menu contribution. It is resolved against the presenting surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenu {
    pub title: Option<String>,
    pub nodes: Vec<MenuNode>,
}

impl ContextMenu {
    pub fn new(nodes: Vec<MenuNode>) -> Self {
        Self { title: None, nodes }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Fully evaluated menu bar. Items remain present with `visible = false` so a
/// host can animate or cache them; invisible items must not be activated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedMenuBar {
    pub surface: Option<SurfaceId>,
    pub generation: Option<u64>,
    pub menus: Vec<ResolvedTopLevelMenu>,
}

impl ResolvedMenuBar {
    pub fn all_items(&self) -> Vec<&ResolvedMenuItem> {
        let mut items = Vec::new();
        for menu in &self.menus {
            for section in &menu.sections {
                collect_resolved_items(&section.nodes, &mut items);
            }
        }
        items
    }

    pub fn find_item(&self, command: &CommandId) -> Option<&ResolvedMenuItem> {
        self.all_items()
            .into_iter()
            .find(|item| &item.command_id == command)
    }
}

fn collect_resolved_items<'a>(
    nodes: &'a [ResolvedMenuNode],
    items: &mut Vec<&'a ResolvedMenuItem>,
) {
    for node in nodes {
        match node {
            ResolvedMenuNode::CommandItem(item) | ResolvedMenuNode::CheckItem(item) => {
                items.push(item);
            }
            ResolvedMenuNode::Submenu(submenu) => {
                collect_resolved_items(&submenu.nodes, items);
            }
            ResolvedMenuNode::Separator | ResolvedMenuNode::SystemRole(_) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTopLevelMenu {
    pub title: String,
    pub sections: Vec<ResolvedMenuSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMenuSection {
    pub title: Option<String>,
    pub nodes: Vec<ResolvedMenuNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSubmenu {
    pub title: String,
    pub visible: bool,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub nodes: Vec<ResolvedMenuNode>,
}

/// Resolved menu item state. `enabled` and `visible` are projections only;
/// invocation always re-validates permissions, context and capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMenuItem {
    pub command_id: CommandId,
    pub title: String,
    pub payload: CommandPayload,
    pub visible: bool,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub disabled_reason: Option<String>,
    pub shortcut_hint: Option<String>,
    pub surface: Option<SurfaceId>,
    pub generation: Option<u64>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedMenuNode {
    Submenu(ResolvedSubmenu),
    CommandItem(ResolvedMenuItem),
    CheckItem(ResolvedMenuItem),
    Separator,
    SystemRole(SystemMenuRole),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedContextMenu {
    pub title: Option<String>,
    pub surface: Option<SurfaceId>,
    pub generation: Option<u64>,
    pub nodes: Vec<ResolvedMenuNode>,
}

impl ResolvedContextMenu {
    pub fn all_items(&self) -> Vec<&ResolvedMenuItem> {
        let mut items = Vec::new();
        collect_resolved_items(&self.nodes, &mut items);
        items
    }
}

/// Source marker for a contribution that was materialized from a lazy bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContributionSource {
    pub scope: ContributionScopeId,
    pub lazy: Option<LazyContributionId>,
}

/// A provider-facing description of the permissions a menu command needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuPermissionSummary {
    pub required: Vec<PermissionKind>,
}
