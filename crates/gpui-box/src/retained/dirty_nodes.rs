//! Private owner-local membership journal. Ordering is never paint/event order.
use super::NodeId;

#[derive(Default)]
pub(super) struct DirtyNodes {
    positions: Vec<Option<usize>>,
    nodes: Vec<NodeId>,
}
impl DirtyNodes {
    pub fn insert(&mut self, node: NodeId) {
        if self.positions.len() <= node.slot {
            self.positions.resize(node.slot + 1, None);
        }
        if let Some(index) = self.positions[node.slot] {
            // A newer generation may replace a removed node before publication.
            // The complete extraction already covers retirement; never revive
            // a stale identity merely because its slot was previously dirty.
            self.nodes[index] = node;
        } else {
            self.positions[node.slot] = Some(self.nodes.len());
            self.nodes.push(node);
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &NodeId> {
        self.nodes.iter()
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn clear(&mut self) {
        for node in self.nodes.drain(..) {
            self.positions[node.slot] = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_generation_replacement_and_clear_match_latest_membership() {
        let mut dirty = DirtyNodes::default();
        let mut expected = std::collections::BTreeMap::new();
        for round in 0..8 {
            for i in 0..2000 {
                let slot = i * 73 % 257;
                let node = NodeId {
                    tree: 7,
                    slot,
                    generation: (round * 2000 + i) as u64,
                };
                dirty.insert(node);
                expected.insert(slot, node);
            }
            let actual: std::collections::BTreeMap<_, _> =
                dirty.iter().map(|id| (id.slot, *id)).collect();
            assert_eq!(actual, expected);
            assert_eq!(dirty.len(), expected.len());
            dirty.clear();
            expected.clear();
            assert!(dirty.is_empty());
        }
    }
}
