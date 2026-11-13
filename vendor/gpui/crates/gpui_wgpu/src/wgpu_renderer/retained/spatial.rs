//! Shared hierarchical placement for retained Quad and Shadow instances.
use super::*;
use storage::Instance;

/// Six explicitly initialized vec4s; local logical placement plus bounded-chain
/// anchors. Root/deep anchors carry resolved origin/clip; ordinary nodes do not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct SpatialNode {
    pub layout_translation: [f32; 4],
    pub scroll: [f32; 4],
    pub clip: [f32; 4],
    pub anchor_clip: [f32; 4],
    pub anchor_origin: [f32; 4],
    /// Parent slot, distance to anchor (0..31), flags (clip=1, anchor=2,
    /// anchor has clip=8), reserved zero.
    pub links: [u32; 4],
}
const _: () = assert!(std::mem::size_of::<SpatialNode>() == 96);
// SAFETY: repr(C), all vec4 fields initialized and no implicit padding; Pod checks it.
unsafe impl Instance for SpatialNode {
    fn empty() -> Self {
        Self {
            links: [u32::MAX, 0, 10, 0],
            ..Default::default()
        }
    }
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.layout_translation
                .iter()
                .chain(&self.scroll)
                .chain(&self.clip)
                .chain(&self.anchor_clip)
                .chain(&self.anchor_origin)
                .all(|v| v.is_finite())
                && self.clip[2] >= 0.0
                && self.clip[3] >= 0.0
                && self.anchor_clip[2] >= 0.0
                && self.anchor_clip[3] >= 0.0
                && self.links[1] < 32
                && self.links[2] & !11 == 0
                && self.links[3] == 0,
            "invalid spatial node"
        );
        Ok(())
    }
}

