//! AccessKit to Android virtual-node transport. Kotlin owns native node objects.
use super::bridge;
use accesskit::{Action, ActionData, ActionRequest, Node, NodeId, Role, TreeId, TreeUpdate};
use gpui::A11yCallbacks;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Default)]
pub(super) struct Accessibility {
    pub callbacks: Option<Rc<A11yCallbacks>>,
    active: bool,
    root: Option<NodeId>,
    focus: Option<NodeId>,
    nodes: BTreeMap<NodeId, Node>,
}
impl Accessibility {
    pub fn update(&mut self, update: TreeUpdate) {
        if let Some(tree) = update.tree {
            self.root = Some(tree.root);
        }
        self.focus = Some(update.focus);
        self.nodes.extend(update.nodes);
        // Updates are incremental. Retain only nodes reachable from the root.
        let mut reachable = BTreeSet::new();
        let mut pending: Vec<_> = self.root.into_iter().collect();
        while let Some(id) = pending.pop() {
            if reachable.insert(id)
                && let Some(node) = self.nodes.get(&id)
            {
                pending.extend(node.children().iter().copied());
            }
        }
        self.nodes.retain(|id, _| reachable.contains(id));
    }
}
pub fn accessibility_snapshot(generation: u64, enabled: bool) -> String {
    let Some(window) = bridge::window(generation) else {
        return "[]".into();
    };
    let changed = window.accessibility.borrow().active != enabled;
    if changed {
        let callbacks = {
            let mut state = window.accessibility.borrow_mut();
            state.active = enabled;
            state.callbacks.clone()
        };
        if let Some(callbacks) = callbacks {
            if enabled {
                if let Some(update) = (callbacks.activation)() {
                    window.accessibility.borrow_mut().update(update);
                }
            } else {
                (callbacks.deactivation)();
            }
        }
        bridge::post(0);
    }
    if !enabled {
        return "[]".into();
    }
    let state = window.accessibility.borrow();
    let root = state.root;
    let nodes: Vec<_> = state
        .nodes
        .iter()
        .map(|(id, node)| {
            let bounds = node.bounds().map(|b| vec![b.x0, b.y0, b.x1, b.y1]);
            let class = match node.role() {
                Role::Button => "android.widget.Button",
                Role::CheckBox => "android.widget.CheckBox",
                Role::Switch => "android.widget.Switch",
                Role::TextInput | Role::MultilineTextInput | Role::SearchInput => {
                    "android.widget.EditText"
                }
                Role::Label => "android.widget.TextView",
                Role::List => "android.widget.ListView",
                _ => "android.view.View",
            };
            let parent = state
                .nodes
                .iter()
                .find(|(_, candidate)| candidate.children().contains(id))
                .map(|(id, _)| id.0.to_string());
            json!({ "id": id.0.to_string(), "root": Some(*id) == root, "parent": parent,
            "children": node.children().iter().map(|child| child.0.to_string()).collect::<Vec<_>>(),
            "class": class, "label": node.label(), "value": node.value(),
            "authorId": node.author_id(), "bounds": bounds,
            "focused": state.focus == Some(*id), "enabled": !node.is_disabled(),
            "focusable": node.supports_action(Action::Focus),
            "checked": node.toggled().map(|value| value == accesskit::Toggled::True),
            "clickable": node.supports_action(Action::Click),
            "editable": node.supports_action(Action::SetValue) })
        })
        .collect();
    serde_json::to_string(&nodes).unwrap_or_else(|_| "[]".into())
}
pub fn accessibility_action(generation: u64, id: u64, operation: i32, value: &str) -> bool {
    let Some(window) = bridge::window(generation) else {
        return false;
    };
    let state = window.accessibility.borrow();
    if !state.active {
        return false;
    }
    let Some(node) = state.nodes.get(&NodeId(id)) else {
        return false;
    };
    if node.is_disabled() {
        return false;
    }
    let action = match operation {
        0 => Action::Click,
        1 => Action::Focus,
        2 => Action::SetValue,
        _ => return false,
    };
    if !node.supports_action(action) {
        return false;
    }
    let Some(callbacks) = state.callbacks.clone() else {
        return false;
    };
    drop(state);
    (callbacks.action)(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: NodeId(id),
        data: (action == Action::SetValue).then(|| ActionData::Value(value.into())),
    });
    bridge::post(0);
    true
}
