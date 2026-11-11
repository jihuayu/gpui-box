//! FramePacket consumer for persistent GPUI WGPU quad allocations.
use super::{
    arena::{InstanceArena, InstanceHandle},
    *,
};
use gpui::{Bounds, ContentMask, DevicePixels, Quad, ScaledPixels, Scene, point, rgba, size};
use gpui_wgpu::{QuadBatch, QuadSubmission, QuadWrite, RetainedImage, RetainedQuadRenderer};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Record {
    bounds: Rect,
    color: u32,
}

impl Record {
    fn quad(self) -> Quad {
        let bounds = Bounds {
            origin: point(ScaledPixels(self.bounds.x), ScaledPixels(self.bounds.y)),
            size: size(
                ScaledPixels(self.bounds.width),
                ScaledPixels(self.bounds.height),
            ),
        };
        Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: rgba(self.color).into(),
            ..Default::default()
        }
    }
}

fn gpu_error(error: impl std::fmt::Display) -> Error {
    Error::Gpu(error.to_string())
}

pub struct FrameSubmission {
    pub full_resync: bool,
    pub gpu: QuadSubmission,
}

pub struct WgpuFrameRenderer {
    backend: RetainedQuadRenderer,
    arena: InstanceArena<Record>,
    instances: BTreeMap<NodeId, Vec<InstanceHandle>>,
    source: Option<(u64, u64)>,
    latest: Option<(u64, u64)>,
    retired_sources: BTreeSet<u64>,
    scale: f32,
    surface: [u32; 2],
    leases: VecDeque<(u64, FramePacket)>,
    max_in_flight: usize,
    failed: bool,
}

impl WgpuFrameRenderer {
    pub fn new(max_in_flight: usize) -> Result<Self, Error> {
        Ok(Self {
            backend: RetainedQuadRenderer::new(max_in_flight).map_err(gpu_error)?,
            arena: InstanceArena::default(),
            instances: BTreeMap::new(),
            source: None,
            latest: None,
            retired_sources: BTreeSet::new(),
            scale: 1.0,
            surface: [0, 0],
            leases: VecDeque::new(),
            max_in_flight,
            failed: false,
        })
    }

    pub fn adapter_info(&self) -> gpui_wgpu::wgpu::AdapterInfo {
        self.backend.adapter_info()
    }

    fn collect(&mut self) -> Result<(), Error> {
        let completed = self.backend.completed_serial().map_err(gpu_error)?;
        self.arena.collect(completed);
        while self
            .leases
            .front()
            .is_some_and(|(serial, _)| *serial <= completed)
        {
            self.leases.pop_front();
        }
        Ok(())
    }

    /// Submit a complete owned frame. Sequential frames consume their deltas.
    /// Missing bases, new trees and device recreation consume the full snapshot.
    pub fn submit(
        &mut self,
        frame: &FramePacket,
        surface: [u32; 2],
        scale: f32,
    ) -> Result<Option<FrameSubmission>, Error> {
        if self.failed {
            return Err(Error::Gpu("reset after failed frame submission".into()));
        }
        validate(frame, surface, scale)?;
        if self.retired_sources.contains(&frame.tree)
            || self
                .latest
                .is_some_and(|(tree, revision)| tree == frame.tree && revision > frame.revision)
        {
            return Err(Error::StaleFrame);
        }
        if self.source == Some((frame.tree, frame.revision))
            && self.scale == scale
            && self.surface == surface
        {
            return Ok(None);
        }
        self.collect()?;
        let full_resync = self.source.is_none()
            || self.source != frame.base_revision.map(|revision| (frame.tree, revision))
            || self.scale != scale;
        self.failed = true;
        let selected = if full_resync {
            &frame.items
        } else {
            &frame.updates
        };
        let retire: Vec<_> = if full_resync {
            let live: BTreeSet<_> = frame.items.iter().map(|item| item.node).collect();
            self.instances
                .keys()
                .filter(|node| !live.contains(node))
                .copied()
                .collect()
        } else {
            frame.removed.to_vec()
        };
        let last_submission = self.backend.last_serial();
        for node in retire {
            if let Some(handles) = self.instances.remove(&node) {
                for handle in handles {
                    self.arena
                        .retire(handle, last_submission)
                        .map_err(gpu_error)?;
                }
            }
        }
        self.arena
            .collect(self.backend.completed_serial().map_err(gpu_error)?);
        let mut command_count_changed = false;
        for item in selected.iter() {
            let handles = self.instances.entry(item.node).or_default();
            if handles.len() != item.commands.len() {
                command_count_changed = true;
            }
            while handles.len() > item.commands.len() {
                self.arena
                    .retire(handles.pop().expect("excess command"), last_submission)
                    .map_err(gpu_error)?;
            }
            for (index, command) in item.commands.iter().enumerate() {
                let DisplayCommand::Fill(color) = command;
                let record = Record {
                    bounds: Rect {
                        x: item.bounds.x * scale,
                        y: item.bounds.y * scale,
                        width: item.bounds.width * scale,
                        height: item.bounds.height * scale,
                    },
                    color: *color,
                };
                if let Some(handle) = handles.get(index) {
                    self.arena.update(*handle, record).map_err(gpu_error)?;
                } else {
                    handles.push(self.arena.allocate(record));
                }
            }
        }
        let rebuild_order = full_resync || frame.order.is_some() || command_count_changed;
        let order = if rebuild_order {
            let mut ranges: Vec<Range<u32>> = vec![];
            for item in frame.items.iter() {
                for handle in self.instances.get(&item.node).ok_or(Error::InvalidFrame)? {
                    if let Some(range) = ranges.last_mut().filter(|range| range.end == handle.slot)
                    {
                        range.end += 1;
                    } else {
                        ranges.push(handle.slot..handle.slot + 1);
                    }
                }
            }
            Some(ranges)
        } else {
            None
        };
        let writes = self
            .arena
            .writes()
            .into_iter()
            .map(|write| QuadWrite {
                start: write.start,
                values: write.values.into_iter().map(Record::quad).collect(),
            })
            .collect();
        let gpu = self
            .backend
            .submit(QuadBatch {
                capacity: self.arena.capacity(),
                writes,
                order,
                size: size(
                    DevicePixels(surface[0] as i32),
                    DevicePixels(surface[1] as i32),
                ),
            })
            .map_err(gpu_error)?;
        self.arena.acknowledge();
        if let Some((tree, _)) = self.latest.filter(|(tree, _)| *tree != frame.tree) {
            self.retired_sources.insert(tree);
        }
        self.source = Some((frame.tree, frame.revision));
        self.latest = self.source;
        self.scale = scale;
        self.surface = surface;
        self.leases.push_back((gpu.serial, frame.clone()));
        self.collect()?;
        self.failed = false;
        Ok(Some(FrameSubmission { full_resync, gpu }))
    }

