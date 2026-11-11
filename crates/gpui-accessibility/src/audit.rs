//! Modal isolation, virtual-list semantics and whole-scene audits.

use crate::error::SemanticError;
use crate::scene::{
    DisplayedScene, SceneRevision, SemanticIdentity, SemanticRole, SemanticTreeSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Audit result for an active modal/dialog/bottom sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalIsolationReport {
    pub modal_root: SemanticIdentity,
    pub active_focus: Option<SemanticIdentity>,
    pub focus_inside_modal: bool,
    pub modal_present: bool,
    pub background_focusables: Vec<SemanticIdentity>,
    pub leaked_background_controls: Vec<SemanticIdentity>,
}

impl ModalIsolationReport {
    pub fn passed(&self) -> bool {
        self.modal_present && self.focus_inside_modal && self.leaked_background_controls.is_empty()
    }
}

/// Tracks a pre-modal trigger through presentation and dismissal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalIsolationAudit {
    trigger: SemanticIdentity,
}

impl ModalIsolationAudit {
    pub fn new(trigger: SemanticIdentity) -> Self {
        Self { trigger }
    }

    pub fn trigger(&self) -> &SemanticIdentity {
        &self.trigger
    }

    /// Find the preferred active modal root: a modal-flagged dialog first, then
    /// the first visible dialog/bottom sheet in document order.
    pub fn find_active_modal(tree: &SemanticTreeSnapshot) -> Option<SemanticIdentity> {
        tree.nodes
            .iter()
            .find(|node| node.role.is_modal() && node.states.modal && !node.states.hidden)
            .or_else(|| {
                tree.nodes
                    .iter()
                    .find(|node| node.role.is_modal() && !node.states.hidden)
            })
            .map(|node| node.identity.clone())
    }

    /// Inspect the tree while a modal is active. This is independent of the
    /// trigger and is also used by the generic scene audit.
    pub fn inspect(
        tree: &SemanticTreeSnapshot,
        modal_root: &SemanticIdentity,
    ) -> Result<ModalIsolationReport, SemanticError> {
        let modal = tree.node(modal_root).ok_or(SemanticError::UnknownNode {
            node: modal_root.node_id,
        })?;
        let modal_present = !modal.states.hidden && !modal.states.disabled;
        let subtree = tree.descendants(&modal.identity);
        let active_focus = tree.focus.clone();
        let focus_inside_modal = active_focus
            .as_ref()
            .is_some_and(|focus| subtree.contains(&focus.node_id));

        let mut background_focusables = Vec::new();
        let mut leaked_background_controls = Vec::new();
        for node in &tree.nodes {
            if subtree.contains(&node.identity.node_id) || !node.is_interactive_or_focusable() {
                continue;
            }
            background_focusables.push(node.identity.clone());
            if !tree.is_effectively_inert(&node.identity) {
                leaked_background_controls.push(node.identity.clone());
            }
        }
        Ok(ModalIsolationReport {
            modal_root: modal.identity.clone(),
            active_focus,
            focus_inside_modal,
            modal_present,
            background_focusables,
            leaked_background_controls,
        })
    }

    pub fn audit(
        &self,
        tree: &SemanticTreeSnapshot,
        modal_root: &SemanticIdentity,
    ) -> Result<ModalIsolationReport, SemanticError> {
        Self::inspect(tree, modal_root)
    }

    /// Prefer the active modal discovered from the committed tree.
    pub fn audit_active(
        &self,
        tree: &SemanticTreeSnapshot,
    ) -> Result<ModalIsolationReport, SemanticError> {
        let modal = Self::find_active_modal(tree).ok_or(SemanticError::UnknownNode {
            node: tree.root.node_id,
        })?;
        self.audit(tree, &modal)
    }

    /// Verify dismissal: the modal is gone/hidden, the trigger identity is
    /// retained, and focus has returned to that retained trigger identity.
    pub fn audit_dismissal(
        &self,
        tree: &SemanticTreeSnapshot,
        modal_root: &SemanticIdentity,
    ) -> Result<ModalDismissalReport, SemanticError> {
        let modal_dismissed = tree
            .node(modal_root)
            .is_none_or(|modal| modal.states.hidden || modal.states.disabled);
        let trigger = tree.node(&self.trigger);
        let trigger_retained = trigger.is_some_and(|node| !node.states.hidden);
        let active_focus = tree.focus.clone();
        let focus_restored = active_focus
            .as_ref()
            .is_some_and(|focus| focus.same_identity(&self.trigger));
        Ok(ModalDismissalReport {
            modal_root: modal_root.clone(),
            expected_trigger: self.trigger.clone(),
            modal_dismissed,
            trigger_retained,
            active_focus,
            focus_restored,
            passed: modal_dismissed && trigger_retained && focus_restored,
        })
    }
}

