//! Owner-local responsive planning. No intermediate source, frame or effect is
//! published. Preview layout is disposable; the final candidate validates and
//! renders before it replaces the live tree.
use super::*;

// This synchronous return is consumed immediately. Boxing the resolved packet
// would add an allocation to every resolved responsive frame, including idle.
#[allow(clippy::large_enum_variant)]
pub(in crate::retained) enum Output {
    Resolved(FramePacket),
    Local(local::LocalFrame),
}

pub(in crate::retained) struct Publication {
    pub output: Output,
    pub changed_styles: usize,
    pub layout_runs: u64,
    pub layout_stats: LayoutStats,
}

pub(in crate::retained) struct Draft<'a> {
    transaction: ReactiveTransaction<'a>,
    preview: Option<UiTree>,
    constraints: LayoutConstraints,
    viewport: [f32; 2],
    has_queries: bool,
    layout_runs: u64,
    layout_stats: LayoutStats,
}

fn add_stats(total: &mut LayoutStats, next: LayoutStats) {
    total.leaf_layouts += next.leaf_layouts;
    total.measure_calls += next.measure_calls;
    total.layout_writes += next.layout_writes;
    total.changed_layout_writes += next.changed_layout_writes;
    total.net_changed_nodes += next.net_changed_nodes;
}

impl Draft<'_> {
    pub fn layout_targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error> {
        self.transaction.runtime.responsive_layout_targets(source)
    }
    pub fn validate_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        binding: BindingId,
    ) -> Result<(), Error> {
        self.transaction
            .runtime
            .validate_responsive_writer(source, target, binding)
    }
    pub fn tree(&self) -> &UiTree {
        self.preview
            .as_ref()
            .unwrap_or(&self.transaction.runtime.tree)
    }

    pub fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error> {
        let original = self.transaction.runtime.source(source.id)?;
        self.transaction
            .values
            .get(&source.id)
            .unwrap_or(&original.value)
            .as_any()
            .downcast_ref()
            .ok_or(Error::InvalidSignal)
    }

    pub fn read<T: 'static>(&mut self, source: Signal<T>) -> Result<&T, Error> {
        self.transaction.read(source)
    }

    pub fn apply(&mut self, values: Vec<(Signal<LayoutStyle>, LayoutStyle)>) -> Result<(), Error> {
        let previous = self.has_queries.then(|| self.transaction.values.clone());
        for (source, value) in values {
            if self.transaction.read(source)? != &value {
                self.transaction.set(source, value)?;
            }
        }
        self.transaction.settle()?;
        let Some(previous) = previous else {
            return Ok(());
        };
        // Include values restored to their original source value, which are
        // absent from the final draft map but must undo earlier preview sinks.
        let ids: BTreeSet<_> = previous
            .keys()
            .chain(self.transaction.values.keys())
            .copied()
            .collect();
        let runtime = self.transaction.runtime;
        let mut properties = Transaction::default();
        for id in ids {
            let source = runtime.source(id)?;
            let before = previous.get(&id).unwrap_or(&source.value);
            let after = self.transaction.values.get(&id).unwrap_or(&source.value);
            if before.equal(after.as_ref()) {
                continue;
            }
            for binding in &source.bindings {
                (runtime.bindings[binding].write)(after.as_ref(), &mut properties);
            }
        }
        if !properties.patches.is_empty() {
            self.preview
                .get_or_insert_with(|| runtime.tree.clone_for_layout_plan())
                .commit(properties, runtime.capabilities)?;
        }
        Ok(())
    }

    pub fn inline_size(&mut self, node: NodeId) -> Result<f32, Error> {
        let runtime = self.transaction.runtime;
        let tree = self.preview.as_ref().unwrap_or(&runtime.tree);
        if tree.layout_dirty || tree.layout_constraints != Some(self.constraints) {
            let tree = self
                .preview
                .get_or_insert_with(|| runtime.tree.clone_for_layout_plan());
            tree.prepare_layout_environment(self.constraints, self.viewport)?;
            let stats = tree.compute_layout(self.constraints)?;
            // Keep observations against the original published output; preview
            // must not publish geometry or clear the accumulated write journal.
            tree.layout_dirty = false;
            self.layout_runs += 1;
            add_stats(&mut self.layout_stats, stats);
        }
        let tree = self.preview.as_ref().unwrap_or(&runtime.tree);
        let layout = tree.layout.layout(tree.node(node)?.layout)?;
        let width = (layout.size.width
            - layout.padding.left
            - layout.padding.right
            - layout.border.left
            - layout.border.right
            - layout.scrollbar_size.width)
            .max(0.0);
        if !width.is_finite() {
            return Err(Error::InvalidFrame);
        }
        Ok(width)
    }
}

impl ReactiveUi {
    #[cfg(test)]
    pub(in crate::retained) fn responsive_test_tree(&mut self) -> &mut UiTree {
        &mut self.tree
    }

