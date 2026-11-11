//! Incremental scene tracking, generation safety and identity retention.
//!
//! A tracker owns the last committed semantic tree per surface. AccessKit
//! `TreeUpdate`s are applied incrementally, unreachable nodes are pruned after
//! every revision, and stale generation/revision input is rejected before it
//! can replace a newer scene. The same validation path accepts complete
//! semantic snapshots from hosts that do not use AccessKit directly.

use crate::error::SemanticError;
use crate::scene::{
    DisplayedScene, IdentityKey, SceneRevision, SemanticIdentity, SemanticTreeSnapshot,
};
use accesskit::{Node, NodeId, TreeId, TreeUpdate};
use gpui_system::SurfaceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The default maximum number of simultaneously tracked surfaces.
pub const DEFAULT_MAX_SURFACES: usize = 8;
/// The default maximum number of nodes retained for one surface.
pub const DEFAULT_MAX_NODES: usize = 100_000;

/// A node moved to a different render-local ID while keeping its stable author
/// identity across revisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRelocation {
    pub previous: SemanticIdentity,
    pub current: SemanticIdentity,
}

/// The same render-local node ID was reused for a different stable identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityReplacement {
    pub node_id: u64,
    pub previous: SemanticIdentity,
    pub current: SemanticIdentity,
}

/// Retention comparison between two accepted revisions. `churned` and
/// `replaced` are the audit failures for keyed controls; `relocated` is benign
/// when the author ID is retained.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityRetentionAudit {
    pub revision: SceneRevision,
    pub retained: Vec<SemanticIdentity>,
    pub relocated: Vec<IdentityRelocation>,
    pub added: Vec<SemanticIdentity>,
    pub churned: Vec<SemanticIdentity>,
    pub replaced: Vec<IdentityReplacement>,
}

impl IdentityRetentionAudit {
    pub fn stable(&self) -> bool {
        self.churned.is_empty() && self.replaced.is_empty()
    }

    pub fn retained_count(&self) -> usize {
        self.retained.len()
    }

    fn between(
        previous: &BTreeMap<IdentityKey, SemanticIdentity>,
        current: &SemanticTreeSnapshot,
    ) -> Self {
        let mut current_by_key = BTreeMap::new();
        let mut current_by_node = BTreeMap::new();
        for node in &current.nodes {
            current_by_key.insert(node.identity.key(), node.identity.clone());
            current_by_node.insert(node.identity.node_id, node.identity.key());
        }
        let mut previous_by_key = BTreeMap::new();
        let mut previous_by_node = BTreeMap::new();
        for (key, identity) in previous {
            previous_by_key.insert(key.clone(), identity.clone());
            previous_by_node.insert(identity.node_id, key.clone());
        }

        let mut audit = Self {
            revision: current.revision,
            ..Self::default()
        };
        for (key, identity) in &current_by_key {
            if let Some(previous_identity) = previous_by_key.get(key) {
                audit.retained.push(identity.clone());
                if previous_identity.node_id != identity.node_id {
                    audit.relocated.push(IdentityRelocation {
                        previous: previous_identity.clone(),
                        current: identity.clone(),
                    });
                }
            } else if let Some(previous_key) = previous_by_node.get(&identity.node_id)
                && let Some(previous_identity) = previous_by_key.get(previous_key)
            {
                audit.replaced.push(IdentityReplacement {
                    node_id: identity.node_id,
                    previous: previous_identity.clone(),
                    current: identity.clone(),
                });
            } else {
                audit.added.push(identity.clone());
            }
        }
        for (key, identity) in &previous_by_key {
            if current_by_key.contains_key(key) {
                continue;
            }
            if current_by_node.contains_key(&identity.node_id) {
                continue;
            }
            audit.churned.push(identity.clone());
        }
        audit
    }
}

#[derive(Debug, Clone)]
struct TrackedSurface {
    active_generation: u64,
    last_revision: Option<SceneRevision>,
    root: Option<NodeId>,
    focus: Option<NodeId>,
    nodes: BTreeMap<NodeId, Node>,
    snapshot: Option<SemanticTreeSnapshot>,
    previous_identities: BTreeMap<IdentityKey, SemanticIdentity>,
    identity_audit: Option<IdentityRetentionAudit>,
}

impl TrackedSurface {
    fn new(generation: u64) -> Self {
        Self {
            active_generation: generation,
            last_revision: None,
            root: None,
            focus: None,
            nodes: BTreeMap::new(),
            snapshot: None,
            previous_identities: BTreeMap::new(),
            identity_audit: None,
        }
    }

