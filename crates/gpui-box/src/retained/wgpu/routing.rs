//! Classify complete base-relative updates against actual submitted snapshots.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoutingStats {
    pub cached: bool,
    pub inspected_items: usize,
    pub prepared_quad_items: usize,
    pub prepared_shadow_items: usize,
    pub prepared_spatial_items: usize,
    pub rebuilt_spatial_items: usize,
    pub reused_quad_items: usize,
    pub reused_shadow_items: usize,
    pub untrusted_full_resync: bool,
}
pub(super) struct Snapshot {
    tree: u64,
    revision: u64,
    items: Arc<PaintItems>,
    properties: Arc<PropertyTrees>,
    depths: Arc<[u8]>,
}
pub(super) struct Entry<'a> {
    pub item: &'a PaintItem,
    pub quad: bool,
    pub shadow: bool,
    pub spatial: bool,
    pub image: bool,
    pub glyph: bool,
}
pub(super) struct Plan<'a> {
    pub entries: Vec<Entry<'a>>,
    pub parent_changed: bool,
    pub stats: RoutingStats,
}
impl Snapshot {
    pub fn follows(&self, frame: &FramePacket) -> bool {
        frame.updates.follows_snapshot(
            frame,
            self.tree,
            self.revision,
            &self.items,
            &self.properties,
        )
    }
    pub fn same_scene(&self, frame: &FramePacket) -> bool {
        Arc::ptr_eq(&self.items, &frame.items) && Arc::ptr_eq(&self.properties, &frame.properties)
    }
    pub fn plan<'a>(
        &self,
        frame: &'a FramePacket,
        validation: &validation::ValidationCache,
        shared: bool,
        content_shared: bool,
    ) -> Option<Plan<'a>> {
        if self.items.len() != frame.items.len()
            || frame.order.is_some()
            || shared && self.depths.len() != frame.items.len()
        {
            return None;
        }
        let mut plan = Plan {
            entries: vec![],
            parent_changed: false,
            stats: RoutingStats {
                cached: true,
                ..Default::default()
            },
        };
        let mut classify = |index: usize, item: &'a PaintItem| -> Option<()> {
            let old = self.items.get(index)?;
            if old.node != item.node {
                return None;
            }
            let mut entry = classify(old, item, shared, shared && self.depths[index] == 0);
            if content_shared {
                (entry.image, entry.glyph) = classify_content(old, item, true);
            }
            plan.parent_changed |= old.spatial.parent != item.spatial.parent;
            plan.stats.inspected_items += 1;
            plan.stats.reused_quad_items += usize::from(!entry.quad);
            plan.stats.reused_shadow_items += usize::from(!entry.shadow);
            if entry.quad || entry.shadow || entry.spatial || entry.image || entry.glyph {
                plan.entries.push(entry);
            }
            Some(())
        };
        if let Some(indices) = frame.updates.indices_for(&frame.items) {
            for index in indices {
                classify(*index, &frame.items[*index])?;
            }
        } else {
            for item in frame.updates.iter() {
                classify(validation.paint_index(item.node)?, item)?;
            }
        }
        Some(plan)
    }
    pub fn next(
        frame: &FramePacket,
        previous: Option<&Self>,
        validation: &validation::ValidationCache,
        rebuild: bool,
        shared: bool,
    ) -> Result<Self, Error> {
        let depths = if !shared {
            Arc::from([])
        } else if !rebuild && previous.is_some_and(|p| p.depths.len() == frame.items.len()) {
            previous.unwrap().depths.clone()
        } else {
            let mut depths: Vec<u8> = Vec::with_capacity(frame.items.len());
            for (index, item) in frame.items.iter().enumerate() {
                let depth = if let Some(parent) = item.spatial.parent {
                    let parent = validation
                        .paint_index(parent)
                        .filter(|p| *p < index)
                        .ok_or(Error::InvalidFrame)?;
                    if depths[parent] == 31 {
                        0
                    } else {
                        depths[parent] + 1
                    }
                } else {
                    0
                };
                depths.push(depth);
            }
            depths.into()
        };
        Ok(Self {
            tree: frame.tree,
            revision: frame.revision,
            items: frame.items.clone(),
            properties: frame.properties.clone(),
            depths,
        })
    }
}
fn radius(item: &PaintItem) -> f32 {
    item.commands
        .first()
        .map_or(0.0, |c| c.quad().1.resolved(item.bounds).radius)
}
fn classify<'a>(old: &PaintItem, item: &'a PaintItem, shared: bool, anchor: bool) -> Entry<'a> {
    let same_size =
        old.bounds.width == item.bounds.width && old.bounds.height == item.bounds.height;
    let same_world = old.bounds == item.bounds && old.clip == item.clip;
    let same_placement = shared || same_world;
    let same_quad = same_size
        && same_placement
        && old.fill_color == item.fill_color
        && (Arc::ptr_eq(&old.commands, &item.commands) || old.commands == item.commands);
    let same_shadow = (old.shadows.is_empty() && item.shadows.is_empty())
        || same_size
            && same_placement
            && radius(old) == radius(item)
            && (Arc::ptr_eq(&old.shadows, &item.shadows) || old.shadows == item.shadows);
    let spatial = shared
        && (old.spatial != item.spatial
            || anchor
                && ([old.bounds.x, old.bounds.y] != [item.bounds.x, item.bounds.y]
                    || old.clip != item.clip));
    Entry {
        item,
        quad: !same_quad,
        shadow: !same_shadow,
        spatial,
        image: false,
        glyph: false,
    }
}

fn classify_content(old: &PaintItem, item: &PaintItem, shared: bool) -> (bool, bool) {
    let same_placement = if shared {
        old.content_offsets == item.content_offsets
    } else {
        old.content_bounds == item.content_bounds
            && old.bounds == item.bounds
            && old.clip == item.clip
    };
    let image = old.image != item.image
        || item.image.is_some()
            && (!same_placement
                || old.content_bounds.width != item.content_bounds.width
                || old.content_bounds.height != item.content_bounds.height);
    #[cfg(feature = "retained-text")]
    let glyph = !old.same_text(item) || item.text.is_some() && !same_placement;
    #[cfg(not(feature = "retained-text"))]
    let glyph = false;
    (image, glyph)
}

#[cfg(test)]
mod tests;
