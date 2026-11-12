//! Transactional CPU indices for the validated, addressable GPU prefixes.
use super::SpatialNode;
use std::{collections::BTreeMap, ops::Range};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpatialWorkStats {
    pub indexed: bool,
    pub full_spatial_scan: bool,
    pub dense_fallback: bool,
    pub checked_edges: usize,
    pub resolved_nodes: usize,
    pub quad_references: usize,
    pub shadow_references: usize,
    pub quad_affected_instances: usize,
    pub shadow_affected_instances: usize,
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub(super) struct Resolved {
    pub origin: [f32; 2],
    pub scroll: [f32; 2],
    pub clip: Option<[f32; 4]>,
}
#[derive(Clone, Copy, Default)]
struct Scratch {
    epoch: u64,
    value: Resolved,
}
#[derive(Default)]
pub(super) struct GraphIndex {
    valid: bool,
    memo_valid: bool,
    scale: f32,
    parents: Vec<Option<u32>>,
    positions: Vec<usize>,
    children: Vec<Vec<u32>>,
    resolved: Vec<Resolved>,
    scratch: Vec<Scratch>,
    visited: Vec<u64>,
    epoch: u64,
}
pub(super) struct GraphPlan {
    capacity: usize,
    scale: f32,
    pub full: bool,
    pub dense: bool,
    parents: Vec<(u32, Option<u32>)>,
    values: Vec<(u32, Resolved)>,
    pub affected: Vec<u32>,
}
impl GraphIndex {
    /// Scratch stamps may change on a rejected input; authoritative state does
    /// not. The plan is committed only after the entire batch passes preflight.
    pub fn prepare(
        &mut self,
        capacity: u32,
        scale: f32,
        adaptive: bool,
        writes: impl Iterator<Item = (u32, SpatialNode)>,
        read: impl Fn(u32) -> SpatialNode,
    ) -> Result<GraphPlan, &'static str> {
        let writes: BTreeMap<_, _> = writes.filter(|(slot, _)| *slot < capacity).collect();
        let get = |slot| writes.get(&slot).copied().unwrap_or_else(|| read(slot));
        let capacity = capacity as usize;
        let full = !self.valid || self.parents.len() != capacity || self.scale != scale;
        self.scratch.resize(capacity, Scratch::default());
        self.visited.resize(capacity, 0);
        self.epoch = match self.epoch.checked_add(1) {
            Some(value) => value,
            None => {
                self.scratch.fill(Scratch::default());
                self.visited.fill(0);
                1
            }
        };
        let mut affected: Vec<u32> = if full {
            (0..capacity as u32).collect()
        } else {
            writes.keys().copied().collect()
        };
        if !full {
            let budget = (capacity / 4).max(64);
            if adaptive && affected.len() > budget {
                return Ok(GraphPlan::dense(capacity, scale, &writes));
            }
            for slot in &affected {
                self.visited[*slot as usize] = self.epoch;
            }
            let mut cursor = 0;
            while cursor < affected.len() {
                for child in &self.children[affected[cursor] as usize] {
                    if self.visited[*child as usize] != self.epoch {
                        self.visited[*child as usize] = self.epoch;
                        affected.push(*child);
                        if adaptive && affected.len() > budget {
                            return Ok(GraphPlan::dense(capacity, scale, &writes));
                        }
                    }
                }
                cursor += 1;
            }
        }
        let mut buckets: [Vec<u32>; 32] = Default::default();
        let mut parents = Vec::with_capacity(if full { capacity } else { writes.len() });
        for slot in &affected {
            let node = get(*slot);
            if node.links[1] >= 32 {
                return Err("invalid spatial depth");
            }
            let anchor = node.links[2] & 2 != 0;
            if anchor {
                if node.links[1] != 0 || node.links[0] != u32::MAX {
                    return Err("invalid spatial anchor");
                }
            } else if node.links[0] as usize >= capacity
                || get(node.links[0]).links[1].checked_add(1) != Some(node.links[1])
            {
                return Err("cyclic or invalid spatial parent/depth");
            }
            if full || writes.contains_key(slot) {
                parents.push((*slot, (!anchor).then_some(node.links[0])));
            }
            buckets[node.links[1] as usize].push(*slot);
        }
        let mut values = Vec::with_capacity(affected.len());
        for slot in buckets.into_iter().flatten() {
            let node = get(slot);
            let state = if node.links[2] & 2 != 0 {
                resolve_node(node, None)
            } else {
                let parent = node.links[0] as usize;
                let parent = if self.scratch[parent].epoch == self.epoch {
                    self.scratch[parent].value
                } else if self.memo_valid {
                    self.resolved[parent]
                } else {
                    self.current(node.links[0], &get)?
                };
                resolve_node(node, Some(parent))
            };
            if !state.origin.iter().all(|v| (*v * scale).is_finite()) {
                return Err("scaled spatial origin overflow");
            }
            if let Some(c) = state.clip
                && ![c[0], c[1], c[2], c[3], c[0] + c[2], c[1] + c[3]]
                    .iter()
                    .all(|v| (*v * scale).is_finite())
            {
                return Err("spatial clip overflow");
            }
            self.scratch[slot as usize] = Scratch {
                epoch: self.epoch,
                value: state,
            };
            values.push((slot, state));
        }
        Ok(GraphPlan {
            capacity,
            scale,
            full,
            dense: false,
            parents,
            values,
            affected,
        })
    }
    fn current(
        &mut self,
        mut slot: u32,
        get: &impl Fn(u32) -> SpatialNode,
    ) -> Result<Resolved, &'static str> {
        let mut path = [0u32; 32];
        let mut len = 0;
        let mut state = loop {
            if self.scratch[slot as usize].epoch == self.epoch {
                break self.scratch[slot as usize].value;
            }
            let node = get(slot);
            if node.links[2] & 2 != 0 {
                break resolve_node(node, None);
            }
            if len == path.len() {
                return Err("invalid indexed ancestry");
            }
            path[len] = slot;
            len += 1;
            slot = node.links[0];
        };
        for slot in path[..len].iter().rev() {
            state = resolve_node(get(*slot), Some(state));
            self.scratch[*slot as usize] = Scratch {
                epoch: self.epoch,
                value: state,
            };
        }
        Ok(state)
    }
    pub fn commit(&mut self, plan: GraphPlan) {
        if plan.full {
            self.memo_valid = true;
            self.parents = vec![None; plan.capacity];
            self.positions = vec![0; plan.capacity];
            self.children = vec![vec![]; plan.capacity];
            self.resolved = vec![Resolved::default(); plan.capacity];
        }
        for (slot, parent) in plan.parents {
            let index = slot as usize;
            if self.parents[index] == parent {
                continue;
            }
            if let Some(old) = self.parents[index] {
                let children = &mut self.children[old as usize];
                let position = self.positions[index];
                children.swap_remove(position);
                if let Some(moved) = children.get(position) {
                    self.positions[*moved as usize] = position;
                }
            }
            if let Some(parent) = parent {
                self.positions[index] = self.children[parent as usize].len();
                self.children[parent as usize].push(slot);
            }
            self.parents[index] = parent;
        }
        for (slot, value) in plan.values {
            self.resolved[slot as usize] = value;
        }
        if plan.dense {
            self.memo_valid = false;
        }
        self.scale = plan.scale;
        self.valid = true;
    }
}

