//! Render construction retains independent, validated scope ownership.
use super::*;
use crate::retained::reactive::{ReactiveEdit, ReactiveEditCommit};
mod responsive_rules;

pub struct LogicalEdit<'a, 'draft, 'tree> {
    pub(super) render: &'a mut ReactiveEdit<'draft, 'tree>,
    pub(super) identity: u64,
    scopes: &'a [Slot],
    pub(super) owners: &'a BTreeMap<NodeId, ScopeId>,
    pub(super) inserted: BTreeMap<NodeId, ScopeId>,
    inserted_order: Vec<NodeId>,
    removals: BTreeSet<NodeId>,
    created_scopes: BTreeMap<ScopeId, Scope>,
    child_lists: BTreeMap<ScopeId, Vec<ScopeId>>,
    removed_scopes: BTreeSet<ScopeId>,
    added_sources: BTreeMap<(ScopeId, u64), Box<dyn ScopedSource>>,
    added_effects: BTreeMap<(ScopeId, u64), EffectId>,
    free_scopes: Option<Vec<usize>>,
    available_scopes: &'a [usize],
    scope_slots: usize,
    next_scope_generation: &'a mut u64,
    capabilities: BackendCapabilities,
    pub(super) frame_environment: Option<(LayoutConstraints, [f32; 2])>,
    automatic_environment: Option<responsive::LayoutEnvironment>,
    constraint_inputs: Vec<super::constraint_builder::Observation>,
    responsive_inputs: Vec<responsive::structural::Observation>,
    live_responsive_rules: &'a BTreeMap<(ScopeId, u64), responsive::LayoutBinding>,
    responsive_rules: Option<BTreeMap<(ScopeId, u64), responsive::LayoutBinding>>,
    layout_build_running: bool,
    pub(super) layout_build_failed: bool,
    layout_build_observations: Vec<super::build_plan::BuildObservation>,
    pub(super) live_layout_producers: &'a BTreeMap<(ScopeId, u64), super::producers::Registration>,
    pub(super) layout_producers: Option<BTreeMap<(ScopeId, u64), super::producers::Registration>>,
    pub(super) next_layout_producer: &'a mut u64,
    pub(super) producer_reads: Option<std::cell::RefCell<reactive::SourceObservation>>,
    pub(super) producer_active: Option<LayoutProducerId>,
    pub(super) producer_completed: BTreeSet<LayoutProducerId>,
    pub(super) producer_scheduling: bool,
    pub(super) producer_stats: LayoutProducerStats,
    pub(super) live_producer_revision: Option<u64>,
    pub(super) producer_render_revision: Option<u64>,
    pub(super) live_producer_index: &'a super::producers::ProducerIndex,
    pub(super) live_producer_session: Option<(LayoutConstraints, [f32; 2])>,
    pub(super) producer_plan_skipped: bool,
}
impl LogicalEdit<'_, '_, '_> {
    pub(super) fn producer_logical_unchanged(&self) -> bool {
        self.created_scopes.is_empty()
            && self.removed_scopes.is_empty()
            && self.child_lists.iter().all(|(id, children)| {
                self.scope(*id)
                    .is_ok_and(|scope| scope.children == *children)
            })
    }
    pub(super) fn has_retired_producer_scopes(&self) -> bool {
        !self.removed_scopes.is_empty()
    }
    pub(super) fn producer_rules_unchanged(&self) -> bool {
        self.responsive_rules.is_none()
    }
    /// Schedule once-only keyed construction from current render ancestry and
    /// explicit non-ancestral prerequisites. Failure poisons this outer edit;
    /// catching it cannot publish a partially executed plan.
    pub fn build_layout_plan(
        &mut self,
        plan: LayoutBuildPlan<'_>,
    ) -> Result<LayoutBuildStats, Error> {
        if self.layout_build_running || self.layout_build_failed || self.frame_environment.is_none()
        {
            self.layout_build_failed = true;
            return Err(Error::InvalidLayoutBuildPlan);
        }
        self.layout_build_running = true;
        let result = self.run_layout_build_plan(plan);
        self.layout_build_running = false;
        self.layout_build_failed |= result.is_err();
        result
    }
    pub(super) fn build_plan_preview(&mut self) -> Result<UiTree, Error> {
        self.settle_automatic_rules()?;
        self.render
            .preview(self.capabilities, &self.retired_outputs()?)
    }
    pub(super) fn observe_layout_build_order(
        &mut self,
        observation: super::build_plan::BuildObservation,
    ) {
        self.layout_build_observations.push(observation);
    }
    /// Capture host inputs in this atomic frame edit and enable automatic
    /// settlement of scope-owned rules before builders and final publication.
    /// Ordinary source reads still see the current draft; use
    /// `responsive_registered` for an explicit selection/read checkpoint.
    pub fn layout_environment(
        &mut self,
        environment: &responsive::LayoutEnvironment,
    ) -> Result<(), Error> {
        if self.frame_environment.is_none() {
            return Err(Error::UnsafeContainerQuery);
        }
        environment.validate()?;
        self.automatic_environment = Some(environment.clone());
        Ok(())
    }

    /// Stop automatic settlement after this edit. Already observed builder or
    /// responsive inputs still require final validation. Last styles remain.
    pub fn clear_layout_environment(&mut self) {
        self.automatic_environment = None;
    }

    /// Settle viewport/container rules against the current keyed/source draft.
    /// Rules must already be registered. Call within `edit_frame` or
    /// `edit_local_frame`, after prerequisite structure and before dependent
    /// builders. This call restores source values/counters if evaluation fails;
    /// final publication rechecks every observed width, writer and proof.
    /// Returned work excludes builder previews and final frame production.
    pub fn responsive(
        &mut self,
        rules: &responsive::LayoutResponsiveBindings,
        environment: &responsive::LayoutEnvironment,
    ) -> Result<responsive::ResponsiveEditStats, Error> {
        let (constraints, viewport) = self.frame_environment.ok_or(Error::UnsafeContainerQuery)?;
        let removed = self.retired_outputs()?;
        let (stats, observation) = self.render.responsive_plan(|render| {
            rules.resolve_edit(
                render,
                environment,
                constraints,
                viewport,
                self.capabilities,
                &removed,
            )
        })?;
        self.responsive_inputs.push(observation);
        Ok(stats)
    }

    /// Reads the final child-slot input of a size-contained constraint box,
    /// before choosing/building that child's keyed structure. Call within
    /// `edit_frame` or `edit_local_frame`. Nested calls observe earlier staged
    /// sources and structure.
    /// The callback must be side-effect free apart from edits to this draft.
    pub fn constraint_builder<R>(
        &mut self,
        host: NodeId,
        build: impl FnOnce(ConstraintBuilderInput, &mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.settle_automatic_rules()?;
        let preview = self.constraint_preview(host, false)?;
        let input = super::constraint_builder::input(&preview, host)?;
        drop(preview);
        self.constraint_inputs
            .push(super::constraint_builder::Observation::Full(host, input));
        build(input, self)
    }

    /// Observe the host's incoming border-box allocation before selecting its
    /// descendants. Unlike constraint_builder, this needs no size containment
    /// and allows content-derived size in both axes. Independent parent edges
    /// must reach the external root or a full Size containment boundary under
    /// ordinary Flex. Size-contained ConstraintBox outgoing slots also qualify.
    /// Ordinary Flex/Grid also qualify along a proven content-independent
    /// two-axis allocation chain. Grid needs definite independent tracks on
    /// both axes. Intrinsic/automatic feedback edges reject.
    /// Prerequisite child lists/algorithms must precede observation.
    pub fn allocation_builder<R>(
        &mut self,
        host: NodeId,
        build: impl FnOnce(ConstraintBuilderInput, &mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.settle_automatic_rules()?;
        let (constraints, viewport) = self
            .frame_environment
            .ok_or(Error::UnsafeConstraintBuilder)?;
        let removed = self.retired_outputs()?;
        let mut preview = self.render.preview(self.capabilities, &removed)?;
        preview.prepare_layout_environment(constraints, viewport)?;
        let dependencies =
            super::constraint_builder::AllocationDependencies::capture(&preview, host)?;
        if dependencies.tracks_input() {
            preview
                .layout
                .set_track_layout_input(preview.node(host)?.layout, true)?;
        }
        preview.compute_layout(constraints)?;
        let input = dependencies.input(&preview, host)?;
        drop(preview);
        self.constraint_inputs
            .push(super::constraint_builder::Observation::Allocation(
                host,
                input,
                dependencies,
            ));
        build(input, self)
    }

    /// Reads only the inline assignment. InlineSize containment and a proven
    /// column/Tight-box chain or exact fixed-width CSS boundary permit
    /// content-derived block height.
    /// Final validation rechecks both the proof and observed inline values.
    pub fn inline_constraint_builder<R>(
        &mut self,
        host: NodeId,
        build: impl FnOnce(InlineConstraintBuilderInput, &mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.settle_automatic_rules()?;
        let preview = self.constraint_preview(host, true)?;
        let input = super::constraint_builder::inline_input(&preview, host)?;
        let dependencies = layout_dependencies::InlineDependencies::capture(&preview, host)?;
        drop(preview);
        self.constraint_inputs
            .push(super::constraint_builder::Observation::Inline(
                host,
                input,
                dependencies,
            ));
        build(input, self)
    }

    fn constraint_preview(&mut self, host: NodeId, inline: bool) -> Result<UiTree, Error> {
        let (constraints, viewport) = self
            .frame_environment
            .ok_or(Error::UnsafeConstraintBuilder)?;
        let removed = self.retired_outputs()?;
        let mut preview = self.render.preview(self.capabilities, &removed)?;
        if inline {
            super::constraint_builder::validate_inline_host(&preview, host)?;
        } else {
            super::constraint_builder::validate_host(&preview, host)?;
        }
        preview.prepare_layout_environment(constraints, viewport)?;
        preview.compute_layout(constraints)?;
        Ok(preview)
    }

    /// Observe the incoming inline assignment of an ordinary content-sized
    /// host, without containment. A complete Column/proven-Row/Tight-box/width-phase
    /// chain must keep descendant block-size changes out of inline allocation.
    /// No final used height or block opportunity is observable.
    pub fn inline_allocation_builder<R>(
        &mut self,
        host: NodeId,
        build: impl FnOnce(InlineConstraintBuilderInput, &mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.settle_automatic_rules()?;
        let (constraints, viewport) = self
            .frame_environment
            .ok_or(Error::UnsafeConstraintBuilder)?;
        let removed = self.retired_outputs()?;
        let mut preview = self.render.preview(self.capabilities, &removed)?;
        super::constraint_builder::validate_inline_allocation_host(&preview, host)?;
        preview
            .layout
            .set_track_layout_input(preview.node(host)?.layout, true)?;
        preview.prepare_layout_environment(constraints, viewport)?;
        preview.compute_layout(constraints)?;
        let input = super::constraint_builder::inline_allocation_input(&preview, host)?;
        let dependencies = layout_dependencies::InlineDependencies::capture(&preview, host)?;
        self.constraint_inputs
            .push(super::constraint_builder::Observation::InlineAllocation(
                host,
                input,
                dependencies,
            ));
        drop(preview);
        build(input, self)
    }

    pub(super) fn retired_scopes(&self) -> Result<BTreeSet<ScopeId>, Error> {
        let mut dead = BTreeSet::new();
        let mut stack: Vec<_> = self.removed_scopes.iter().copied().collect();
        while let Some(id) = stack.pop() {
            if dead.insert(id) {
                stack.extend(self.children(id)?);
            }
        }
        Ok(dead)
    }

    fn retired_outputs(&self) -> Result<Vec<NodeId>, Error> {
        let dead = self.retired_scopes()?;
        Ok(self
            .owners
            .iter()
            .chain(self.inserted.iter())
            .filter(|(_, owner)| dead.contains(owner))
            .map(|(node, _)| *node)
            .collect())
    }

    /// Surviving output of a scope in append order. Use it with keyed state to
    /// retain render handles rather than appending replacement nodes on every build.
    pub fn renders(&mut self, scope: ScopeId) -> Result<Vec<NodeId>, Error> {
        self.scope(scope)?;
        let dead = self.render.tree().removed_ids()?;
        Ok(self
            .scope(scope)?
            .renders
            .iter()
            .copied()
            .chain(
                self.inserted_order
                    .iter()
                    .copied()
                    .filter(|node| self.inserted[node] == scope),
            )
            .filter(|node| !dead.contains(node))
            .collect())
    }

    pub(super) fn scope(&self, id: ScopeId) -> Result<&Scope, Error> {
        if id.owner != self.identity {
            return Err(Error::InvalidScope);
        }
        self.created_scopes
            .get(&id)
            .or_else(|| {
                self.scopes
                    .get(id.slot)
                    .filter(|s| s.generation == id.generation)
                    .and_then(|s| s.scope.as_ref())
            })
            .ok_or(Error::InvalidScope)
    }
    pub fn children(&self, id: ScopeId) -> Result<&[ScopeId], Error> {
        Ok(self
            .child_lists
            .get(&id)
            .unwrap_or(&self.scope(id)?.children))
    }
    pub fn reconcile_keys(&mut self, parent: ScopeId, keys: &[u64]) -> Result<Vec<ScopeId>, Error> {
        self.scope(parent)?;
        if self.removed_scopes.contains(&parent) {
            return Err(Error::InvalidScope);
        }
        let unique: BTreeSet<_> = keys.iter().copied().collect();
        if unique.len() != keys.len() {
            return Err(Error::DuplicateKey);
        }
        let old: BTreeMap<_, _> = self
            .children(parent)?
            .iter()
            .map(|id| (self.scope(*id).unwrap().key, *id))
            .collect();
        self.next_scope_generation
            .checked_add(keys.iter().filter(|key| !old.contains_key(key)).count() as u64)
            .ok_or(Error::RevisionExhausted)?;
        let mut order = vec![];
        for key in keys {
            if let Some(id) = old.get(key) {
                order.push(*id);
                continue;
            }
            let generation = *self.next_scope_generation;
            *self.next_scope_generation =
                generation.checked_add(1).ok_or(Error::RevisionExhausted)?;
            let slot = self
                .free_scopes
                .get_or_insert_with(|| self.available_scopes.to_vec())
                .pop()
                .unwrap_or_else(|| {
                    let slot = self.scope_slots;
                    self.scope_slots += 1;
                    slot
                });
            let id = ScopeId {
                owner: self.identity,
                slot,
                generation,
            };
            self.created_scopes.insert(
                id,
                Scope {
                    parent: Some(parent),
                    key: *key,
                    children: vec![],
                    sources: BTreeMap::new(),
                    effects: BTreeMap::new(),
                    renders: vec![],
                },
            );
            order.push(id);
        }
        for (key, id) in old {
            if !unique.contains(&key) {
                self.removed_scopes.insert(id);
            }
        }
        self.child_lists.insert(parent, order.clone());
        Ok(order)
    }
    pub fn state<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        init: impl FnOnce() -> T,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self
            .added_sources
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.sources.get(&key))
        {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.render.validate_scoped_source(signal, None)?;
            self.read(signal)?;
            return Ok(signal);
        }
        self.scope(scope)?;
        let signal = self.render.signal(init())?;
        self.read(signal)?;
        self.added_sources.insert((scope, key), Box::new(signal));
        Ok(signal)
    }
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self
            .added_sources
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.sources.get(&key))
        {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.render
                .validate_scoped_source(signal, Some(dependencies))?;
            self.read(signal)?;
            return Ok(signal);
        }
        self.scope(scope)?;
        let signal = self.render.computed(dependencies, evaluate)?;
        self.read(signal)?;
        self.added_sources.insert((scope, key), Box::new(signal));
        Ok(signal)
    }
    pub fn effect(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        if let Some(id) = self
            .added_effects
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.effects.get(&key))
        {
            self.render.validate_scoped_effect(*id, dependencies)?;
            return Ok(*id);
        }
        self.scope(scope)?;
        let id = self.render.effect(dependencies, observe)?;
        self.added_effects.insert((scope, key), id);
        Ok(id)
    }
    pub fn append(
        &mut self,
        owner: ScopeId,
        parent: NodeId,
        style: Style,
        color: u32,
    ) -> Result<NodeId, Error> {
        self.scope(owner)?;
        let node = self.render.tree().append(parent, style, color)?;
        self.inserted.insert(node, owner);
        self.inserted_order.push(node);
        Ok(node)
    }
    /// Construct output with the framework-owned layout protocol, including
    /// single-child constraint boundaries, under the same logical draft.
    pub fn append_layout(
        &mut self,
        owner: ScopeId,
        parent: NodeId,
        style: LayoutStyle,
        color: u32,
    ) -> Result<NodeId, Error> {
        self.scope(owner)?;
        let node = self.render.tree().append_layout(parent, style, color)?;
        self.inserted.insert(node, owner);
        self.inserted_order.push(node);
        Ok(node)
    }
    pub fn reparent(&mut self, node: NodeId, host: NodeId) -> Result<(), Error> {
        self.render.tree().reparent(node, host)
    }
    /// Authorize removal for this output's owner. Foreign-owned descendants
    /// must first be moved out, or explicitly removed in this same edit.
    pub fn remove(&mut self, node: NodeId) -> Result<(), Error> {
        self.inserted
            .get(&node)
            .or_else(|| self.owners.get(&node))
            .ok_or(Error::InvalidNode)?;
        self.removals.insert(node);
        self.render.tree().remove(node)
    }
    pub fn set_children(&mut self, node: NodeId, children: Vec<NodeId>) -> Result<(), Error> {
        self.render.tree().set_children(node, children)
    }
    pub fn properties(&mut self) -> &mut Transaction {
        self.render.tree().properties()
    }
    /// Project final output to device pixels without changing layout/query or
    /// text measurement. Published frame and policy share this atomic edit.
    pub fn pixel_snap(&mut self, policy: PixelSnap) -> Result<(), Error> {
        self.render.tree().pixel_snap(policy)
    }
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        let value = self.render.read(signal)?;
        if let Some(reads) = &self.producer_reads {
            self.render.record_source(&mut reads.borrow_mut(), signal)?;
        }
        Ok(value)
    }
    pub fn bind<T: 'static>(
        &mut self,
        signal: Signal<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.render.bind(signal, property)
    }
    pub fn unbind(&mut self, binding: BindingId) -> Result<(), Error> {
        self.render.unbind(binding)
    }
}
impl LogicalUi {
    /// Atomically edit keyed scope lifetimes, sources, effects, bindings and
    /// render output. All user validation completes before callbacks run.
    pub fn edit<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        self.edit_validated(
            sources,
            render,
            None,
            None::<fn(&mut UiTree) -> Result<(), Error>>,
        )
    }

    /// Keyed construction and final layout/extraction publish together. Effects
    /// run only after the logical maps and complete owned frame are installed.
    pub fn edit_frame<S, R>(
        &mut self,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<(ReactiveEditCommit<R>, FramePacket), Error> {
        constraints.validate()?;
        let mut frame = None;
        let commit = self.edit_validated(
            sources,
            render,
            Some((constraints, viewport)),
            Some(|tree: &mut UiTree| {
                let candidate = tree.frame_in_viewport(constraints, viewport)?;
                // Finite authored values can still overflow when composed. Reject
                // before publishing state; backend validation happens much later.
                for item in candidate.items.iter() {
                    spatial::validate_geometry(
                        item.bounds,
                        item.content_bounds,
                        item.clip,
                        &item.spatial,
                    )?;
                }
                frame = Some(candidate);
                Ok(())
            }),
        )?;
        Ok((commit, frame.expect("validated complete frame")))
    }

    /// Local-placement counterpart to `edit_frame`, with the same atomic
    /// sources/keyed lifecycle/layout publication and constraint builders.
    /// Property/material-only edits retain sparse local snapshots; validation
    /// checks composed geometry without resolving a full FramePacket.
    pub fn edit_local_frame<S, R>(
        &mut self,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<(ReactiveEditCommit<R>, local::LocalFrame), Error> {
        constraints.validate()?;
        let mut frame = None;
        let commit = self.edit_validated(
            sources,
            render,
            Some((constraints, viewport)),
            Some(|tree: &mut UiTree| {
                let candidate = tree.local_frame_in_viewport(constraints, viewport)?;
                candidate.validate_geometry()?;
                frame = Some(candidate);
                Ok(())
            }),
        )?;
        Ok((commit, frame.expect("validated complete local frame")))
    }

    fn edit_validated<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
        frame_environment: Option<(LayoutConstraints, [f32; 2])>,
        validate: Option<impl FnOnce(&mut UiTree) -> Result<(), Error>>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        let frame_environment = frame_environment
            .or_else(|| {
                self.layout_session
                    .as_ref()
                    .map(|s| (s.constraints, s.viewport))
            })
            .or(self.producer_session);
        let mut layout_session = self.layout_session.clone();
        let mut inserted = BTreeMap::new();
        let mut inserted_order = vec![];
        let mut created_scopes = BTreeMap::new();
        let mut child_lists = BTreeMap::new();
        let mut removed_scopes = BTreeSet::new();
        let mut added_sources = BTreeMap::new();
        let mut added_effects = BTreeMap::new();
        let mut responsive_rules = None;
        let mut layout_producers = None;
        let mut producer_stats = LayoutProducerStats::default();
        let constraint_inputs: std::cell::RefCell<Vec<super::constraint_builder::Observation>> =
            std::cell::RefCell::new(vec![]);
        let responsive_inputs: std::cell::RefCell<Vec<responsive::structural::FinalObservation>> =
            std::cell::RefCell::new(vec![]);
        let build_observations: std::cell::RefCell<Vec<super::build_plan::BuildObservation>> =
            std::cell::RefCell::new(vec![]);
        let capabilities = self.ui.capabilities();
        let pending = self.ui.edit_deferred_validated(
            sources,
            |value, render_draft| {
                let mut draft = LogicalEdit {
                    render: render_draft,
                    identity: self.identity,
                    scopes: &self.scopes,
                    owners: &self.render_owners,
                    inserted: BTreeMap::new(),
                    inserted_order: vec![],
                    removals: BTreeSet::new(),
                    created_scopes: BTreeMap::new(),
                    child_lists: BTreeMap::new(),
                    removed_scopes: BTreeSet::new(),
                    added_sources: BTreeMap::new(),
                    added_effects: BTreeMap::new(),
                    free_scopes: None,
                    available_scopes: &self.free,
                    scope_slots: self.scopes.len(),
                    next_scope_generation: &mut self.next_scope_generation,
                    capabilities,
                    frame_environment,
                    automatic_environment: self
                        .layout_session
                        .as_ref()
                        .map(|s| s.environment.clone()),
                    constraint_inputs: vec![],
                    responsive_inputs: vec![],
                    live_responsive_rules: &self.responsive_rules,
                    responsive_rules: None,
                    layout_build_running: false,
                    layout_build_failed: false,
                    layout_build_observations: vec![],
                    live_layout_producers: &self.layout_producers,
                    layout_producers: None,
                    next_layout_producer: &mut self.next_layout_producer,
                    producer_reads: None,
                    producer_active: None,
                    producer_completed: BTreeSet::new(),
                    producer_scheduling: false,
                    producer_stats: LayoutProducerStats::default(),
                    live_producer_revision: self.producer_revision,
                    producer_render_revision: None,
                    live_producer_index: &self.producer_index,
                    live_producer_session: self.producer_session,
                    producer_plan_skipped: false,
                };
                let value = render(value, &mut draft)?;
                draft.settle_layout_producers()?;
                if draft.layout_build_failed {
                    return Err(Error::InvalidLayoutBuildPlan);
                }
                draft.retire_responsive_rules()?;
                let mut dead = BTreeSet::new();
                let mut stack: Vec<_> = draft.removed_scopes.iter().copied().collect();
                while let Some(id) = stack.pop() {
                    if dead.insert(id) {
                        stack.extend(draft.children(id)?);
                    }
                }
                // Retire whole logical subtrees as a set. Dependency validation is
                // deferred to the reactive preflight, before cleanup or publication.
                for id in &dead {
                    let dependencies: Vec<_> = draft
                        .scope(*id)?
                        .sources
                        .values()
                        .map(|s| s.dependency())
                        .chain(
                            draft
                                .added_sources
                                .range((*id, 0)..=(*id, u64::MAX))
                                .map(|(_, s)| s.dependency()),
                        )
                        .collect();
                    let effects: Vec<_> = draft
                        .scope(*id)?
                        .effects
                        .values()
                        .copied()
                        .chain(
                            draft
                                .added_effects
                                .range((*id, 0)..=(*id, u64::MAX))
                                .map(|(_, e)| *e),
                        )
                        .collect();
                    for effect in effects {
                        draft.render.dispose_effect(effect)?;
                    }
                    for dependency in dependencies {
                        draft.render.dispose_dependency(dependency)?;
                    }
                }
                let output: Vec<_> = draft
                    .owners
                    .iter()
                    .chain(draft.inserted.iter())
                    .filter(|(_, owner)| dead.contains(owner))
                    .map(|(node, _)| *node)
                    .collect();
                draft
                    .render
                    .retire_output_bindings(&output.iter().copied().collect());
                for node in output {
                    draft.removals.insert(node);
                    draft.render.tree().remove(node)?;
                }
                draft.render.tree().removed_ids()?; // settle pending property child lists
                for root in &draft.removals {
                    let owner = draft
                        .inserted
                        .get(root)
                        .or_else(|| draft.owners.get(root))
                        .unwrap();
                    let mut visited = BTreeSet::new();
                    let mut stack = vec![*root];
                    while let Some(node) = stack.pop() {
                        if !visited.insert(node) {
                            continue;
                        }
                        if node != *root && draft.removals.contains(&node) {
                            continue;
                        }
                        if draft
                            .inserted
                            .get(&node)
                            .or_else(|| draft.owners.get(&node))
                            != Some(owner)
                        {
                            return Err(Error::ScopeInUse);
                        }
                        stack.extend(draft.render.tree().children(node)?);
                    }
                }
                // Key changes sort all final surviving sibling outputs by logical
                // preorder, retaining same-owner insertion order and portal hosts.
                if !draft.child_lists.is_empty() {
                    let mut ranks = BTreeMap::new();
                    let mut stack = vec![self.root];
                    while let Some(id) = stack.pop() {
                        if dead.contains(&id) {
                            continue;
                        }
                        ranks.insert(id, ranks.len());
                        stack.extend(draft.children(id)?.iter().rev());
                    }
                    let dead_nodes = draft.render.tree().removed_ids()?;
                    let parents: Vec<_> = draft
                        .owners
                        .keys()
                        .chain(draft.inserted.keys())
                        .copied()
                        .filter(|id| !dead_nodes.contains(id))
                        .collect();
                    for parent in parents {
                        let mut children: Vec<_> = draft
                            .render
                            .tree()
                            .children(parent)?
                            .iter()
                            .filter(|id| !dead_nodes.contains(id))
                            .copied()
                            .collect();
                        let before = children.clone();
                        children.sort_by_key(|node| {
                            ranks[draft
                                .inserted
                                .get(node)
                                .or_else(|| draft.owners.get(node))
                                .unwrap()]
                        });
                        if children != before {
                            draft.render.tree().set_children(parent, children)?;
                        }
                    }
                }
                draft.validate_responsive_rules()?;
                draft.settle_automatic_rules()?;
                draft.validate_producer_sources()?;
                layout_session = draft.automatic_environment.take().map(|environment| {
                    let (constraints, viewport) = frame_environment.expect("captured frame input");
                    LayoutSession {
                        environment,
                        constraints,
                        viewport,
                    }
                });
                responsive_rules = draft.responsive_rules.take();
                layout_producers = draft.layout_producers.take();
                producer_stats = draft.producer_stats;
                *responsive_inputs.borrow_mut() = draft
                    .responsive_inputs
                    .into_iter()
                    .map(|observation| observation.finish(draft.render))
                    .collect::<Result<Vec<_>, _>>()?;
                inserted = draft.inserted;
                inserted_order = draft.inserted_order;
                created_scopes = draft.created_scopes;
                child_lists = draft.child_lists;
                removed_scopes = dead;
                added_sources = draft.added_sources;
                added_effects = draft.added_effects;
                *constraint_inputs.borrow_mut() = draft.constraint_inputs;
                *build_observations.borrow_mut() = draft.layout_build_observations;
                Ok(value)
            },
            (validate.is_some() || frame_environment.is_some()).then_some({
                |tree: &mut UiTree| {
                    for input in constraint_inputs.borrow().iter() {
                        let tracked = match input {
                            super::constraint_builder::Observation::InlineAllocation(
                                host,
                                _,
                                _,
                            ) => Some(*host),
                            super::constraint_builder::Observation::Allocation(
                                host,
                                _,
                                dependencies,
                            ) if dependencies.tracks_input() => Some(*host),
                            _ => None,
                        };
                        if let Some(host) = tracked {
                            let layout = tree.node(host)?.layout;
                            tree.layout.set_track_layout_input(layout, true)?;
                            tree.layout_dirty |= tree.layout.dirty(layout)?;
                            // Even a structurally unchanged builder must run
                            // the observation pass; cached frame output alone
                            // cannot populate a newly opted-in input record.
                            tree.dirty |= tree.layout_dirty;
                        }
                    }
                    if let Some(validate) = validate {
                        validate(tree)?;
                    } else if let Some((constraints, viewport)) = frame_environment {
                        // Ordinary edits in an active session publish complete
                        // local output before effects, without resolving a packet.
                        tree.local_frame_in_viewport(constraints, viewport)?
                            .validate_geometry()?;
                    }
                    for input in constraint_inputs.borrow().iter() {
                        input.validate(tree)?;
                    }
                    for input in responsive_inputs.borrow().iter() {
                        input.validate(tree)?;
                    }
                    for observation in build_observations.borrow().iter() {
                        observation.validate(tree)?;
                    }
                    tree.layout.clear_tracked_layout_inputs();
                    Ok(())
                }
            }),
        )?;
        let result = &pending.output;
        let dead: BTreeSet<_> = result.removed.iter().copied().collect();
        let owners: BTreeSet<_> = result
            .removed
            .iter()
            .filter_map(|n| self.render_owners.remove(n))
            .collect();
        for owner in owners {
            self.scope_mut(owner)
                .renders
                .retain(|node| !dead.contains(node));
        }
        for id in &removed_scopes {
            if created_scopes.remove(id).is_none() {
                let slot = &mut self.scopes[id.slot];
                slot.scope = None;
                if let Some(next) = slot.generation.checked_add(1) {
                    slot.generation = next;
                    self.free.push(id.slot);
                }
            }
        }
        for (id, scope) in created_scopes {
            if id.slot >= self.scopes.len() {
                for slot in self.scopes.len()..id.slot {
                    self.free.push(slot);
                }
                self.scopes.resize_with(id.slot + 1, || Slot {
                    generation: 0,
                    scope: None,
                });
            }
            self.free.retain(|slot| *slot != id.slot);
            self.scopes[id.slot] = Slot {
                generation: id.generation,
                scope: Some(scope),
            };
        }
        for (id, children) in child_lists {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).children = children;
            }
        }
        for ((id, key), source) in added_sources {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).sources.insert(key, source);
            }
        }
        for ((id, key), effect) in added_effects {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).effects.insert(key, effect);
            }
        }
        if let Some(rules) = responsive_rules {
            self.responsive_rules = rules;
        }
        self.layout_session = layout_session;
        if let Some(producers) = layout_producers {
            self.layout_producers = producers;
            self.producer_index = super::producers::ProducerIndex::new(&self.layout_producers);
        }
        self.producer_session = if self.layout_producers.is_empty() {
            None
        } else {
            Some(frame_environment.expect("producer frame session"))
        };
        self.producer_stats = producer_stats;
        self.producer_revision = self.producer_session.map(|_| self.ui.tree().revision);
        let live_inserted: BTreeSet<_> = result.inserted.iter().copied().collect();
        for node in inserted_order {
            if live_inserted.contains(&node) {
                let owner = inserted[&node];
                self.scope_mut(owner).renders.push(node);
                self.render_owners.insert(node, owner);
            }
        }
        Ok(self.ui.finish_edit(pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portal_ownership_survives_edits_and_foreign_descendants_preflight_before_sources() {
        let mut ui = LogicalUi::new(Style::default(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 1)
            .unwrap();
        let portal = ui.append_render(scope, host, Style::default(), 2).unwrap();
        let source = ui.state(scope, 0, || 2u32).unwrap();
        ui.bind(source, Property::color(portal)).unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        assert!(matches!(
            ui.edit(
                |tx| {
                    tx.set(source, 3)?;
                    Ok(())
                },
                |(), edit| edit.remove(host)
            ),
            Err(Error::ScopeInUse)
        ));
        assert_eq!(*ui.read(source).unwrap(), 2);
        assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
        let root_node = ui.root_render();
        let result = ui
            .edit(
                |tx| {
                    tx.set(source, 3)?;
                    Ok(())
                },
                |(), edit| {
                    let host2 = edit.append(root, root_node, Style::default(), 4)?;
                    edit.reparent(portal, host2)?;
                    edit.remove(host)?;
                    let extra = edit.append(scope, host2, Style::default(), 0)?;
                    edit.bind(source, Property::color(extra))?;
                    Ok(extra)
                },
            )
            .unwrap();
        assert_eq!(result.result.commit.revision, before.revision + 1);
        assert_eq!(ui.renders(scope).unwrap(), [portal, result.result.value]);
        assert!(!ui.renders(root).unwrap().contains(&host));
        ui.reconcile_keys(root, &[]).unwrap();
        assert!(ui.read(source).is_err());
        assert!(ui.reactive().tree().node(portal).is_err());
        assert!(ui.reactive().tree().node(result.result.value).is_err());
    }
    #[test]
    fn explicit_foreign_removal_cannot_authorize_an_unrelated_portal() {
        let mut ui = LogicalUi::new(Style::default(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let foreign = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap();
        let portal = ui
            .append_render(foreign, host, Style::default(), 1)
            .unwrap();
        let other = ui
            .append_render(foreign, ui.root_render(), Style::default(), 2)
            .unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        assert!(matches!(
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.remove(other)?;
                    edit.remove(host)
                }
            ),
            Err(Error::ScopeInUse)
        ));
        assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
        let result = ui
            .edit(
                |_| Ok(()),
                |(), edit| {
                    edit.remove(portal)?;
                    edit.remove(host)
                },
            )
            .unwrap();
        assert_eq!(result.result.commit.revision, before.revision + 1);
        assert_eq!(ui.renders(foreign).unwrap(), [other]);
    }
}

#[cfg(test)]
mod lifecycle_tests;