    pub fn image(&mut self) -> Result<RetainedImage, Error> {
        let image = self.backend.image().map_err(gpu_error)?;
        self.collect()?;
        Ok(image)
    }

    pub fn reference_image(
        &mut self,
        frame: &FramePacket,
        surface: [u32; 2],
        scale: f32,
    ) -> Result<RetainedImage, Error> {
        validate(frame, surface, scale)?;
        let mut scene = Scene::default();
        for item in frame.items.iter() {
            for command in item.commands.iter() {
                let DisplayCommand::Fill(color) = command;
                scene.insert_primitive(
                    Record {
                        bounds: Rect {
                            x: item.bounds.x * scale,
                            y: item.bounds.y * scale,
                            width: item.bounds.width * scale,
                            height: item.bounds.height * scale,
                        },
                        color: *color,
                    }
                    .quad(),
                );
            }
        }
        scene.finish();
        self.backend
            .reference_image(
                &scene,
                size(
                    DevicePixels(surface[0] as i32),
                    DevicePixels(surface[1] as i32),
                ),
            )
            .map_err(gpu_error)
    }

    /// A new device allocation epoch starts from a full immutable snapshot.
    pub fn reset(&mut self) -> Result<(), Error> {
        let mut replacement = Self::new(self.max_in_flight)?;
        replacement.latest = self.latest;
        replacement.retired_sources = self.retired_sources.clone();
        *self = replacement;
        Ok(())
    }
}

fn validate(frame: &FramePacket, surface: [u32; 2], scale: f32) -> Result<(), Error> {
    if !scale.is_finite()
        || scale <= 0.0
        || surface
            .iter()
            .any(|size| *size == 0 || *size > i32::MAX as u32)
    {
        return Err(Error::InvalidFrame);
    }
    let mut nodes = BTreeSet::new();
    for item in frame.items.iter().chain(frame.updates.iter()) {
        let b = item.bounds;
        if [b.x, b.y, b.width, b.height]
            .iter()
            .any(|v| !(*v * scale).is_finite())
            || b.width < 0.0
            || b.height < 0.0
        {
            return Err(Error::InvalidFrame);
        }
    }
    for item in frame.items.iter() {
        if item.node.tree != frame.tree || !nodes.insert(item.node) {
            return Err(Error::InvalidFrame);
        }
    }
    if frame
        .base_revision
        .is_some_and(|base| base >= frame.revision)
    {
        return Err(Error::InvalidFrame);
    }
    let current: BTreeMap<_, _> = frame.items.iter().map(|item| (item.node, item)).collect();
    let mut updates = BTreeSet::new();
    for item in frame.updates.iter() {
        if !updates.insert(item.node)
            || current
                .get(&item.node)
                .is_none_or(|value| value.bounds != item.bounds || value.commands != item.commands)
        {
            return Err(Error::InvalidFrame);
        }
    }
    if frame
        .removed
        .iter()
        .any(|id| id.tree != frame.tree || nodes.contains(id))
    {
        return Err(Error::InvalidFrame);
    }
    if frame.order.as_ref().is_some_and(|order| {
        !order
            .iter()
            .copied()
            .eq(frame.items.iter().map(|item| item.node))
    }) {
        return Err(Error::InvalidFrame);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consumer_rejects_mixed_tree_deltas_and_invalid_revisions() {
        let mut tree = UiTree::new(Style::default(), 0xff0000ff).unwrap();
        let frame = tree.frame([100.0, 100.0]).unwrap();
        assert!(validate(&frame, [100, 100], 1.0).is_ok());
        let mut invalid = frame.clone();
        invalid.base_revision = Some(frame.revision);
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
        let mut other = UiTree::new(Style::default(), 0).unwrap();
        let other_frame = other.frame([100.0, 100.0]).unwrap();
        let mut invalid = frame.clone();
        invalid.updates = other_frame.items;
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
        let mut invalid = frame.clone();
        invalid.removed = Arc::from([tree.root()]);
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
    }
}