/// Result of the dismissal half of a modal audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModalDismissalReport {
    pub modal_root: SemanticIdentity,
    pub expected_trigger: SemanticIdentity,
    pub modal_dismissed: bool,
    pub trigger_retained: bool,
    pub active_focus: Option<SemanticIdentity>,
    pub focus_restored: bool,
    pub passed: bool,
}

/// One virtual-list invariant failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualListViolation {
    MissingBounds(SemanticIdentity),
    MissingSetMetadata(SemanticIdentity),
    InvalidIndex {
        node: SemanticIdentity,
        index: usize,
        size: usize,
    },
    InconsistentSetSize {
        expected: usize,
        found: usize,
    },
    DuplicateIndex(usize),
    NonContiguousWindow,
    OrphanItem(SemanticIdentity),
    OffscreenItem(SemanticIdentity),
    SelectionNotRetained(SemanticIdentity),
}

/// Audit result for one `Role::List` subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualListAuditReport {
    pub scene: DisplayedScene,
    pub list: SemanticIdentity,
    pub visible_items: Vec<SemanticIdentity>,
    pub visible_indices: Vec<usize>,
    pub visible_range: Option<(usize, usize)>,
    pub size_of_set: Option<usize>,
    pub selection: Option<SemanticIdentity>,
    pub selection_visible: bool,
    pub selection_retained: bool,
    pub orphans: Vec<SemanticIdentity>,
    pub offscreen: Vec<SemanticIdentity>,
    pub violations: Vec<VirtualListViolation>,
}

impl VirtualListAuditReport {
    pub fn passed(&self) -> bool {
        self.violations.is_empty() && (self.selection.is_none() || self.selection_retained)
    }
}

/// Stateful list audit. Retaining state lets a selection that scrolls outside
/// the visible window be distinguished from a selection dropped by filtering.
#[derive(Debug, Clone)]
pub struct VirtualListSemanticsAudit {
    list: SemanticIdentity,
    selection: Option<SemanticIdentity>,
    last_selected_index: Option<usize>,
}

impl VirtualListSemanticsAudit {
    pub fn new(list: SemanticIdentity, selection: Option<SemanticIdentity>) -> Self {
        Self {
            list,
            selection,
            last_selected_index: None,
        }
    }

    pub fn list(&self) -> &SemanticIdentity {
        &self.list
    }

    pub fn selection(&self) -> Option<&SemanticIdentity> {
        self.selection.as_ref()
    }

    pub fn audit(
        &mut self,
        tree: &SemanticTreeSnapshot,
    ) -> Result<VirtualListAuditReport, SemanticError> {
        let list = tree.node(&self.list).ok_or(SemanticError::UnknownNode {
            node: self.list.node_id,
        })?;
        if list.role != SemanticRole::List {
            return Err(SemanticError::NotAList {
                node: list.identity.node_id,
            });
        }
        let subtree = tree.descendants(&list.identity);
        let list_bounds = list.bounds;
        let mut violations = Vec::new();
        let mut visible_items = Vec::new();
        let mut visible_indices = Vec::new();
        let mut offscreen = Vec::new();
        let mut expected_size: Option<usize> = None;
        let mut seen_indices = BTreeSet::new();

        for node in &tree.nodes {
            if !subtree.contains(&node.identity.node_id) || node.role != SemanticRole::ListItem {
                continue;
            }
            visible_items.push(node.identity.clone());
            match (node.index_in_set, node.size_of_set) {
                (Some(index), Some(size)) => {
                    if expected_size.is_none() {
                        expected_size = Some(size);
                    } else if expected_size != Some(size) {
                        violations.push(VirtualListViolation::InconsistentSetSize {
                            expected: expected_size.unwrap_or(size),
                            found: size,
                        });
                    }
                    if index >= size {
                        violations.push(VirtualListViolation::InvalidIndex {
                            node: node.identity.clone(),
                            index,
                            size,
                        });
                    }
                    if !seen_indices.insert(index) {
                        violations.push(VirtualListViolation::DuplicateIndex(index));
                    }
                    visible_indices.push(index);
                }
                _ => {
                    violations.push(VirtualListViolation::MissingSetMetadata(
                        node.identity.clone(),
                    ));
                }
            }
            if node.bounds.is_none() {
                violations.push(VirtualListViolation::MissingBounds(node.identity.clone()));
            }
            if let (Some(item_bounds), Some(viewport)) = (node.bounds, list_bounds)
                && !item_bounds.intersects(viewport)
            {
                offscreen.push(node.identity.clone());
                violations.push(VirtualListViolation::OffscreenItem(node.identity.clone()));
            }
        }

        let visible_range = if visible_indices.is_empty() {
            None
        } else {
            Some((
                *visible_indices.iter().min().unwrap_or(&0),
                *visible_indices.iter().max().unwrap_or(&0),
            ))
        };
        if let Some((minimum, maximum)) = visible_range
            && seen_indices.len() != maximum.saturating_sub(minimum).saturating_add(1)
        {
            violations.push(VirtualListViolation::NonContiguousWindow);
        }

        let mut orphans = Vec::new();
        for node in &tree.nodes {
            if node.role != SemanticRole::ListItem {
                continue;
            }
            let has_list_ancestor = tree
                .ancestors(&node.identity)
                .into_iter()
                .any(|ancestor| ancestor.role == SemanticRole::List);
            if !has_list_ancestor {
                orphans.push(node.identity.clone());
                violations.push(VirtualListViolation::OrphanItem(node.identity.clone()));
            }
        }

        let mut selection_visible = false;
        let mut selection_retained = false;
        if let Some(selection) = &self.selection {
            if let Some(found) = visible_items
                .iter()
                .find(|item| item.same_identity(selection))
                .cloned()
            {
                selection_visible = true;
                selection_retained = true;
                if let Some(node) = tree.node(&found) {
                    self.last_selected_index = node.index_in_set;
                }
            } else if tree.node(selection).is_some() {
                // Present but outside this list subtree: not a retained list
                // selection even though the identity exists elsewhere.
                selection_retained = false;
                violations.push(VirtualListViolation::SelectionNotRetained(
                    selection.clone(),
                ));
            } else if let (Some(last), Some((minimum, maximum))) =
                (self.last_selected_index, visible_range)
            {
                // The selected item scrolled outside the visible window. The
                // absent item is expected and retention is proven by the
                // controller's stable identity plus the moved window.
                if !(minimum..=maximum).contains(&last) {
                    selection_retained = true;
                } else {
                    violations.push(VirtualListViolation::SelectionNotRetained(
                        selection.clone(),
                    ));
                }
            } else {
                violations.push(VirtualListViolation::SelectionNotRetained(
                    selection.clone(),
                ));
            }
        }

        Ok(VirtualListAuditReport {
            scene: tree.displayed_scene(),
            list: list.identity.clone(),
            visible_items,
            visible_indices,
            visible_range,
            size_of_set: expected_size,
            selection: self.selection.clone(),
            selection_visible,
            selection_retained,
            orphans,
            offscreen,
            violations,
        })
    }
}

