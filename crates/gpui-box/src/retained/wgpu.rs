//! FramePacket consumer for persistent GPUI WGPU quad allocations.
use super::{
    arena::{InstanceArena, InstanceHandle},
    *,
};
use gpui::{Bounds, ContentMask, DevicePixels, Quad, ScaledPixels, Scene, point, rgba, size};
#[cfg(test)]
use gpui_wgpu::wgpu;
use gpui_wgpu::{
    ImageUpdates, QuadBatch, QuadImage, QuadRedraw, QuadSubmission, QuadWrite, RetainedImage,
    RetainedQuadRenderer,
};
// Exercise the native driver's actual mailbox implementation using this
// workspace's locked dependencies, without resolving the upstream test tree.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../../vendor/gpui/crates/gpui_wgpu/src/wgpu_renderer/retained/completion.rs"]
mod completion_protocol_tests;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
mod composition;
pub use composition::CompositionStats;
mod validation;
pub use gpui_wgpu::{CompletionReceiver, CompletionStats};
pub use validation::ValidationStats;
#[cfg(test)]
use validation::validate;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Record {
    bounds: Rect,
    color: u32,
    clip: Option<Rect>,
    decoration: QuadDecoration,
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
            content_mask: ContentMask {
                bounds: self.clip.map_or(bounds, |clip| Bounds {
                    origin: point(ScaledPixels(clip.x), ScaledPixels(clip.y)),
                    size: size(ScaledPixels(clip.width), ScaledPixels(clip.height)),
                }),
            },
            background: rgba(self.color).into(),
            corner_radii: gpui::Corners::all(ScaledPixels(self.decoration.radius)),
            border_widths: gpui::Edges::all(ScaledPixels(self.decoration.border_width)),
            border_color: rgba(self.decoration.border_color).into(),
            ..Default::default()
        }
    }
}

