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
                    && frame.follows(old)
            })
            .filter(|old| frame.material_updates().iter().all(|&index| {
                composition::same_topology(old.material(index), frame.material(index))
            }))
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
        // Private provenance proves stable topology and identifies independent
        // material/placement changes. Changed material is validated here;
        // projected spatial and primitive records still use GPU preflight.
        self.failed = true;
        let resources = frame.resource_frame();
        validation::validate_header(&resources, surface, scale)?;
        let old = previous.as_ref().unwrap();
        // Consume geometry while its stack-owned item is live. Only resource
        // changes need to retain owned items for the resource batch builders.
        let mut image_items = vec![];
        let mut glyph_items = vec![];
        let mut prepared_quads = 0;
        let mut prepared_shadows = 0;
        let last_serial = self.backend.last_serial();
        let mut indices = frame.material_updates().iter().copied().peekable();
        let mut prepared: Result<(), Error> = Ok(());
        while let Some(start) = indices.next() {
            let mut end = start + 1;
            while indices.peek() == Some(&end) { indices.next(); end += 1; }
            // Adjacent changed records share one open ancestry traversal;
            // sparse gaps do not force resolving unaffected descendants.
            frame.visit_geometry(start..end, |index, item| {
                if prepared.is_err() { return; }
                prepared = (|| {
                    validation::validate_item(item, scale)?;
                    let entry = routing::classify(old.material(index), item, true, false);
                    let property = self.spatial_arena.slot(item.node)?;
                    self.prepare_primitives(item, property, scale, last_serial, entry.quad, entry.shadow)?;
                    prepared_quads += usize::from(entry.quad);
                    prepared_shadows += usize::from(entry.shadow);
                    let (image, glyph) = routing::classify_content(old.material(index), item, true);
                    if image { image_items.push(item.clone()); }
                    if glyph { glyph_items.push(item.clone()); }
                    Ok(())
                })();
            });
        }
        prepared?;
        let selected_images = image_items.iter().collect::<Vec<_>>();
        let selected_glyphs = glyph_items.iter().collect::<Vec<_>>();
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
                writes: self.arena.writes().into_iter().map(|write| QuadWrite {
                    start: write.start, values: write.values.into_iter().map(Record::quad).collect(),
                }).collect(),
                order: None,
                layers,
                images: image_updates(&resources, scale, false, Some(&self.spatial_arena), Some(&selected_images))?,
                glyphs: glyph_updates(&resources, scale, false, Some(&self.spatial_arena), Some(&selected_glyphs))?,
                shadows: gpui_wgpu::ShadowBatch {
                    capacity: self.shadow_arena.capacity(),
                    writes: self.shadow_arena.writes().into_iter().map(|write| gpui_wgpu::ShadowWrite {
                        start: write.start, values: write.values.into_iter().map(ShadowRecord::shadow).collect(),
                    }).collect(),
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
        self.arena.acknowledge();
        self.shadow_arena.acknowledge();
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
                inspected_items: updates.len() + frame.material_updates().len(),
                prepared_quad_items: prepared_quads,
                prepared_shadow_items: prepared_shadows,
                prepared_spatial_items: updates.len(),
                ..Default::default()
            },
            full_resync: false,
            gpu,
            validation: ValidationStats {
                validated_items: frame.material_updates().len(),
                ..Default::default()
            },
            composition: CompositionStats {bounds_items,bounds_translated_nodes,..Default::default()},
        }))
    }
}