/// Whole-scene audit issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneAuditIssue {
    Structural(SemanticError),
    MissingFocus,
    MultipleModalRoots(Vec<SemanticIdentity>),
    ListAuditFailed(SemanticIdentity),
}

/// Canonical scene audit used by `AccessibilityOperation::AuditScene`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneAuditReport {
    pub scene: DisplayedScene,
    pub revision: SceneRevision,
    pub node_count: usize,
    pub focus: Option<SemanticIdentity>,
    pub modal: Option<ModalIsolationReport>,
    pub lists: Vec<VirtualListAuditReport>,
    pub issues: Vec<SceneAuditIssue>,
}

impl SceneAuditReport {
    pub fn inspect(tree: &SemanticTreeSnapshot) -> Self {
        let mut issues = Vec::new();
        if let Err(error) = tree.validate() {
            issues.push(SceneAuditIssue::Structural(error));
        }
        if tree.focus.is_none() {
            issues.push(SceneAuditIssue::MissingFocus);
        }
        let modals: Vec<_> = tree
            .nodes
            .iter()
            .filter(|node| node.role.is_modal() && !node.states.hidden)
            .map(|node| node.identity.clone())
            .collect();
        if modals.len() > 1 {
            issues.push(SceneAuditIssue::MultipleModalRoots(modals.clone()));
        }
        let modal =
            modals
                .first()
                .and_then(|modal| match ModalIsolationAudit::inspect(tree, modal) {
                    Ok(report) => Some(report),
                    Err(error) => {
                        issues.push(SceneAuditIssue::Structural(error));
                        None
                    }
                });
        let mut lists = Vec::new();
        for list in tree
            .nodes
            .iter()
            .filter(|node| node.role == SemanticRole::List)
        {
            let mut audit = VirtualListSemanticsAudit::new(list.identity.clone(), None);
            match audit.audit(tree) {
                Ok(report) => {
                    if !report.passed() {
                        issues.push(SceneAuditIssue::ListAuditFailed(list.identity.clone()));
                    }
                    lists.push(report);
                }
                Err(error) => {
                    issues.push(SceneAuditIssue::Structural(error));
                    issues.push(SceneAuditIssue::ListAuditFailed(list.identity.clone()));
                }
            }
        }
        Self {
            scene: tree.displayed_scene(),
            revision: tree.revision,
            node_count: tree.nodes.len(),
            focus: tree.focus.clone(),
            modal,
            lists,
            issues,
        }
    }

    pub fn passed(&self) -> bool {
        self.issues.is_empty()
            && self.modal.as_ref().is_none_or(ModalIsolationReport::passed)
            && self.lists.iter().all(VirtualListAuditReport::passed)
    }
}
