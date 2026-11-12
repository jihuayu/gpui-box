//! Staged construction, topology and properties with one publication boundary.
use super::*;
use std::collections::BTreeSet;

pub struct TreeEditCommit<R> {
    pub value: R,
    pub commit: Commit,
    /// Successfully published new handles. Created-then-removed output is absent.
    pub inserted: Vec<NodeId>,
    pub removed: Vec<NodeId>,
}

/// An owner-local draft. No layout, live node, frame or binding changes until
/// the closure and complete final-state validation succeed. Failed reservations
/// consume generation numbers so leaked draft handles never become valid later.
pub struct TreeEdit<'a> {
    tree: &'a mut UiTree,
    nodes: BTreeMap<NodeId, Node>,
    created: BTreeSet<NodeId>,
    removed: Vec<NodeId>,
    properties: Transaction,
    free: Option<Vec<usize>>,
    slots: usize,
}
impl TreeEdit<'_> {
    pub fn root(&self) -> NodeId {
        self.tree.root
    }
    fn node(&self, id: NodeId) -> Result<&Node, Error> {
        self.nodes
            .get(&id)
            .map(Ok)
            .unwrap_or_else(|| self.tree.node(id))
    }
    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node, Error> {
        if !self.nodes.contains_key(&id) {
            self.nodes.insert(id, self.tree.node(id)?.clone());
        }
        Ok(self.nodes.get_mut(&id).unwrap())
    }
    pub fn children(&self, id: NodeId) -> Result<&[NodeId], Error> {
        Ok(&self.node(id)?.children)
    }
    pub fn append(&mut self, parent: NodeId, style: Style, color: u32) -> Result<NodeId, Error> {
        self.settle_topology()?;
        self.node(parent)?;
        let generation = self.tree.issue_node_generation()?;
        let slot = self
            .free
            .get_or_insert_with(|| self.tree.free.clone())
            .pop()
            .unwrap_or_else(|| {
                let slot = self.slots;
                self.slots += 1;
                slot
            });
        let id = NodeId {
            tree: self.tree.identity,
            slot,
            generation,
        };
        self.nodes.insert(
            id,
            Node {
                layout: self.tree.node(self.tree.root)?.layout, // replaced only at publication
                published_layout: taffy::Layout::new(),
                parent: Some(parent),
                children: vec![],
                style,
                color,
                translation: [0.0; 2],
                chunk: None,
                clip: None,
                scroll: [0.0; 2],
                opacity: 1.0,
                measure: None,
                decoration: QuadDecoration::default(),
                shadows: Arc::from([]),
                image: None,
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                text: None,
            },
        );
        self.created.insert(id);
        self.node_mut(parent)?.children.push(id);
        Ok(id)
    }
    /// Replace complete child lists. Parent links are resolved from the final
    /// graph during validation; graph rotations can be staged in any order.
    pub fn set_children(&mut self, parent: NodeId, children: Vec<NodeId>) -> Result<(), Error> {
        self.settle_topology()?;
        self.replace_children(parent, children)
    }
    fn replace_children(&mut self, parent: NodeId, children: Vec<NodeId>) -> Result<(), Error> {
        for id in &children {
            self.node(*id)?;
        }
        self.node_mut(parent)?.children = children;
        Ok(())
    }
    pub fn reparent(&mut self, node: NodeId, host: NodeId) -> Result<(), Error> {
        self.settle_topology()?;
        self.node(host)?;
        self.node(node)?;
        if node == self.tree.root {
            return Err(Error::CannotRemoveRoot);
        }
        // Complete child-list writes need not have updated cached parent links.
        // Resolve all current draft hosts, including temporarily duplicated edges.
        let parents: Vec<_> = self
            .live_ids()
            .into_iter()
            .filter(|id| self.node(*id).unwrap().children.contains(&node))
            .collect();
        if parents == [host] {
            return Ok(());
        }
        for old in parents {
            self.node_mut(old)?.children.retain(|id| *id != node);
        }
        self.node_mut(host)?.children.push(node);
        self.node_mut(node)?.parent = Some(host);
        Ok(())
    }
    /// Remove the final draft subtree (including new children). Reparent a
    /// surviving child out before removal when evacuating a host in this edit.
    pub fn remove(&mut self, node: NodeId) -> Result<(), Error> {
        self.node(node)?;
        if node == self.tree.root {
            return Err(Error::CannotRemoveRoot);
        }
        self.removed.push(node);
        Ok(())
    }
    pub fn properties(&mut self) -> &mut Transaction {
        &mut self.properties
    }
    fn live_ids(&self) -> Vec<NodeId> {
        self.tree
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                entry.node.as_ref().map(|_| NodeId {
                    tree: self.tree.identity,
                    slot,
                    generation: entry.generation,
                })
            })
            .chain(self.created.iter().copied())
            .collect()
    }
    fn settle_topology(&mut self) -> Result<(), Error> {
        if !self.properties.topology_pending {
            return Ok(());
        }
        let properties = std::mem::take(&mut self.properties);
        for patch in properties.patches {
            if let Patch::Reorder(parent, children) = patch {
                self.replace_children(parent, children)?;
            } else {
                self.properties.patches.push(patch);
            }
        }
        Ok(())
    }
    pub(super) fn removed_ids(&mut self) -> Result<BTreeSet<NodeId>, Error> {
        self.settle_topology()?;
        let mut dead = BTreeSet::new();
        let mut stack = self.removed.clone();
        while let Some(id) = stack.pop() {
            if dead.insert(id) {
                stack.extend(&self.node(id)?.children);
            }
        }
        Ok(dead)
    }
}
impl UiTree {
    pub fn edit<R>(
        &mut self,
        capabilities: BackendCapabilities,
        update: impl FnOnce(&mut TreeEdit<'_>) -> Result<R, Error>,
    ) -> Result<TreeEditCommit<R>, Error> {
        let slots = self.slots.len();
        let mut draft = TreeEdit {
            tree: self,
            nodes: BTreeMap::new(),
            created: BTreeSet::new(),
            removed: vec![],
            properties: Transaction::default(),
            free: None,
            slots,
        };
        let value = update(&mut draft)?;
        draft.publish(value, capabilities)
    }
}
impl TreeEdit<'_> {
    fn publish<R>(
        mut self,
        value: R,
        capabilities: BackendCapabilities,
    ) -> Result<TreeEditCommit<R>, Error> {
        // Reorder participates in final topology rather than a second apply.
        self.settle_topology()?;
        if self.nodes.is_empty() && self.removed.is_empty() {
            // Preserve the existing property-only complexity and route selection.
            // In particular an equal source event must not scan the render tree.
            let commit = self.tree.commit(self.properties, capabilities)?;
            return Ok(TreeEditCommit {
                value,
                commit,
                inserted: vec![],
                removed: vec![],
            });
        }
        let ids = self.live_ids();
        let mut parents = vec![None; self.slots];
        for id in &ids {
            for child in &self.node(*id)?.children {
                self.node(*child)?;
                if *child == self.tree.root || parents[child.slot].replace(*id).is_some() {
                    return Err(Error::InvalidChildren);
                }
            }
        }
        for child in &ids {
            if let Some(parent) = parents[child.slot]
                && self.node(*child)?.parent != Some(parent)
            {
                self.node_mut(*child)?.parent = Some(parent);
            }
        }
        let dead = self.removed_ids()?;
        // Project surviving children solely while preparing property inputs.
        let mut restore = vec![];
        for id in &ids {
            if dead.contains(id) {
                continue;
            }
            if self
                .node(*id)?
                .children
                .iter()
                .any(|child| dead.contains(child))
            {
                restore.push((*id, self.node(*id)?.children.clone()));
                self.node_mut(*id)?
                    .children
                    .retain(|child| !dead.contains(child));
            }
        }
        for patch in &self.properties.patches {
            if dead.contains(&patch.node()) {
                return Err(Error::InvalidNode);
            }
        }
        let properties = std::mem::take(&mut self.properties);
        let changes = UiTree::prepare_properties(properties, |id| self.node(id))?;
        for (id, children) in restore {
            self.node_mut(id)?.children = children;
        }
        for (id, entry, _, _, _, _, _, _, _, _) in &changes {
            if let Some(measure) = &entry.measure {
                self.node_mut(*id)?.measure = measure.clone();
            }
        }
        let tx = StructureTransaction {
            removed: self.removed.clone(),
            ..Default::default()
        };
        let mut topology = UiTree::prepare_topology(
            tx,
            |id| self.node(id),
            ids.iter().copied(),
            self.slots,
            self.tree.root,
            // This is validation only. Decide revision exhaustion after excluding
            // cancelled reservations and comparing the final committed graph.
            0,
            true,
        )?;
        let removed: BTreeSet<_> = topology.removed.iter().copied().collect();
        let inserted: Vec<_> = self
            .created
            .iter()
            .filter(|id| !removed.contains(id))
            .copied()
            .collect();
        // The final graph getter is the draft itself, so compute actual live
        // topology differences against the committed owner separately.
        topology.changed.clear();
        for id in &ids {
            if removed.contains(id) {
                continue;
            }
            let children: Vec<_> = self
                .node(*id)?
                .children
                .iter()
                .filter(|child| !removed.contains(child))
                .copied()
                .collect();
            if self.created.contains(id) || self.tree.node(*id)?.children != children {
                topology.changed.push((*id, children));
            }
        }
        let topology_changed =
            !topology.changed.is_empty() || removed.iter().any(|id| !self.created.contains(id));
        let changed = topology_changed || !changes.is_empty();
        topology.next = if changed {
            Some(
                self.tree
                    .revision
                    .checked_add(1)
                    .ok_or(Error::RevisionExhausted)?,
            )
        } else {
            None
        };
        // No fallible user validation follows this boundary. Allocating Taffy
        // leaves for valid styles is an owning invariant, with stable old IDs.
        for id in &inserted {
            let mut node = self.nodes.remove(id).unwrap();
            node.layout = self
                .tree
                .layout
                .new_leaf(node.style.clone())
                .expect("draft Taffy leaf");
            node.children.clear();
            self.tree.layout_nodes.insert(node.layout, *id);
            if id.slot >= self.tree.slots.len() {
                for slot in self.tree.slots.len()..id.slot {
                    self.tree.free.push(slot);
                }
                self.tree.slots.resize_with(id.slot + 1, || Slot {
                    generation: 0,
                    node: None,
                });
            }
            self.tree.free.retain(|slot| *slot != id.slot);
            self.tree.slots[id.slot] = Slot {
                generation: id.generation,
                node: Some(node),
            };
        }
        // A newly allocated leaf already has its final parent link. Existing
        // moved nodes get parentage from the same final child-list application.
        topology.removed.retain(|id| !self.created.contains(id));
        let mut commit = if topology_changed {
            self.tree.apply_structure(topology)
        } else {
            if changed {
                self.tree.advance()?;
            }
            Commit {
                revision: self.tree.revision,
                ..Default::default()
            }
        };
        if !changes.is_empty() {
            let properties = self.tree.apply_properties(changes, capabilities);
            commit.request_frame |= properties.request_frame;
            commit.routes.extend(properties.routes);
            commit.decisions.extend(properties.decisions);
        }
        let unique: BTreeSet<_> = commit
            .routes
            .iter()
            .map(|(id, _)| *id)
            .chain(inserted.iter().copied())
            .collect();
        commit.changed_nodes = unique.len();
        Ok(TreeEditCommit {
            value,
            commit,
            inserted,
            removed: removed
                .into_iter()
                .filter(|id| !self.created.contains(id))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests;
