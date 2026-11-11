//! Final-topology transactions over existing persistent render identities.
use super::*;

/// Replace complete child lists for every affected parent. A reparent supplies
/// both its old and new host lists. Validation considers the final tree, not
/// transient intermediate parentage. Subtree removal can join the same commit;
/// constructing nodes still uses the separate append API.
#[derive(Default)]
pub struct StructureTransaction {
    children: BTreeMap<NodeId, Vec<NodeId>>,
    removed: Vec<NodeId>,
}
impl StructureTransaction {
    /// Remove the current subtree. Overlapping removal roots coalesce. A node
    /// inside a removed subtree cannot be rescued by a reparent in this batch.
    pub fn remove(&mut self, node: NodeId) {
        self.removed.push(node);
    }
    pub fn children(&mut self, parent: NodeId, children: Vec<NodeId>) {
        self.children.insert(parent, children);
    }
}
pub(super) struct PreparedStructure {
    changed: Vec<(NodeId, Vec<NodeId>)>,
    removed: Vec<NodeId>,
    next: Option<u64>,
}
impl PreparedStructure {
    pub(super) fn has_removals(&self) -> bool {
        !self.removed.is_empty()
    }
}
impl UiTree {
    /// Validate ownership, uniqueness, root reachability and measured leaves
    /// before changing any topology or Taffy input. One changed batch advances
    /// one revision; unchanged final lists retain the published frame.
    /// Validation currently scans all arena slots and live edges. This is not a
    /// sublinear topology update; no-op batches skip that final-graph scan.
    pub fn commit_structure(&mut self, tx: StructureTransaction) -> Result<Commit, Error> {
        let prepared = self.prepare_structure(tx)?;
        Ok(self.apply_structure(prepared))
    }
    pub(super) fn prepare_structure(
        &self,
        mut tx: StructureTransaction,
    ) -> Result<PreparedStructure, Error> {
        let mut removed = vec![false; self.slots.len()];
        let mut removal_order = vec![];
        for root in &tx.removed {
            self.node(*root)?;
            if *root == self.root {
                return Err(Error::CannotRemoveRoot);
            }
            let mut stack = vec![*root];
            while let Some(id) = stack.pop() {
                if std::mem::replace(&mut removed[id.slot], true) {
                    continue;
                }
                removal_order.push(id);
                stack.extend(&self.node(id)?.children);
            }
        }
        // Omitted lists retain surviving children. Explicit lists must never
        // refer to a retired generation, including one removed in this batch.
        for (parent, children) in &tx.children {
            self.node(*parent)?;
            if removed[parent.slot] {
                return Err(Error::InvalidChildren);
            }
            for child in children {
                self.node(*child)?;
                if removed[child.slot] {
                    return Err(Error::InvalidChildren);
                }
            }
        }
        for id in &removal_order {
            if let Some(parent) = self.node(*id)?.parent
                && !removed[parent.slot]
            {
                tx.children.entry(parent).or_insert_with(|| {
                    self.node(parent)
                        .unwrap()
                        .children
                        .iter()
                        .filter(|child| !removed[child.slot])
                        .copied()
                        .collect()
                });
            }
        }
        for (parent, children) in &tx.children {
            let node = self.node(*parent)?;
            if !children.is_empty() && node.measure.is_some() {
                return Err(Error::MeasuredNodeHasChildren);
            }
            for child in children {
                self.node(*child)?;
            }
        }
        let changed: Vec<_> = tx
            .children
            .into_iter()
            .filter(|(id, children)| self.node(*id).unwrap().children != *children)
            .collect();
        if changed.is_empty() && removal_order.is_empty() {
            return Ok(PreparedStructure {
                changed,
                removed: removal_order,
                next: None,
            });
        }
        let mut lists = vec![None; self.slots.len()];
        for (id, children) in &changed {
            lists[id.slot] = Some(children);
        }
        let mut parents = vec![false; self.slots.len()];
        let mut parent_count = 0;
        let mut live = 0;
        for (slot, entry) in self.slots.iter().enumerate() {
            let Some(node) = &entry.node else { continue };
            if removed[slot] {
                continue;
            }
            live += 1;
            let id = NodeId {
                tree: self.identity,
                slot,
                generation: entry.generation,
            };
            let children = lists[id.slot].unwrap_or(&node.children);
            for child in children {
                if *child == self.root || std::mem::replace(&mut parents[child.slot], true) {
                    return Err(Error::InvalidChildren);
                }
                parent_count += 1;
            }
        }
        if parent_count + 1 != live {
            return Err(Error::InvalidChildren);
        }
        let mut visited = vec![false; self.slots.len()];
        let mut visited_count = 0;
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut visited[id.slot], true) {
                return Err(Error::InvalidChildren);
            }
            visited_count += 1;
            stack.extend(lists[id.slot].unwrap_or(&self.node(id)?.children));
        }
        if visited_count != live {
            return Err(Error::InvalidChildren);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        Ok(PreparedStructure {
            changed,
            removed: removal_order,
            next: Some(next),
        })
    }
    pub(super) fn apply_structure(&mut self, prepared: PreparedStructure) -> Commit {
        let PreparedStructure {
            changed,
            removed,
            next,
        } = prepared;
        let Some(next) = next else {
            return Commit {
                revision: self.revision,
                ..Default::default()
            };
        };
        // Every Taffy ID is live and the final graph is a tree. Detach all changed
        // parents before linking, so valid graph rotations need no transient cycle.
        // Taffy errors here indicate a broken owning invariant, not user input.
        for (parent, _) in &changed {
            self.layout
                .set_children(self.node(*parent).unwrap().layout, &[])
                .expect("validated Taffy parent");
        }
        for id in removed.iter().rev() {
            let layout = self.node(*id).unwrap().layout;
            self.layout.remove(layout).expect("validated Taffy removal");
            let slot = &mut self.slots[id.slot];
            slot.node = None;
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(id.slot);
            }
        }
        for (parent, children) in &changed {
            let layouts: Vec<_> = children
                .iter()
                .map(|id| self.node(*id).unwrap().layout)
                .collect();
            self.layout
                .set_children(self.node(*parent).unwrap().layout, &layouts)
                .expect("validated Taffy topology");
        }
        let mut result = Commit {
            revision: next,
            changed_nodes: changed.len() + removed.len(),
            request_frame: true,
            ..Default::default()
        };
        for id in removed {
            result.route(
                id,
                UpdatePath::ReconcileStructure,
                RouteReason::RenderTopologyChanged,
            );
        }
        for (parent, children) in changed {
            for child in &children {
                self.node_mut(*child).parent = Some(parent);
            }
            self.node_mut(parent).children = children;
            result.route(
                parent,
                UpdatePath::ReconcileStructure,
                RouteReason::RenderTopologyChanged,
            );
        }
        self.revision = next;
        self.dirty = true;
        self.layout_dirty = true;
        result
    }
}

#[cfg(test)]
mod tests;
