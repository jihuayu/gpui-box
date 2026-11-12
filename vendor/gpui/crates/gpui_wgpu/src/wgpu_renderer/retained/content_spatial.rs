//! Shared content placement and inverse image/glyph property memberships.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub(super) type ContentId = [u64; 3];

/// A shared border-box property plus the two ordered content-box additions.
/// None at an image/glyph draw retains the original absolute placement path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialContent {
    /// One-based SpatialNode slot, matching Quad/Shadow references.
    pub property: u32,
    pub border: [f32; 2],
    pub padding: [f32; 2],
}
impl SpatialContent {
    pub(super) fn origin(
        &self,
        cache: &spatial::SpatialCache,
        batch: &SpatialBatch,
    ) -> Result<[f32; 2]> {
        anyhow::ensure!(
            batch.enabled && self.property > 0 && self.property <= batch.capacity,
            "invalid shared content reference"
        );
        anyhow::ensure!(
            self.border
                .iter()
                .chain(&self.padding)
                .all(|v| v.is_finite()),
            "invalid content offsets"
        );
        let state = cache.projected_state(self.property - 1, batch)?;
        let origin = [
            (state.origin[0] + self.border[0]) + self.padding[0],
            (state.origin[1] + self.border[1]) + self.padding[1],
        ];
        anyhow::ensure!(
            origin
                .iter()
                .all(|v| v.is_finite() && (*v * batch.scale).is_finite()),
            "shared content origin overflow"
        );
        Ok(origin)
    }
}

#[derive(Default)]
pub(super) struct ContentIndex {
    users: BTreeMap<u32, BTreeSet<ContentId>>,
}
impl ContentIndex {
    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }
    pub fn set(&mut self, id: ContentId, old: Option<SpatialContent>, new: Option<SpatialContent>) {
        let old = old.map(|s| s.property);
        let new = new.map(|s| s.property);
        if old == new {
            return;
        }
        if let Some(property) = old {
            let users = self
                .users
                .get_mut(&property)
                .expect("content property membership");
            users.remove(&id);
            if users.is_empty() {
                self.users.remove(&property);
            }
        }
        if let Some(property) = new {
            self.users.entry(property).or_default().insert(id);
        }
    }
    pub fn affected(
        &self,
        cache: &spatial::SpatialCache,
        batch: &SpatialBatch,
        slots: Option<&[u32]>,
        all: bool,
    ) -> BTreeSet<ContentId> {
        if all {
            return self.users.values().flatten().copied().collect();
        }
        if batch.writes.is_empty() {
            return BTreeSet::new();
        }
        if let Some(slots) = slots {
            slots
                .iter()
                .filter_map(|s| self.users.get(&(s + 1)))
                .flatten()
                .copied()
                .collect()
        } else {
            self.users
                .iter()
                .filter(|(property, _)| cache.projected_affected(**property, batch))
                .flat_map(|(_, ids)| ids)
                .copied()
                .collect()
        }
    }
}

pub(super) fn shader(source: &str) -> String {
    let mut result = String::from("struct Bounds { origin: vec2<f32>, size: vec2<f32> }\n");
    result.push_str(include_str!("spatial_resolve.wgsl"));
    result.push_str(source);
    result
}