impl GraphPlan {
    // Caller must run full projected validation before committing this plan.
    fn dense(capacity: usize, scale: f32, writes: &BTreeMap<u32, SpatialNode>) -> Self {
        Self {
            capacity,
            scale,
            full: false,
            dense: true,
            parents: writes
                .iter()
                .map(|(s, n)| (*s, (n.links[2] & 2 == 0).then_some(n.links[0])))
                .collect(),
            values: vec![],
            affected: vec![],
        }
    }
}
pub(super) fn resolve_node(node: SpatialNode, parent: Option<Resolved>) -> Resolved {
    let Some(parent) = parent else {
        return Resolved {
            origin: [node.anchor_origin[0], node.anchor_origin[1]],
            scroll: [node.scroll[0], node.scroll[1]],
            clip: (node.links[2] & 8 != 0).then_some(node.anchor_clip),
        };
    };
    let mut origin = parent.origin;
    for (axis, value) in origin.iter_mut().enumerate() {
        *value = (*value - parent.scroll[axis]) + node.layout_translation[axis];
        *value += node.layout_translation[axis + 2];
    }
    let mut clip = parent.clip;
    if node.links[2] & 1 != 0 {
        let mut own = node.clip;
        own[0] += origin[0];
        own[1] += origin[1];
        clip = Some(if let Some(old) = clip {
            let x = old[0].max(own[0]);
            let y = old[1].max(own[1]);
            [
                x,
                y,
                (old[0] + old[2]).min(own[0] + own[2]).max(x) - x,
                (old[1] + old[3]).min(own[1] + own[3]).max(y) - y,
            ]
        } else {
            own
        });
    }
    Resolved {
        origin,
        scroll: [node.scroll[0], node.scroll[1]],
        clip,
    }
}