    fn reset(&mut self, generation: u64) {
        *self = Self::new(generation);
    }

    fn finish_revision(&mut self, scene: DisplayedScene, snapshot: SemanticTreeSnapshot) {
        let audit = IdentityRetentionAudit::between(&self.previous_identities, &snapshot);
        self.previous_identities = snapshot
            .nodes
            .iter()
            .map(|node| (node.identity.key(), node.identity.clone()))
            .collect();
        self.identity_audit = Some(audit);
        self.last_revision = Some(scene.revision);
        self.snapshot = Some(snapshot);
    }
}

/// Per-surface semantic scene tracker.
#[derive(Debug, Clone)]
pub struct SemanticSceneTracker {
    max_surfaces: usize,
    max_nodes: usize,
    surfaces: BTreeMap<u64, TrackedSurface>,
    disposed: BTreeMap<u64, u64>,
}

impl Default for SemanticSceneTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticSceneTracker {
    pub fn new() -> Self {
        Self::with_limits(DEFAULT_MAX_SURFACES, DEFAULT_MAX_NODES)
    }

    pub fn with_limits(max_surfaces: usize, max_nodes: usize) -> Self {
        Self {
            max_surfaces: max_surfaces.max(1),
            max_nodes: max_nodes.max(1),
            surfaces: BTreeMap::new(),
            disposed: BTreeMap::new(),
        }
    }

