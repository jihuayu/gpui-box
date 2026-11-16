//! Actual Taffy output changes drive selective geometry publication.
use super::*;

impl UiTree {
    /// Taffy positions out-of-flow boxes relative to their containing block.
    /// The render/property hierarchy needs parent-local placement instead.
    /// Only out-of-flow placement walks layout ancestry, once per layout pass.
    /// Transforms and scroll remain visual properties of the render hierarchy.
    fn parent_layout_origin(&self, id: NodeId) -> Result<[f32; 2], Error> {
        let node = self.node(id)?;
        let layout = self.layout.layout(node.layout)?;
        if !node.style.position.is_out_of_flow() || node.parent.is_none() {
            return Ok([layout.location.x, layout.location.y]);
        }
        let world = self.layout_world_origin(id)?;
        let parent = self.layout_world_origin(node.parent.expect("nonroot"))?;
        Ok([world[0] - parent[0], world[1] - parent[1]])
    }

    fn layout_world_origin(&self, id: NodeId) -> Result<[f32; 2], Error> {
        use taffy::CoreStyle;
        // Iterative even for deep trees; sum root-to-leaf for the same f32
        // operation order as extraction, skipping non-containing ancestors.
        let mut path = vec![];
        let mut next = Some(id);
        while let Some(id) = next {
            let node = self.node(id)?;
            path.push(self.layout.layout(node.layout)?.location);
            next = node.parent;
            if node.style.position.is_out_of_flow() {
                while let Some(candidate) = next {
                    let ancestor = self.node(candidate)?;
                    if ancestor.parent.is_none()
                        || ancestor
                            .style
                            .is_containing_block()
                            .for_position(node.style.position)
                    {
                        break;
                    }
                    next = ancestor.parent;
                }
            }
        }
        Ok(path.into_iter().rev().fold([0.0; 2], |origin, location| {
            [origin[0] + location.x, origin[1] + location.y]
        }))
    }

    /// Runs a framework-owned measure/layout query without publishing a paint
    /// frame. This is the bridge for intrinsic Flex sizing; the resulting
    /// value is parent-local and does not update `published_layout`.
    pub(super) fn measure_root_with_constraints(
        &mut self,
        constraints: LayoutConstraints,
    ) -> Result<LayoutResult, Error> {
        self.measure_node_with_constraints_impl(self.root, constraints)
    }

    pub(super) fn measure_node_with_constraints_impl(
        &mut self,
        id: NodeId,
        constraints: LayoutConstraints,
    ) -> Result<LayoutResult, Error> {
        constraints.validate()?;
        let available_space = Size {
            width: constraints.width.to_taffy()?,
            height: constraints.height.to_taffy()?,
        };
        let root = self.node(id)?.layout;
        // Taffy stores both computed layouts and cache state in the tree. A
        // measure-only query must not mutate the committed layout/cache used by
        // the next paint frame, so run it against an owned clone.
        let mut layout = self.layout.clone();
        let mut stats = LayoutStats::default();
        layout.compute_layout_with_measure_and_observer(
            root,
            available_space,
            |inputs, _, snapshot, style| {
                measure::layout_leaf(inputs, snapshot.as_deref(), style, &mut stats)
            },
            |_, _, _| {},
        )?;
        let result = layout.unrounded_layout(root);
        Ok(LayoutResult::from_taffy(result))
    }

    pub(super) fn compute_layout(
        &mut self,
        constraints: LayoutConstraints,
    ) -> Result<LayoutStats, Error> {
        let root = self.node(self.root)?.layout;
        let mut stats = LayoutStats::default();
        let mut layout_writes = 0;
        let mut changed_layout_writes = 0;
        let nodes = &self.layout_nodes;
        let written = &mut self.layout_written;
        self.layout.compute_layout_with_measure_and_observer(
            root,
            Size {
                width: constraints.width.to_taffy()?,
                height: constraints.height.to_taffy()?,
            },
            |inputs, _, snapshot, style| {
                measure::layout_leaf(inputs, snapshot.as_deref(), style, &mut stats)
            },
            |id, previous, current| {
                stats_layout_write(
                    &mut layout_writes,
                    &mut changed_layout_writes,
                    previous != current,
                );
                if previous != current {
                    written.insert(nodes[&id]);
                }
            },
        )?;
        stats.layout_writes = layout_writes;
        stats.changed_layout_writes = changed_layout_writes;
        // Cached out-of-flow raw positions can still need new parent-local
        // offsets when a static ancestor moves. Index only those nodes; ordinary
        // layout frames do not scan all retained slots. Cost is O(M * depth).
        let candidates: std::collections::BTreeSet<_> = self
            .layout_written
            .iter()
            .chain(&self.out_of_flow)
            .copied()
            .collect();
        let origins = candidates
            .into_iter()
            .map(|id| self.parent_layout_origin(id).map(|origin| (id, origin)))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, origin) in origins {
            if self.node(id)?.published_origin != origin {
                self.layout_written.insert(id);
            }
            self.node_mut(id).layout_origin = origin;
        }
        stats.net_changed_nodes = self
            .layout_written
            .iter()
            .filter(|id| {
                let node = self.node(**id).expect("observed live layout");
                node.published_layout != *self.layout.unrounded_layout(node.layout)
                    || node.published_origin != node.layout_origin
            })
            .count() as u64;
        Ok(stats)
    }

    pub(super) fn mark_layout_geometry(&mut self) -> Result<(), Error> {
        for id in &self.layout_written {
            let node = self.node(*id)?;
            let old = &node.published_layout;
            let new = self.layout.layout(node.layout)?;
            // A parent's position or clamped scroll moves every descendant,
            // even those whose relative layout result was a Taffy cache hit.
            let scroll = |layout: &taffy::Layout| {
                [
                    node.scroll[0].min(layout.scroll_width()),
                    node.scroll[1].min(layout.scroll_height()),
                ]
            };
            if node.published_origin != node.layout_origin || scroll(old) != scroll(new) {
                self.property_dirty.insert(*id);
            } else if old.size != new.size || old.padding != new.padding || old.border != new.border
            {
                // Size/content-box changes alone affect this item's quad,
                // glyph wrap and image destination. Changed children are
                // separately observed; equal cached descendants remain shared.
                self.geometry_dirty.insert(*id);
            }
        }
        Ok(())
    }

    pub(super) fn finish_layout_publication(&mut self) {
        for id in std::mem::take(&mut self.layout_written) {
            let layout = self.node(id).expect("published layout identity").layout;
            let published = *self.layout.unrounded_layout(layout);
            let node = self.node_mut(id);
            node.published_layout = published;
            node.published_origin = node.layout_origin;
        }
        self.layout_dirty = false;
    }
}

fn stats_layout_write(writes: &mut u64, changed: &mut u64, differs: bool) {
    *writes += 1;
    *changed += u64::from(differs);
}

#[cfg(test)]
mod tests;
