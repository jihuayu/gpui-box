//! Actual Taffy output changes drive selective geometry publication.
use super::*;

impl UiTree {
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
        let baseline = self
            .node(id)?
            .measure
            .as_ref()
            .and_then(MeasureSnapshot::baseline)
            .map(|value| value + result.padding.top + result.border.top);
        Ok(LayoutResult::from_taffy_and_baseline(result, baseline))
    }

    pub(super) fn compute_layout(&mut self, viewport: [f32; 2]) -> Result<LayoutStats, Error> {
        let root = self.node(self.root)?.layout;
        let mut stats = LayoutStats::default();
        let mut layout_writes = 0;
        let mut changed_layout_writes = 0;
        let nodes = &self.layout_nodes;
        let written = &mut self.layout_written;
        self.layout.compute_layout_with_measure_and_observer(
            root,
            Size {
                width: AvailableSpace::Definite(viewport[0]),
                height: AvailableSpace::Definite(viewport[1]),
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
        stats.net_changed_nodes = self
            .layout_written
            .iter()
            .filter(|id| {
                let node = self.node(**id).expect("observed live layout");
                node.published_layout != *self.layout.unrounded_layout(node.layout)
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
            if old.location != new.location || scroll(old) != scroll(new) {
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
            let (layout, measure) = {
                let node = self.node(id).expect("published layout identity");
                (node.layout, node.measure.clone())
            };
            let published = *self.layout.unrounded_layout(layout);
            let baseline = measure
                .as_ref()
                .and_then(MeasureSnapshot::baseline)
                .map(|value| value + published.padding.top + published.border.top);
            let node = self.node_mut(id);
            node.published_layout = published;
            node.published_baseline = baseline;
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