    pub fn surface_count(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_disposed(&self, surface: SurfaceId) -> bool {
        self.disposed.contains_key(&surface.0)
    }

    pub fn active_generation(&self, surface: SurfaceId) -> Option<u64> {
        self.surfaces
            .get(&surface.0)
            .map(|state| state.active_generation)
    }

    pub fn last_revision(&self, surface: SurfaceId) -> Option<SceneRevision> {
        self.surfaces
            .get(&surface.0)
            .and_then(|state| state.snapshot.as_ref().map(|snapshot| snapshot.revision))
    }

    pub fn snapshot(&self, surface: SurfaceId) -> Option<&SemanticTreeSnapshot> {
        self.surfaces
            .get(&surface.0)
            .and_then(|state| state.snapshot.as_ref())
    }

    pub fn identity_audit(&self, surface: SurfaceId) -> Option<&IdentityRetentionAudit> {
        self.surfaces
            .get(&surface.0)
            .and_then(|state| state.identity_audit.as_ref())
    }

    /// Verify that a scene is exactly the currently committed revision.
    pub fn validate_scene(&self, scene: DisplayedScene) -> Result<(), SemanticError> {
        if let Some(disposed) = self.disposed.get(&scene.surface.0)
            && scene.generation <= *disposed
        {
            return Err(SemanticError::SurfaceDisposed {
                surface: scene.surface,
            });
        }
        let state =
            self.surfaces
                .get(&scene.surface.0)
                .ok_or(SemanticError::SurfaceNotTracked {
                    surface: scene.surface,
                })?;
        if scene.generation < state.active_generation {
            return Err(SemanticError::StaleGeneration {
                displayed: scene.generation,
                active: state.active_generation,
            });
        }
        if scene.generation > state.active_generation {
            return Err(SemanticError::SurfaceNotTracked {
                surface: scene.surface,
            });
        }
        let current =
            self.last_revision(scene.surface)
                .ok_or(SemanticError::SurfaceNotTracked {
                    surface: scene.surface,
                })?;
        if scene.revision != current {
            return Err(SemanticError::StaleRevision {
                displayed: scene.revision,
                current,
            });
        }
        Ok(())
    }

    /// Apply an AccessKit update. A newer generation clears all previous state
    /// and requires a full tree (`update.tree`). Equal revisions are stale.
    pub fn ingest_accesskit(
        &mut self,
        scene: DisplayedScene,
        update: TreeUpdate,
    ) -> Result<SemanticTreeSnapshot, SemanticError> {
        if update.tree_id != TreeId::ROOT {
            return Err(SemanticError::UnsupportedTree);
        }
        let mut state = self.prepare_surface(scene)?;
        if let Some(tree) = update.tree {
            state.root = Some(tree.root);
        }
        let root = state.root.ok_or(SemanticError::MissingRoot)?;
        for (node_id, node) in update.nodes {
            state.nodes.insert(node_id, node);
        }
        state.focus = Some(update.focus);
        prune_unreachable(&mut state.nodes, root)?;
        if state.nodes.len() > self.max_nodes {
            return Err(SemanticError::ResourceLimitExceeded {
                limit: self.max_nodes,
            });
        }
        let snapshot = SemanticTreeSnapshot::from_accesskit_nodes(
            scene,
            root,
            update.focus,
            &state.nodes,
            Some(self.max_nodes),
        )?;
        state.finish_revision(scene, snapshot.clone());
        self.surfaces.insert(scene.surface.0, state);
        Ok(snapshot)
    }

    /// Commit a complete semantic snapshot. Hosts that build semantic scenes
    /// without AccessKit use this path; incremental AccessKit updates can be
    /// applied afterwards because the snapshot is converted back to nodes.
    pub fn commit(
        &mut self,
        snapshot: SemanticTreeSnapshot,
    ) -> Result<SemanticTreeSnapshot, SemanticError> {
        snapshot.validate()?;
        if snapshot.nodes.len() > self.max_nodes {
            return Err(SemanticError::ResourceLimitExceeded {
                limit: self.max_nodes,
            });
        }
        let scene = snapshot.displayed_scene();
        let mut state = self.prepare_surface(scene)?;
        state.root = Some(snapshot.root.node());
        state.focus = snapshot.focus.as_ref().map(SemanticIdentity::node);
        state.nodes = snapshot.to_accesskit_nodes();
        state.finish_revision(scene, snapshot.clone());
        self.surfaces.insert(scene.surface.0, state);
        Ok(snapshot)
    }

    /// Explicitly advance a tracked surface to a newer generation. The caller
    /// must commit a full scene revision before any observable operation.
    pub fn advance_generation(
        &mut self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SemanticError> {
        if let Some(disposed) = self.disposed.get(&surface.0)
            && generation <= *disposed
        {
            return Err(SemanticError::SurfaceDisposed { surface });
        }
        let state = self
            .surfaces
            .get_mut(&surface.0)
            .ok_or(SemanticError::SurfaceNotTracked { surface })?;
        if generation <= state.active_generation {
            return Err(SemanticError::StaleGeneration {
                displayed: generation,
                active: state.active_generation,
            });
        }
        state.reset(generation);
        Ok(())
    }

    /// Dispose a surface and reject any scene from the disposed generation or
    /// older. A later generation may be initialized as a new native surface.
    pub fn dispose_surface(&mut self, surface: SurfaceId) {
        let generation = self
            .surfaces
            .remove(&surface.0)
            .map_or(0, |state| state.active_generation);
        self.disposed.insert(surface.0, generation);
    }

    fn prepare_surface(&self, scene: DisplayedScene) -> Result<TrackedSurface, SemanticError> {
        if let Some(disposed) = self.disposed.get(&scene.surface.0)
            && scene.generation <= *disposed
        {
            return Err(SemanticError::SurfaceDisposed {
                surface: scene.surface,
            });
        }
        let mut state = match self.surfaces.get(&scene.surface.0) {
            Some(state) => state.clone(),
            None => {
                if self.surfaces.len() >= self.max_surfaces {
                    return Err(SemanticError::ResourceLimitExceeded {
                        limit: self.max_surfaces,
                    });
                }
                TrackedSurface::new(scene.generation)
            }
        };
        if scene.generation < state.active_generation {
            return Err(SemanticError::StaleGeneration {
                displayed: scene.generation,
                active: state.active_generation,
            });
        }
        if scene.generation > state.active_generation {
            state.reset(scene.generation);
        } else if let Some(current) = state.last_revision
            && scene.revision <= current
        {
            return Err(SemanticError::StaleRevision {
                displayed: scene.revision,
                current,
            });
        }
        Ok(state)
    }
}

fn prune_unreachable(
    nodes: &mut BTreeMap<NodeId, Node>,
    root: NodeId,
) -> Result<(), SemanticError> {
    if !nodes.contains_key(&root) {
        return Err(SemanticError::RootNotInTree { node: root.0 });
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![(root, None::<NodeId>)];
    while let Some((node_id, parent)) = pending.pop() {
        if !reachable.insert(node_id.0) {
            continue;
        }
        let node = nodes
            .get(&node_id)
            .ok_or(SemanticError::MissingRelatedNode {
                node: parent.map_or(node_id.0, |parent| parent.0),
                related: node_id.0,
            })?;
        for child in node.children() {
            pending.push((*child, Some(node_id)));
        }
    }
    nodes.retain(|node_id, _| reachable.contains(&node_id.0));
    Ok(())
}
