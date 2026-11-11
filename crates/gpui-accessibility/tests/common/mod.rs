//! Shared deterministic fixtures for accessibility integration tests.

#![allow(dead_code)]

use accesskit::{Node, NodeId, Rect, Role, Tree, TreeId, TreeUpdate};
use gpui_accessibility::{
    DisplayedScene, SceneRevision, SemanticBounds, SemanticIdentity, SemanticNodeSnapshot,
    SemanticRole, SemanticTreeSnapshot,
};
use gpui_system::{OperationContext, SurfaceId, UiOwner};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

pub const SURFACE: SurfaceId = SurfaceId(7);

pub fn scene(revision: u64) -> DisplayedScene {
    DisplayedScene::new(SURFACE, 1, SceneRevision(revision))
}

pub fn scene_generation(generation: u64, revision: u64) -> DisplayedScene {
    DisplayedScene::new(SURFACE, generation, SceneRevision(revision))
}

pub fn bounds(x: f64, y: f64, width: f64, height: f64) -> SemanticBounds {
    SemanticBounds::from_origin_size(x, y, width, height).expect("finite bounds")
}

pub fn base_node(id: u64, role: Role) -> Node {
    let mut node = Node::new(role);
    node.set_label(format!("node-{id}"));
    node.set_bounds(Rect::new(0.0, 0.0, 100.0, 40.0));
    node
}

pub fn update(root: u64, focus: u64, nodes: Vec<(u64, Node)>) -> TreeUpdate {
    TreeUpdate {
        nodes: nodes
            .into_iter()
            .map(|(id, node)| (NodeId(id), node))
            .collect(),
        tree: Some(Tree::new(NodeId(root))),
        tree_id: TreeId::ROOT,
        focus: NodeId(focus),
    }
}

pub fn incremental(focus: u64, nodes: Vec<(u64, Node)>) -> TreeUpdate {
    TreeUpdate {
        nodes: nodes
            .into_iter()
            .map(|(id, node)| (NodeId(id), node))
            .collect(),
        tree: None,
        tree_id: TreeId::ROOT,
        focus: NodeId(focus),
    }
}

pub fn author(node: &mut Node, author_id: &str) {
    node.set_author_id(author_id);
}

pub fn semantic_node(id: u64, author_id: Option<&str>, role: SemanticRole) -> SemanticNodeSnapshot {
    SemanticNodeSnapshot {
        identity: SemanticIdentity::new(id, author_id.map(str::to_owned)).expect("identity"),
        role,
        name: Some(format!("semantic-{id}")),
        description: None,
        value: None,
        placeholder: None,
        states: Default::default(),
        relationships: Default::default(),
        bounds: Some(bounds(0.0, 0.0, 100.0, 40.0)),
        actions: Vec::new(),
        index_in_set: None,
        size_of_set: None,
    }
}

pub fn snapshot(
    displayed: DisplayedScene,
    root: u64,
    focus: u64,
    nodes: Vec<SemanticNodeSnapshot>,
) -> SemanticTreeSnapshot {
    let root_identity = nodes
        .iter()
        .find(|node| node.identity.node_id == root)
        .expect("root node")
        .identity
        .clone();
    let focus_identity = nodes
        .iter()
        .find(|node| node.identity.node_id == focus)
        .expect("focus node")
        .identity
        .clone();
    SemanticTreeSnapshot {
        surface: displayed.surface,
        generation: displayed.generation,
        revision: displayed.revision,
        root: root_identity,
        focus: Some(focus_identity),
        nodes,
    }
}

pub fn link(nodes: &mut [SemanticNodeSnapshot], parent: u64, child: u64) {
    let parent_index = nodes
        .iter()
        .position(|node| node.identity.node_id == parent)
        .expect("parent");
    let child_index = nodes
        .iter()
        .position(|node| node.identity.node_id == child)
        .expect("child");
    let child_identity = nodes[child_index].identity.clone();
    let parent_identity = nodes[parent_index].identity.clone();
    nodes[parent_index]
        .relationships
        .children
        .push(child_identity);
    nodes[child_index].relationships.parent = Some(parent_identity);
}

pub fn ui_context(owner: &UiOwner) -> OperationContext {
    OperationContext {
        ui: Some(owner.context(None).expect("current UI context")),
        ..OperationContext::default()
    }
}

pub fn poll_once<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

pub fn completed<T>(result: gpui_system::ServiceResult<T>) -> T {
    match result {
        Ok(gpui_system::Completion::Completed(value)) => value,
        Ok(gpui_system::Completion::Cancelled(reason)) => {
            panic!("operation was cancelled: {reason:?}")
        }
        Err(error) => panic!("operation failed: {error:?}"),
    }
}
