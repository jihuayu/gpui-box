//! Compact private query trees, independent of unrelated live arena capacity.
use super::*;
use crate::util::sys::Map;

impl<NodeContext: Clone> TaffyTree<NodeContext> {
    /// Copy the union of the selected subtrees for isolated direct-child
    /// measurement. Returned roots correspond to the input order, including
    /// duplicates. Node IDs are private to the returned tree and are remapped.
    /// Styles/delegates/parent-data snapshots share immutable ownership; only
    /// included NodeContexts are cloned. Contexts must not embed live node IDs
    /// that a callback expects to resolve in this new tree.
    ///
    /// Layout/cache/OOF/observation records start empty: cached output can name
    /// old nodes and final placements do not exist in the new tree. Parent links
    /// outside the copied union are cut. This is not a published geometry clone
    /// or a root-layout replacement: supply the explicit original child inputs.
    /// No mutation reaches the source tree, including on error or unwind.
    pub fn clone_subtrees_for_measurement(
        &self,
        roots: &[NodeId],
    ) -> TaffyResult<(Self, Vec<NodeId>)> {
        let mut query = Self::with_capacity(roots.len());
        query.config = self.config;
        let mut mapped: Map<u64, NodeId> = Map::default();
        let mut ordered = Vec::new();
        let mut pending: Vec<_> = roots.iter().copied().rev().collect();
        while let Some(old) = pending.pop() {
            if mapped.contains_key(&u64::from(old)) {
                continue;
            }
            let key = old.into();
            let data = self
                .nodes
                .get(key)
                .ok_or(TaffyError::InvalidInputNode(old))?;
            let new = query.new_leaf_shared(data.style.clone())?;
            let new_key = new.into();
            if let Some(context) = self.node_context_data.get(key) {
                query.node_context_data.insert(new_key, context.clone());
                query.nodes[new_key].has_context = true;
            }
            if let Some(delegate) = self.custom_layouts.get(key) {
                query.custom_layouts.insert(new_key, delegate.clone());
            }
            if let Some(value) = self.parent_data.get(key) {
                query.parent_data.insert(new_key, value.clone());
            }
            if self.exact_measure_nodes.contains_key(key) {
                query.exact_measure_nodes.insert(new_key, ());
            }
            if self.tracked_layout_nodes.contains_key(key) {
                query.tracked_layout_nodes.insert(new_key, ());
            }
            mapped.insert(u64::from(old), new);
            ordered.push(old);
            pending.extend(self.children[key].iter().copied().rev());
        }
        for old in ordered {
            let new = mapped[&u64::from(old)];
            let mut children = new_vec_with_capacity(self.children[old.into()].len());
            for child in &self.children[old.into()] {
                let new_child = mapped[&u64::from(*child)];
                children.push(new_child);
                query.parents[new_child.into()] = Some(new);
            }
            query.children[new.into()] = children;
        }
        let roots = roots.iter().map(|node| mapped[&u64::from(*node)]).collect();
        Ok((query, roots))
    }
}

#[cfg(all(test, feature = "flexbox"))]
#[path = "query_subtrees/tests.rs"]
mod tests;
