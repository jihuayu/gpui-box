//! Final-topology transactions over existing persistent render identities.
use super::*;

/// Replace complete child lists for every affected parent. A reparent supplies
/// both its old and new host lists. Validation considers the final tree, not
/// transient intermediate parentage. No node is constructed or disposed here.
#[derive(Default)]
pub struct StructureTransaction {
    children: BTreeMap<NodeId, Vec<NodeId>>,
}
impl StructureTransaction {
    pub fn children(&mut self, parent: NodeId, children: Vec<NodeId>) {
        self.children.insert(parent, children);
    }
}
impl UiTree {
    /// Validate ownership, uniqueness, root reachability and measured leaves
    /// before changing any topology or Taffy input. One changed batch advances
    /// one revision; unchanged final lists retain the published frame.
    /// Validation currently scans all arena slots and live edges. This is not a
    /// sublinear topology update; no-op batches skip that final-graph scan.
    pub fn commit_structure(&mut self, tx: StructureTransaction) -> Result<Commit, Error> {
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
        if changed.is_empty() {
            return Ok(Commit {
                revision: self.revision,
                ..Default::default()
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
        // Every Taffy ID is live and the final graph is a tree. Detach all changed
        // parents before linking, so valid graph rotations need no transient cycle.
        // Taffy errors here indicate a broken owning invariant, not user input.
        for (parent, _) in &changed {
            self.layout
                .set_children(self.node(*parent)?.layout, &[])
                .expect("validated Taffy parent");
        }
        for (parent, children) in &changed {
            let layouts: Vec<_> = children
                .iter()
                .map(|id| self.node(*id).unwrap().layout)
                .collect();
            self.layout
                .set_children(self.node(*parent)?.layout, &layouts)
                .expect("validated Taffy topology");
        }
        let mut result = Commit {
            revision: next,
            changed_nodes: changed.len(),
            request_frame: true,
            ..Default::default()
        };
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
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