pub struct SpatialBatch {
    pub enabled: bool,
    pub capacity: u32,
    pub scale: f32,
    pub writes: Vec<InstanceWrite<SpatialNode>>,
}
impl Default for SpatialBatch {
    fn default() -> Self {
        Self {
            enabled: false,
            capacity: 0,
            scale: 1.0,
            writes: vec![],
        }
    }
}
pub(super) type SpatialLease = (wgpu::Buffer, wgpu::Buffer, wgpu::BindGroup);
pub(super) struct SpatialCache {
    storage: PersistentStorage<SpatialNode>,
    pub layout: wgpu::BindGroupLayout,
    settings: wgpu::Buffer,
    pub binding: wgpu::BindGroup,
    pub quads: wgpu::RenderPipeline,
    pub shadows: wgpu::RenderPipeline,
    pub enabled: bool,
    scale: f32,
    capacity: u32,
    index: super::spatial_index::GraphIndex,
}
impl SpatialCache {
    pub fn new(renderer: &WgpuHeadlessRenderer) -> Self {
        let device = &renderer.context.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("retained_spatial"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(96),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(16),
                    },
                    count: None,
                },
            ],
        });
        let settings = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("retained_spatial_settings"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let storage = PersistentStorage::new(renderer, "retained_spatial_nodes");
        let binding = Self::binding(device, &layout, &storage.buffer, &settings);
        // The pinned shared math is reused verbatim. Only the transport loads
        // and flat world origins differ; fragment shaders never walk the tree.
        let mut source = STORAGE_BUFFER_SHADERS.to_owned();
        for (old, new) in [
            ("fn load_quad(", "fn load_quad_raw("),
            ("fn load_shadow(", "fn load_shadow_raw("),
            (
                "    @location(5) @interpolate(flat) background_color1: vec4<f32>,",
                "    @location(5) @interpolate(flat) background_color1: vec4<f32>,\n    @location(6) @interpolate(flat) retained_origin: vec2<f32>,\n    @location(7) @interpolate(flat) retained_clip: vec4<f32>,",
            ),
            (
                "    out.quad_id = instance_id;",
                "    out.quad_id = instance_id;\n    out.retained_origin = quad.bounds.origin;\n    out.retained_clip = vec4(quad.content_mask.origin, quad.content_mask.size);",
            ),
            (
                "    let quad = load_quad(input.quad_id);",
                "    var quad = load_quad_raw(input.quad_id);\n    quad.bounds.origin = input.retained_origin;\n    quad.content_mask = Bounds(input.retained_clip.xy, input.retained_clip.zw);",
            ),
            (
                "    @location(1) @interpolate(flat) shadow_id: u32,",
                "    @location(1) @interpolate(flat) shadow_id: u32,\n    @location(4) @interpolate(flat) retained_origins: vec4<f32>,\n    @location(5) @interpolate(flat) retained_clip: vec4<f32>,",
            ),
            (
                "    out.shadow_id = instance_id;",
                "    out.shadow_id = instance_id;\n    out.retained_origins = vec4(shadow.bounds.origin, shadow.element_bounds.origin);\n    out.retained_clip = vec4(shadow.content_mask.origin, shadow.content_mask.size);",
            ),
            (
                "    let shadow = load_shadow(input.shadow_id);",
                "    var shadow = load_shadow_raw(input.shadow_id);\n    shadow.bounds.origin = input.retained_origins.xy;\n    shadow.element_bounds.origin = input.retained_origins.zw;\n    shadow.content_mask = Bounds(input.retained_clip.xy, input.retained_clip.zw);",
            ),
        ] {
            assert_eq!(
                source.matches(old).count(),
                1,
                "pinned spatial shader contract changed: {old}"
            );
            source = source.replace(old, new);
        }
        source.push_str(include_str!("spatial_resolve.wgsl"));
        source.push_str(include_str!("spatial.wgsl"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("retained_spatial_primitives"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("retained_spatial_primitives"),
            bind_group_layouts: &[
                Some(&renderer.core.resources.bind_group_layouts.globals),
                Some(&renderer.core.resources.bind_group_layouts.instances),
                Some(&layout),
            ],
            immediate_size: 0,
        });
        let pipeline = |vertex, fragment| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("retained_spatial_primitive"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: renderer.core.target_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            storage,
            layout,
            settings,
            binding,
            quads: pipeline("vs_quad", "fs_quad"),
            shadows: pipeline("vs_shadow", "fs_shadow"),
            enabled: false,
            scale: 1.0,
            capacity: 0,
            index: Default::default(),
        }
    }
    fn binding(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        nodes: &wgpu::Buffer,
        settings: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("retained_spatial"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: settings.as_entire_binding(),
                },
            ],
        })
    }
    pub fn clear_index(&mut self) { self.index = Default::default(); }
    pub fn preflight(&mut self, renderer: &WgpuHeadlessRenderer, batch: &SpatialBatch, indexed: bool)
        -> Result<Option<super::spatial_index::GraphPlan>> {
        if !indexed { self.validate(renderer, batch)?; return Ok(None); }
        anyhow::ensure!(batch.scale.is_finite() && batch.scale > 0.0, "invalid spatial scale");
        PersistentStorage::<SpatialNode>::validate(renderer, batch.capacity, &batch.writes)?;
        if !batch.enabled {
            anyhow::ensure!(batch.capacity == 0 && batch.writes.is_empty(), "disabled spatial writes");
            return Ok(None);
        }
        let plan = self.index.prepare(batch.capacity, batch.scale, true,
            batch.writes.iter().flat_map(|w| w.values.iter().enumerate().map(move |(i,v)| (w.start+i as u32,*v))),
            |slot| self.storage.value(slot)).map_err(anyhow::Error::msg)?;
        if plan.dense {
            // Raw records/ranges/scale already passed above. Validate the exact
            // final prefix, including unreferenced and re-exposed mirror slots.
            let mut projected: Vec<_> = (0..batch.capacity)
                .map(|index| self.storage.value(index)).collect();
            for write in &batch.writes {
                let start = write.start as usize;
                let end = (start + write.values.len()).min(projected.len());
                if start < end {
                    projected[start..end].copy_from_slice(&write.values[..end - start]);
                }
            }
            super::spatial_index::validate_projected(&projected, batch.scale)
                .map_err(anyhow::Error::msg)?;
        }
        Ok(Some(plan))
    }
    pub fn commit_index(&mut self, plan: Option<super::spatial_index::GraphPlan>) {
        if let Some(plan) = plan { self.index.commit(plan); }
        else { self.clear_index(); }
    }
    pub fn validate(&self, renderer: &WgpuHeadlessRenderer, batch: &SpatialBatch) -> Result<()> {
        anyhow::ensure!(
            batch.scale.is_finite() && batch.scale > 0.0,
            "invalid spatial scale"
        );
        PersistentStorage::<SpatialNode>::validate(renderer, batch.capacity, &batch.writes)?;
        if !batch.enabled {
            anyhow::ensure!(
                batch.capacity == 0 && batch.writes.is_empty(),
                "disabled spatial writes"
            );
            return Ok(());
        }
        // Full validation already visits the entire addressable prefix. Build
        // its projected mirror once in linear time instead of allocating an
        // ordered map per upload and doing tree lookups on every ancestor.
        // Apply ranges in transport order: overlapping writes remain last-win.
        let mut projected: Vec<_> = (0..batch.capacity)
            .map(|index| self.storage.value(index))
            .collect();
        for write in &batch.writes {
            let start = write.start as usize;
            let end = (start + write.values.len()).min(projected.len());
            if start < end {
                projected[start..end].copy_from_slice(&write.values[..end - start]);
            }
        }
        let get = |index: u32| projected[index as usize];
        for index in 0..batch.capacity {
            let node = get(index);
            if node.links[2] & 2 != 0 {
                anyhow::ensure!(
                    node.links[1] == 0 && node.links[0] == u32::MAX,
                    "invalid spatial anchor"
                );
            } else {
                anyhow::ensure!(
                    node.links[0] < batch.capacity
                        && node.links[1] == get(node.links[0]).links[1] + 1,
                    "cyclic or invalid spatial parent/depth"
                );
            }
        }
        for index in 0..batch.capacity {
            let mut path = SmallVec::<[u32; 32]>::new();
            let mut cursor = index;
            loop {
                let node = get(cursor);
                path.push(cursor);
                if node.links[2] & 2 != 0 {
                    break;
                }
                cursor = node.links[0];
            }
            let anchor = get(path.pop().unwrap());
            let mut origin = [anchor.anchor_origin[0], anchor.anchor_origin[1]];
            let mut scroll = [anchor.scroll[0], anchor.scroll[1]];
            let mut clip = (anchor.links[2] & 8 != 0).then_some(anchor.anchor_clip);
            for slot in path.iter().rev() {
                let node = get(*slot);
                for axis in 0..2 {
                    origin[axis] = (origin[axis] - scroll[axis]) + node.layout_translation[axis];
                    origin[axis] += node.layout_translation[axis + 2];
                }
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
                scroll = [node.scroll[0], node.scroll[1]];
                anyhow::ensure!(
                    origin.iter().all(|v| v.is_finite()),
                    "spatial origin overflow"
                );
            }
            anyhow::ensure!(
                origin.iter().all(|v| (*v * batch.scale).is_finite()),
                "scaled spatial origin overflow"
            );
            if let Some(clip) = clip {
                anyhow::ensure!(
                    [
                        clip[0],
                        clip[1],
                        clip[2],
                        clip[3],
                        clip[0] + clip[2],
                        clip[1] + clip[3]
                    ]
                    .iter()
                    .all(|v| (*v * batch.scale).is_finite()),
                    "spatial clip overflow"
                );
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        renderer: &WgpuHeadlessRenderer,
        batch: SpatialBatch,
    ) -> Result<storage::StorageStats> {
        let generation = self.storage.generation;
        let mut stats = self
            .storage
            .apply(renderer, batch.capacity, batch.writes, false)?;
        self.capacity = batch.capacity;
        if generation != self.storage.generation {
            self.binding = Self::binding(
                &renderer.context.device,
                &self.layout,
                &self.storage.buffer,
                &self.settings,
            );
        }
        if self.scale != batch.scale || self.enabled != batch.enabled {
            renderer.context.queue.write_buffer(
                &self.settings,
                0,
                bytemuck::cast_slice(&[batch.scale, 0.0, 0.0, 0.0]),
            );
            self.scale = batch.scale;
            self.enabled = batch.enabled;
            stats.bytes += 16;
        }
        Ok(stats)
    }
    pub fn extent_changed(&self, batch: &SpatialBatch) -> bool {
        self.capacity != batch.capacity || self.parameters_change(batch)
    }
    pub fn projected_state(&self, slot: u32, batch: &SpatialBatch) -> Result<super::spatial_index::Resolved> {
        let mut path=SmallVec::<[SpatialNode;32]>::new(); let mut cursor=slot;
        let mut state=loop {
            anyhow::ensure!(cursor<batch.capacity && path.len()<32,"invalid content ancestry");
            let node=self.storage.projected(cursor,&batch.writes);
            if node.links[2]&2!=0 {break super::spatial_index::resolve_node(node,None);}
            path.push(node);cursor=node.links[0];
        };
        for node in path.into_iter().rev() {state=super::spatial_index::resolve_node(node,Some(state));}
        Ok(state)
    }
    pub fn projected_affected(&self, property:u32, batch:&SpatialBatch)->bool {
        if property==0 {return false;} let mut slot=property-1;
        for _ in 0..32 {
            if batch.writes.iter().any(|w| slot>=w.start && ((slot-w.start) as usize)<w.values.len()) {return true;}
            let node=self.storage.projected(slot,&batch.writes);
            if node.links[2]&2!=0 {break;} slot=node.links[0];
        }
        false
    }
    pub fn parameters_change(&self, batch: &SpatialBatch) -> bool {
        self.enabled != batch.enabled || self.scale != batch.scale
    }
    pub fn generation(&self) -> u64 {
        self.storage.generation
    }
    pub fn physical_scale(&self) -> f32 { self.scale }
    pub fn current_placement(&self, slot:u32) -> Option<([f32;2], Option<Bounds<ScaledPixels>>)> {
        let batch = SpatialBatch {enabled:self.enabled,capacity:self.capacity,scale:self.scale,writes:vec![]};
        let state = self.projected_state(slot, &batch).ok()?;
        Some(([state.origin[0]*self.scale,state.origin[1]*self.scale],state.clip.map(|clip| Bounds {
            origin:gpui::point(ScaledPixels(clip[0]*self.scale),ScaledPixels(clip[1]*self.scale)),
            size:gpui::size(ScaledPixels(clip[2]*self.scale),ScaledPixels(clip[3]*self.scale)),
        })))
    }
    pub fn affected(&self, property: u32, dirty: &[Range<u32>], all: bool) -> bool {
        if property == 0 {
            return false;
        }
        if all {
            return true;
        }
        let mut index = property - 1;
        for _ in 0..32 {
            if dirty.iter().any(|r| r.contains(&index)) {
                return true;
            }
            let node = self.storage.value(index);
            if node.links[2] & 2 != 0 {
                break;
            }
            index = node.links[0];
        }
        false
    }
    pub fn lease(&self) -> SpatialLease {
        (
            self.storage.buffer.clone(),
            self.settings.clone(),
            self.binding.clone(),
        )
    }
}