fn record(item: &PaintItem, command: &DisplayCommand, scale: f32) -> Record {
    let (color, decoration) = item.resolved_quad(command);
    let decoration = decoration.resolved(item.bounds);
    let scaled = |bounds: Rect| Rect {
        x: bounds.x * scale,
        y: bounds.y * scale,
        width: bounds.width * scale,
        height: bounds.height * scale,
    };
    Record {
        bounds: scaled(item.bounds),
        color,
        clip: item.clip.map(scaled),
        decoration: QuadDecoration {
            radius: decoration.radius * scale,
            border_width: decoration.border_width * scale,
            ..decoration
        },
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum CompositorPolicy {
    #[default]
    Full,
    Damage,
    Auto {
        max_damage_fraction: f32,
    },
}

fn choose_redraw(
    policy: CompositorPolicy,
    damage: Damage,
    surface: [u32; 2],
    scale: f32,
    force_full: bool,
) -> QuadRedraw {
    if force_full || damage.full || policy == CompositorPolicy::Full {
        return QuadRedraw::Full;
    }
    let Some(bounds) = damage.bounds else {
        return QuadRedraw::Unchanged;
    };
    // Conservative coverage includes a device-pixel fringe for edge AA.
    let x = (bounds.x * scale - 1.0)
        .floor()
        .clamp(0.0, surface[0] as f32) as u32;
    let y = (bounds.y * scale - 1.0)
        .floor()
        .clamp(0.0, surface[1] as f32) as u32;
    let right = ((bounds.x + bounds.width) * scale + 1.0)
        .ceil()
        .clamp(0.0, surface[0] as f32) as u32;
    let bottom = ((bounds.y + bounds.height) * scale + 1.0)
        .ceil()
        .clamp(0.0, surface[1] as f32) as u32;
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    if width == 0 || height == 0 {
        return QuadRedraw::Unchanged;
    }
    if let CompositorPolicy::Auto {
        max_damage_fraction,
    } = policy
    {
        let full_area = (u64::from(surface[0]) * u64::from(surface[1])) as f64;
        if (u64::from(width) * u64::from(height)) as f64
            >= full_area * f64::from(max_damage_fraction)
        {
            return QuadRedraw::Full;
        }
    }
    QuadRedraw::Damage {
        x,
        y,
        width,
        height,
    }
}

fn gpu_error(error: impl std::fmt::Display) -> Error {
    Error::Gpu(error.to_string())
}

pub struct FrameSubmission {
    pub full_resync: bool,
    pub gpu: QuadSubmission,
    pub validation: ValidationStats,
    pub composition: CompositionStats,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameLifecycleStats {
    pub submitted: u64,
    pub completed: u64,
    pub gpu_leases: usize,
    pub frame_leases: usize,
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
    reference: Option<RetainedQuadRenderer>,
    policy: CompositorPolicy,
    composition_cache: composition::CompositionCache,
    cache_composition: bool,
    cache_layers: bool,
    window_image: Option<(u64, Arc<gpui::RenderImage>)>,
    validation_cache: validation::ValidationCache,
    cache_validation: bool,
    resource_deltas: bool,
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
            reference: None,
            policy: CompositorPolicy::Full,
            composition_cache: composition::CompositionCache::default(),
            cache_composition: true,
            cache_layers: true,
            window_image: None,
            validation_cache: validation::ValidationCache::default(),
            cache_validation: true,
            resource_deltas: true,
        })
    }

    pub fn adapter_info(&self) -> gpui_wgpu::wgpu::AdapterInfo {
        self.backend.adapter_info()
    }

    pub fn set_policy(&mut self, policy: CompositorPolicy) -> Result<(), Error> {
        if let CompositorPolicy::Auto {
            max_damage_fraction,
        } = policy
            && (!max_damage_fraction.is_finite() || !(0.0..=1.0).contains(&max_damage_fraction))
        {
            return Err(Error::InvalidFrame);
        }
        if self.policy != policy {
            self.policy = policy;
            self.source = None;
        }
        Ok(())
    }

    /// Disable only packet validation reuse for a same-renderer performance control.
    pub fn set_validation_cache_enabled(&mut self, enabled: bool) {
        self.cache_validation = enabled;
    }

    /// Force complete resource input for a same-consumer performance control.
    pub fn set_resource_deltas_enabled(&mut self, enabled: bool) {
        self.resource_deltas = enabled;
    }

    /// Force complete CPU composition planning without changing resource deltas.
    pub fn set_composition_cache_enabled(&mut self, enabled: bool) {
        self.cache_composition = enabled;
    }

    pub fn set_raster_cache_enabled(&mut self, enabled: bool) {
        if self.cache_layers != enabled {
            self.cache_layers = enabled;
            self.source = None;
        }
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
        let validation =
            self.validation_cache
                .check(frame, surface, scale, self.cache_validation)?;
        if self.retired_sources.contains(&frame.tree)
            || self
                .latest
                .is_some_and(|(tree, revision)| tree == frame.tree && revision > frame.revision)
        {
            return Err(Error::StaleFrame);
        }
        // Completion collection must also run on equal frames: no new visual
        // submission is required to retire the previous immutable frame leases.
        self.collect()?;
        if self.source == Some((frame.tree, frame.revision))
            && self.scale == scale
            && self.surface == surface
        {
            return Ok(None);
        }
        let full_resync = self.source.is_none()
            || self.source != frame.base_revision.map(|revision| (frame.tree, revision))
            || self.scale != scale;
        self.failed = true;
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
        let selected: Box<dyn Iterator<Item = &PaintItem> + '_> = if full_resync {
            Box::new(frame.items.iter())
        } else {
            Box::new(frame.updates.iter())
        };
        for item in selected {
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
                let record = record(item, command, scale);
                if let Some(handle) = handles.get(index) {
                    self.arena.update(*handle, record).map_err(gpu_error)?;
                } else {
                    handles.push(self.arena.allocate(record));
                }
            }
        }
        let (order, layers, composition) = self.composition_cache.resolve(
            frame,
            &self.instances,
            full_resync || command_count_changed || !self.cache_composition,
        )?;
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
                layers,
                images: image_updates(frame, scale, full_resync || !self.resource_deltas),
                cache_layers: self.cache_layers,
                glyphs: glyph_updates(frame, scale, full_resync)?,
                size: size(
                    DevicePixels(surface[0] as i32),
                    DevicePixels(surface[1] as i32),
                ),
                redraw: choose_redraw(
                    self.policy,
                    frame.damage,
                    surface,
                    scale,
                    full_resync || self.surface != surface,
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
        Ok(Some(FrameSubmission {
            full_resync,
            gpu,
            validation,
            composition,
        }))
    }

    pub fn image(&mut self) -> Result<RetainedImage, Error> {
        let image = self.backend.image().map_err(gpu_error)?;
        self.collect()?;
        Ok(image)
    }

    /// Correct window presentation bridge for isolated groups. This waits for a
    /// readback and uploads through the legacy sprite atlas; direct import is pending.
    pub fn window_image(&mut self) -> Result<Arc<gpui::RenderImage>, Error> {
        let serial = self.backend.last_serial();
        if let Some((cached, image)) = &self.window_image
            && *cached == serial
        {
            return Ok(image.clone());
        }
        let image = self.backend.window_image().map_err(gpu_error)?;
        self.collect()?;
        self.window_image = Some((serial, image.clone()));
        Ok(image)
    }

    /// Poll completions while the producer is idle and release finished frame leases.
    pub fn poll(&mut self) -> Result<(), Error> {
        self.collect()
    }

    /// Subscribe to native completion wakeups without a polling timer. Await
    /// readiness outside any owner borrow, then `poll` on the UI owner. This
    /// subscription closes on receiver drop, renderer drop or device reset.
    pub fn open_completions(&mut self) -> Result<CompletionReceiver, Error> {
        self.backend.open_completions().map_err(gpu_error)
    }

    /// Counters only: unlike `poll`, this cannot complete or collect any work.
    pub fn lifecycle(&self) -> FrameLifecycleStats {
        let (submitted, completed, gpu_leases) = self.backend.lifecycle();
        FrameLifecycleStats {
            submitted,
            completed,
            gpu_leases,
            frame_leases: self.leases.len(),
        }
    }
    pub fn completion_stats(&self) -> Option<CompletionStats> {
        self.backend.completion_stats()
    }

    /// Wait for all queued work, excluding texture readback.
    pub fn finish(&mut self) -> Result<(), Error> {
        self.backend.finish().map_err(gpu_error)?;
        self.collect()
    }

    pub fn reference_image(
        &mut self,
        frame: &FramePacket,
        surface: [u32; 2],
        scale: f32,
    ) -> Result<RetainedImage, Error> {
        validation::validate(frame, surface, scale)?;
        if !frame.properties.effects.is_empty()
            || has_text(frame)
            || frame.items.iter().any(|item| item.image.is_some())
        {
            // Legacy GPUI flattens element opacity. Group isolation instead uses
            // a fresh full renderer with no previous raster-cache contents.
            let mut fresh = Self::new(1)?;
            fresh.submit(frame, surface, scale)?;
            return fresh.image();
        }
        let mut scene = Scene::default();
        for item in frame.items.iter() {
            for command in item.commands.iter() {
                scene.insert_primitive(record(item, command, scale).quad());
            }
        }
        scene.finish();
        if self.reference.is_none() {
            self.reference = Some(RetainedQuadRenderer::new(1).map_err(gpu_error)?);
        }
        self.reference
            .as_mut()
            .expect("reference backend")
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
        replacement.policy = self.policy;
        replacement.cache_layers = self.cache_layers;
        replacement.cache_validation = self.cache_validation;
        replacement.resource_deltas = self.resource_deltas;
        replacement.cache_composition = self.cache_composition;
        *self = replacement;
        Ok(())
    }
}