    pub(in crate::retained) fn validate_responsive_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        id: BindingId,
    ) -> Result<(), Error> {
        self.source(source.id)?;
        if !self.bindings.get(&id).is_some_and(|binding| {
            binding.source == source.id && binding.node == target && binding.field == Field::Style
        }) {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }

    pub(in crate::retained) fn responsive_layout_targets(
        &self,
        source: Signal<LayoutStyle>,
    ) -> Result<Vec<NodeId>, Error> {
        if self.source(source.id)?.computation.is_some() {
            return Err(Error::ReadonlySignal);
        }
        let mut visited = BTreeSet::new();
        let mut pending = vec![source.id];
        let mut targets = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let source = self.source(id)?;
            pending.extend(&source.dependents);
            for binding in &source.bindings {
                let binding = &self.bindings[binding];
                let layout = match binding.field {
                    Field::Style | Field::Measure | Field::ParentLayout | Field::Image => true,
                    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                    Field::Text => true,
                    _ => false,
                };
                if layout {
                    targets.insert(binding.node);
                }
            }
        }
        Ok(targets.into_iter().collect())
    }

    pub(in crate::retained) fn responsive_output(
        &mut self,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
        local: bool,
        has_queries: bool,
        plan: impl FnOnce(&mut Draft<'_>) -> Result<usize, Error>,
    ) -> Result<Publication, Error> {
        constraints.validate()?;
        if viewport
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(Error::InvalidViewport);
        }
        let mut draft = Draft {
            transaction: ReactiveTransaction {
                runtime: self,
                values: BTreeMap::new(),
                writes: 0,
                equal: 0,
                dirty_roots: BTreeMap::new(),
                computed_evaluations: 0,
                computed_equal: 0,
            },
            preview: None,
            constraints,
            viewport,
            has_queries,
            layout_runs: 0,
            layout_stats: LayoutStats::default(),
        };
        let changed_styles = plan(&mut draft)?;
        let Draft {
            transaction,
            preview,
            mut layout_runs,
            mut layout_stats,
            ..
        } = draft;
        let publish_sources = transaction.writes != 0;
        let prepared = transaction.into_prepared()?;
        let (properties, sink_writes, effects) = self.collect_sinks(&prepared, |_| true);
        // Previews are layout-only and never form a FramePacket predecessor.
        // Release their owned tree before making the final candidate. Replaying
        // final sinks once also discards any intermediate writes that restored
        // their initial value, yielding one final source/render transaction.
        drop(preview);
        let needs_candidate = !properties.patches.is_empty()
            || self.tree.dirty && (!local || !self.tree.can_reuse_local_frame())
            || self.tree.layout_dirty
            || self.tree.layout_constraints != Some(constraints)
            || self.tree.viewport != Some(viewport);
        let mut candidate = needs_candidate.then(|| self.tree.clone_for_layout_plan());
        let tree = candidate.as_mut().unwrap_or(&mut self.tree);
        let commit = tree.commit(properties, self.capabilities)?;
        let publication_commit = Commit {
            revision: if tree.layout_constraints != Some(constraints)
                || tree.viewport != Some(viewport)
            {
                tree.revision
                    .checked_add(1)
                    .ok_or(Error::RevisionExhausted)?
            } else {
                tree.revision
            },
            request_frame: commit.request_frame
                || tree.layout_constraints != Some(constraints)
                || tree.viewport != Some(viewport),
            ..commit
        };
        let final_layout_expected =
            tree.layout_dirty || tree.layout_constraints != Some(constraints);
        let output = if local {
            Output::Local(tree.local_frame_in_viewport(constraints, viewport)?)
        } else {
            Output::Resolved(tree.frame_in_viewport(constraints, viewport)?)
        };
        // A local sparse publication performs no layout. A complete fallback
        // leaves its resolved frame in the tree; its revision proves it belongs
        // to this output rather than a retained older resolved baseline.
        let final_stats = match &output {
            Output::Resolved(frame) => Some((frame.layout_runs, frame.layout_stats)),
            Output::Local(frame) => tree
                .frame
                .as_ref()
                .filter(|resolved| final_layout_expected && resolved.revision == frame.revision())
                .map(|frame| (frame.layout_runs, frame.layout_stats)),
        };
        if let Some((runs, stats)) = final_stats {
            layout_runs += runs;
            add_stats(&mut layout_stats, stats);
        }
        if let Some(candidate) = candidate {
            self.tree = candidate;
        }
        if publish_sources {
            self.publish_update((), prepared, publication_commit, sink_writes, effects);
        }
        Ok(Publication {
            output,
            changed_styles,
            layout_runs,
            layout_stats,
        })
    }
}

impl UiTree {
    /// Private identity-preserving draft; it can only replace this owner after
    /// successful validation. Exposing Clone would allow two writers with the
    /// same tree identity/generation namespace, so no public Clone is provided.
    pub(in crate::retained) fn clone_for_layout_plan(&self) -> Self {
        Self {
            identity: self.identity,
            slots: self.slots.clone(),
            free: self.free.clone(),
            layout: self.layout.clone(),
            root: self.root,
            revision: self.revision,
            next_generation: self.next_generation,
            dirty: self.dirty,
            layout_dirty: self.layout_dirty,
            full_extract_required: self.full_extract_required,
            layout_nodes: self.layout_nodes.clone(),
            layout_written: self.layout_written.clone(),
            out_of_flow: self.out_of_flow.clone(),
            custom_parents: self.custom_parents.clone(),
            geometry_dirty: self.geometry_dirty.clone(),
            property_dirty: self.property_dirty.clone(),
            local_dirty: self.local_dirty.clone(),
            local_material_dirty: self.local_material_dirty.clone(),
            paint_subtree_ends: self.paint_subtree_ends.clone(),
            paint_dirty: self.paint_dirty.clone(),
            paint_indices: self.paint_indices.clone(),
            frame_build_policy: self.frame_build_policy,
            viewport: self.viewport,
            layout_constraints: self.layout_constraints,
            frame: self.frame.clone(),
            local_frame: self.local_frame.clone(),
        }
    }
}
