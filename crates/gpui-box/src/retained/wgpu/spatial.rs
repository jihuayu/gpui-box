//! Shared GPU placement slots with bounded root-to-leaf shader walks.
use super::*;

#[derive(Default)]
pub(super) struct SpatialArena {
    arena: InstanceArena<gpui_wgpu::SpatialNode>,
    nodes: BTreeMap<NodeId, (InstanceHandle, u32)>,
    parents: BTreeMap<NodeId, Option<NodeId>>,
    cleared: Vec<gpui_wgpu::InstanceWrite<gpui_wgpu::SpatialNode>>,
}
impl SpatialArena {
    pub fn collect(&mut self, serial: u64) {
        self.arena.collect(serial);
    }
    pub fn retire(&mut self, node: NodeId, serial: u64) -> Result<(), Error> {
        self.parents.remove(&node);
        if let Some((handle, _)) = self.nodes.remove(&node) {
            self.arena.retire(handle, serial).map_err(gpu_error)?;
            self.cleared.push(gpui_wgpu::InstanceWrite {
                start: handle.slot,
                values: vec![gpui_wgpu::SpatialNode {
                    links: [u32::MAX, 0, 10, 0],
                    ..Default::default()
                }],
            });
        }
        Ok(())
    }
    pub fn topology_changed(&self, frame: &FramePacket) -> bool {
        frame
            .updates
            .iter()
            .any(|item| self.parents.get(&item.node) != Some(&item.spatial.parent))
    }
    pub fn slot(&self, node: NodeId) -> Result<u32, Error> {
        self.nodes
            .get(&node)
            .and_then(|(h, _)| h.slot.checked_add(1))
            .ok_or(Error::InvalidFrame)
    }
    pub fn update(&mut self, item: &PaintItem) -> Result<u32, Error> {
        self.parents.insert(item.node, item.spatial.parent);
        let parent = item
            .spatial
            .parent
            .map(|node| self.nodes.get(&node).copied().ok_or(Error::InvalidFrame))
            .transpose()?;
        // Deep trees remain correct: resolve a new anchor every 32 nodes.
        // Only anchor world data changes for descendants beyond a bounded walk.
        let anchor = parent.is_none_or(|(_, depth)| depth == 31);
        let depth = if anchor { 0 } else { parent.unwrap().1 + 1 };
        let spatial = &item.spatial;
        let rect = |b: Rect| [b.x, b.y, b.width, b.height];
        let value = gpui_wgpu::SpatialNode {
            layout_translation: if anchor {
                [0.0; 4]
            } else {
                [
                    spatial.layout_origin[0],
                    spatial.layout_origin[1],
                    spatial.translation[0],
                    spatial.translation[1],
                ]
            },
            scroll: [spatial.scroll[0], spatial.scroll[1], 0.0, 0.0],
            clip: if anchor {
                [0.0; 4]
            } else {
                spatial.clip.map_or([0.0; 4], rect)
            },
            anchor_clip: if anchor {
                item.clip.map_or([0.0; 4], rect)
            } else {
                [0.0; 4]
            },
            anchor_origin: if anchor {
                [item.bounds.x, item.bounds.y, 0.0, 0.0]
            } else {
                [0.0; 4]
            },
            links: [
                if anchor {
                    u32::MAX
                } else {
                    parent.unwrap().0.slot
                },
                depth,
                if anchor {
                    2 | if item.clip.is_some() { 8 } else { 0 }
                } else {
                    u32::from(spatial.clip.is_some())
                },
                0,
            ],
        };
        let handle = if let Some((handle, old_depth)) = self.nodes.get_mut(&item.node) {
            self.arena.update(*handle, value).map_err(gpu_error)?;
            *old_depth = depth;
            *handle
        } else {
            let handle = self.arena.allocate(value);
            self.nodes.insert(item.node, (handle, depth));
            handle
        };
        handle.slot.checked_add(1).ok_or(Error::InvalidFrame)
    }
    pub fn batch(&self, enabled: bool, scale: f32) -> gpui_wgpu::SpatialBatch {
        if !enabled {
            return gpui_wgpu::SpatialBatch::default();
        }
        gpui_wgpu::SpatialBatch {
            enabled,
            scale,
            capacity: self.arena.capacity(),
            writes: self
                .cleared
                .iter()
                .map(|w| gpui_wgpu::InstanceWrite {
                    start: w.start,
                    values: w.values.clone(),
                })
                .chain(
                    self.arena
                        .writes()
                        .into_iter()
                        .map(|w| gpui_wgpu::InstanceWrite {
                            start: w.start,
                            values: w.values,
                        }),
                )
                .collect(),
        }
    }
    pub fn acknowledge(&mut self) {
        self.arena.acknowledge();
        self.cleared.clear();
    }
}