#[derive(Default)]
pub(super) struct ReferenceIndex {
    valid: bool,
    properties: Vec<u32>,
    positions: Vec<usize>,
    users: Vec<Vec<u32>>,
    max_property: u32,
}
pub(super) struct ReferencePlan {
    capacity: usize,
    user_capacity: usize,
    full: bool,
    edits: Vec<(u32, u32)>,
    pub checked: usize,
}
impl ReferenceIndex {
    pub fn prepare(
        &self,
        capacity: u32,
        spatial_capacity: u32,
        writes: impl Iterator<Item = (u32, u32)>,
        read: impl Fn(u32) -> u32,
    ) -> Result<ReferencePlan, &'static str> {
        let writes: BTreeMap<_, _> = writes.filter(|(slot, _)| *slot < capacity).collect();
        let full = !self.valid
            || self.properties.len() != capacity as usize
            || self.max_property > spatial_capacity;
        let edits: Vec<_> = if full {
            (0..capacity)
                .map(|slot| {
                    (
                        slot,
                        writes.get(&slot).copied().unwrap_or_else(|| read(slot)),
                    )
                })
                .collect()
        } else {
            writes.into_iter().collect()
        };
        if edits
            .iter()
            .any(|(_, property)| *property > spatial_capacity)
        {
            return Err("spatial reference outside allocation");
        }
        let user_capacity = edits
            .iter()
            .map(|(_, p)| *p as usize + 1)
            .max()
            .unwrap_or(1);
        Ok(ReferencePlan {
            capacity: capacity as usize,
            user_capacity,
            full,
            checked: edits.len(),
            edits,
        })
    }
    pub fn commit(&mut self, plan: ReferencePlan) {
        if plan.full {
            self.properties = vec![0; plan.capacity];
            self.positions = vec![0; plan.capacity];
            self.users = vec![vec![]; plan.user_capacity];
            self.max_property = 0;
        } else {
            self.users
                .resize_with((plan.user_capacity).max(self.users.len()), Vec::new);
        }
        for (slot, property) in plan.edits {
            let index = slot as usize;
            let old = self.properties[index];
            if old == property {
                continue;
            }
            if old != 0 {
                let users = &mut self.users[old as usize];
                let position = self.positions[index];
                users.swap_remove(position);
                if let Some(moved) = users.get(position) {
                    self.positions[*moved as usize] = position;
                }
            }
            if property != 0 {
                self.positions[index] = self.users[property as usize].len();
                self.users[property as usize].push(slot);
                self.max_property = self.max_property.max(property);
            }
            self.properties[index] = property;
        }
        if self.max_property != 0 && self.users[self.max_property as usize].is_empty() {
            self.max_property = self.users.iter().rposition(|v| !v.is_empty()).unwrap_or(0) as u32;
        }
        self.valid = true;
    }
    pub fn affected(&self, properties: &[u32], dirty: &mut Vec<Range<u32>>) -> usize {
        let mut slots: Vec<_> = properties
            .iter()
            .flat_map(|p| {
                self.users
                    .get(*p as usize + 1)
                    .into_iter()
                    .flatten()
                    .copied()
            })
            .collect();
        slots.sort_unstable();
        slots.dedup();
        let count = slots.len();
        let mut added: Vec<Range<u32>> = vec![];
        for slot in slots {
            if let Some(last) = added.last_mut().filter(|r| r.end == slot) {
                last.end += 1;
            } else {
                added.push(slot..slot + 1);
            }
        }
        dirty.extend(added);
        count
    }
}

#[cfg(test)]
#[path = "spatial_index/tests.rs"]
mod tests;
