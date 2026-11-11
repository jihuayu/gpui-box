//! Actual Taffy output changes drive selective geometry publication.
use super::*;

impl UiTree {
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
            let layout = self.node(id).expect("published layout identity").layout;
            self.node_mut(id).published_layout = *self.layout.unrounded_layout(layout);
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