fn image_updates(frame: &FramePacket, scale: f32, reset: bool) -> ImageUpdates {
    let mut updates = ImageUpdates {
        reset,
        ..Default::default()
    };
    if !reset {
        updates.removed = frame
            .removed
            .iter()
            .map(|node| [node.tree, node.slot as u64, node.generation])
            .collect();
    }
    let items: Box<dyn Iterator<Item = &PaintItem> + '_> = if reset {
        Box::new(frame.items.iter())
    } else {
        Box::new(frame.updates.iter())
    };
    for item in items {
        let id = [item.node.tree, item.node.slot as u64, item.node.generation];
        let Some(image) = &item.image else {
            if !reset {
                updates.removed.push(id);
            }
            continue;
        };
        let rect = |r: Rect| [r.x * scale, r.y * scale, r.width * scale, r.height * scale];
        updates.images.push(QuadImage {
            id,
            resource: image.id(),
            size: image.size(),
            rgba: image.pixels().clone(),
            bounds: rect(item.content_bounds),
            clip: rect(item.clip.unwrap_or(item.bounds)),
        });
    }
    updates
}

fn has_text(frame: &FramePacket) -> bool {
    #[cfg(feature = "retained-text")]
    {
        frame.items.iter().any(|item| item.text.is_some())
    }
    #[cfg(not(feature = "retained-text"))]
    {
        let _ = frame;
        false
    }
}
fn glyph_updates(
    frame: &FramePacket,
    scale: f32,
    reset: bool,
) -> Result<gpui_wgpu::GlyphUpdates, Error> {
    let mut updates = gpui_wgpu::GlyphUpdates {
        reset,
        ..Default::default()
    };
    let items: Box<dyn Iterator<Item = &PaintItem> + '_> = if reset {
        Box::new(frame.items.iter())
    } else {
        Box::new(frame.updates.iter())
    };
    if !reset {
        updates.removed = frame
            .removed
            .iter()
            .map(|node| [node.tree, node.slot as u64, node.generation])
            .collect();
    }
    #[cfg(feature = "retained-text")]
    {
        for item in items {
            let Some(text) = &item.text else {
                if !reset {
                    updates.removed.push([
                        item.node.tree,
                        item.node.slot as u64,
                        item.node.generation,
                    ]);
                }
                continue;
            };
            let chunk = item.glyphs.as_ref().ok_or(Error::InvalidFrame)?;
            let clip = item.clip.unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                width: i32::MAX as f32 / scale,
                height: i32::MAX as f32 / scale,
            });
            updates.batches.push(gpui_wgpu::GlyphBatch {
                id: [item.node.tree, item.node.slot as u64, item.node.generation],
                clip: [
                    clip.x * scale,
                    clip.y * scale,
                    clip.width * scale,
                    clip.height * scale,
                ],
                color: text.color,
                origin: [item.content_bounds.x, item.content_bounds.y],
                scale,
                geometry: chunk.gpu_geometry()?,
            });
        }
        Ok(updates)
    }
    #[cfg(not(feature = "retained-text"))]
    {
        let _ = (items, scale);
        Ok(updates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redraw_covers_fractional_edges_and_auto_falls_back_for_large_damage() {
        let damage = Damage {
            full: false,
            bounds: Some(Rect {
                x: 10.25,
                y: 12.25,
                width: 4.0,
                height: 6.0,
            }),
        };
        assert_eq!(
            choose_redraw(CompositorPolicy::Damage, damage, [100, 100], 1.5, false),
            QuadRedraw::Damage {
                x: 14,
                y: 17,
                width: 9,
                height: 12
            }
        );
        assert_eq!(
            choose_redraw(
                CompositorPolicy::Auto {
                    max_damage_fraction: 0.01
                },
                damage,
                [100, 100],
                1.5,
                false
            ),
            QuadRedraw::Full
        );
        assert_eq!(
            choose_redraw(
                CompositorPolicy::Damage,
                Damage::default(),
                [100, 100],
                1.0,
                false
            ),
            QuadRedraw::Unchanged
        );
        assert_eq!(
            choose_redraw(
                CompositorPolicy::Damage,
                Damage::default(),
                [100, 100],
                1.0,
                true
            ),
            QuadRedraw::Full
        );
    }
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
        invalid.updates = other_frame.items.iter().cloned().collect::<Vec<_>>().into();
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
        let mut invalid = frame.clone();
        Arc::make_mut(&mut invalid.updates)[0].image =
            Some(ImageResource::rgba8([1, 1], vec![255; 4]).unwrap());
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
        let mut invalid = frame.clone();
        Arc::make_mut(&mut invalid.items)[0].commands =
            Arc::from([DisplayCommand::DecoratedFill {
                color: 0,
                decoration: QuadDecoration {
                    radius: f32::NAN,
                    ..Default::default()
                },
            }]);
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
        let mut invalid = frame.clone();
        Arc::make_mut(&mut invalid.properties).effects.insert(
            tree.root(),
            PropertyNode {
                parent: Some(tree.root()),
                value: 0.5,
            },
        );
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
        let mut invalid = frame.clone();
        Arc::make_mut(&mut invalid.items)[0].properties.effect = Some(tree.root());
        assert_eq!(
            validate(&invalid, [100, 100], 1.0),
            Err(Error::InvalidFrame)
        );
    }
}
