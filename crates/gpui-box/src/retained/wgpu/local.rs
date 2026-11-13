//! Local snapshot consumer. Spatial transitions reuse immutable
//! payload/order/resources. Unsupported consumers/effects resolve explicitly.
use super::*;

impl WgpuFrameRenderer {
    pub fn submit_local_for_window(
        &mut self,
        window: &gpui::Window,
        frame: &local::LocalFrame,
        surface: [u32; 2],
        scale: f32,
    ) -> Result<Option<FrameSubmission>, Error> {
        #[cfg(target_os = "windows")]
        self.ensure_window_device(window)?;
        #[cfg(not(target_os = "windows"))]
        let _ = window;
        self.submit_local(frame, surface, scale)
    }
    pub fn submit_local(
        &mut self,
        frame: &local::LocalFrame,
        surface: [u32; 2],
        scale: f32,
    ) -> Result<Option<FrameSubmission>, Error> {
        if self.failed {
            return Err(Error::Gpu("reset after failed frame submission".into()));
        }
        if self.retired_sources.contains(&frame.tree())
            || self
                .latest
                .is_some_and(|(tree, revision)| tree == frame.tree() && revision > frame.revision())
        {
            return Err(Error::StaleFrame);
        }
        self.collect()?;
        let previous = self.local_snapshot.clone();
        if previous.as_ref().is_some_and(|old| old.same(frame))
            && self.scale == scale
            && self.surface == surface
            && self.source == Some((frame.tree(), frame.revision()))
        {
            return Ok(None);
        }
        let updates = previous
            .as_ref()
            .filter(|old| {
                self.shared_spatial
                    && self.shared_content
                    && self.scale == scale
                    && self.surface == surface
                    && self.source == Some((old.tree(), old.revision()))
            })
            .and_then(|old| frame.spatial_updates(old));
        let Some(updates) = updates else {
            let resolved = frame.resolve();
            if self.latest == Some((frame.tree(), frame.revision()))
                && self
                    .routing_snapshot
                    .as_ref()
                    .is_some_and(|old| !old.equivalent_scene(&resolved))
            {
                return Err(Error::InvalidFrame);
            }
            // Local and resolved routes may alternate at the same revision.
            // Local layer bounds need fresh setup rather than an equal
            // resolved-packet shortcut.
            self.source = None;
            self.routing_snapshot = None;
            let tight=self.tight_layer_bounds;
            self.tight_layer_bounds &= self.local_tight_layer_bounds;
            let result = self.submit(&resolved, surface, scale);
            self.tight_layer_bounds=tight;
            let mut result = result?;
            self.local_ink = if self.local_tight_layer_bounds && tight && !resolved.properties.effects.is_empty() {
                let (ink,count)=local_ink::LocalInk::rebuild(frame);
                if let Some(result)=&mut result {result.composition.bounds_items+=count;}
                Some(ink)
            } else {None};
            self.local_snapshot = Some(frame.clone());
            return Ok(result);
        };
        // Private immutable provenance proves only placement changed. Exact
        // resolved anchors remain validated by the existing GPU preflight.
        self.failed = true;
        for item in &updates {
            self.spatial_arena.update_local(item)?;
        }
        let mut layers=self.composition_cache.local_spatial_layers();
        let mut bounds_items=0;
        let mut bounds_translated_nodes=0;
        if self.local_tight_layer_bounds && self.tight_layer_bounds && !layers.is_empty() {
            let ink=self.local_ink.as_mut().ok_or(Error::InvalidFrame)?;
            let stats=ink.update_motion(frame,previous.as_ref().unwrap(),self.local_ink_translation);
            bounds_items=stats.0;
            bounds_translated_nodes=stats.1;
            self.composition_cache.bound_local_layers(&mut layers,frame,ink,surface,scale);
        }
        let gpu = self
            .backend
            .submit(QuadBatch {
                capacity: self.arena.capacity(),
                writes: vec![],
                order: None,
                layers,
                images: ImageUpdates::default(),
                glyphs: gpui_wgpu::GlyphUpdates::default(),
                shadows: gpui_wgpu::ShadowBatch {
                    capacity: self.shadow_arena.capacity(),
                    writes: vec![],
                },
                spatial: self.spatial_arena.batch(true, scale),
                cache_layers: self.cache_layers,
                size: size(
                    DevicePixels(surface[0] as i32),
                    DevicePixels(surface[1] as i32),
                ),
                redraw: QuadRedraw::Full,
            })
            .map_err(gpu_error)?;
        self.spatial_arena.acknowledge();
        self.source = Some((frame.tree(), frame.revision()));
        self.latest = self.source;
        self.routing_snapshot = None;
        self.validation_cache = validation::ValidationCache::default();
        self.local_snapshot = Some(frame.clone());
        self.local_leases.push_back((gpu.serial, frame.clone()));
        self.collect()?;
        self.failed = false;
        Ok(Some(FrameSubmission {
            routing: RoutingStats {
                cached: true,
                inspected_items: updates.len(),
                prepared_spatial_items: updates.len(),
                ..Default::default()
            },
            full_resync: false,
            gpu,
            validation: ValidationStats::default(),
            composition: CompositionStats {bounds_items,bounds_translated_nodes,..Default::default()},
        }))
    }
}
