//! Source settlement and render construction share one validation/publication.
use super::*;

#[derive(Clone)]
pub(super) struct PreparedUpdate {
    values: BTreeMap<SourceId, Rc<dyn Value>>,
    writes: u64,
    equal: u64,
    computed_evaluations: u64,
    computed_equal: u64,
}

impl ReactiveTransaction<'_> {
    pub(super) fn into_prepared(mut self) -> Result<PreparedUpdate, Error> {
        self.settle()?;
        Ok(PreparedUpdate {
            values: self.values,
            writes: self.writes,
            equal: self.equal,
            computed_evaluations: self.computed_evaluations,
            computed_equal: self.computed_equal,
        })
    }
}

pub struct ReactiveEditCommit<R> {
    pub result: ReactiveCommit<R>,
    pub inserted: Vec<NodeId>,
    pub removed: Vec<NodeId>,
}
pub(in crate::retained) struct PendingReactiveEdit<R> {
    pub(in crate::retained) output: ReactiveEditCommit<R>,
    effects: BTreeSet<EffectId>,
    cleanups: Vec<EffectCleanup>,
}

/// Reads see the settled source draft. New bindings synchronize with those
/// values and publish only after the complete render draft validates.
pub struct ReactiveEdit<'a, 'tree> {
    tree: &'a mut TreeEdit<'tree>,
    identity: u64,
    sources: &'a [SourceSlot],
    prepared: &'a mut PreparedUpdate,
    endpoints: &'a BTreeMap<(NodeId, Field), BindingId>,
    next_binding: &'a mut u64,
    bindings: Vec<(BindingId, Binding)>,
    bound_endpoints: BTreeSet<(NodeId, Field)>,
    live_bindings: &'a HashMap<BindingId, Binding>,
    unbound: BTreeSet<BindingId>,
    created: BTreeMap<SourceId, Source>,
    free_sources: Option<Vec<usize>>,
    source_slots: usize,
    available_sources: &'a [usize],
    next_source_generation: &'a mut u64,
    removed_sources: BTreeSet<SourceId>,
    effects: BTreeMap<EffectId, effects::Effect>,
    live_effects: &'a BTreeMap<EffectId, effects::Effect>,
    removed_effects: BTreeSet<EffectId>,
    next_effect: &'a mut u64,
}
impl<'tree> ReactiveEdit<'_, 'tree> {
    pub(in crate::retained) fn responsive_plan<R>(
        &mut self,
        plan: impl FnOnce(&mut Self) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let previous = self.prepared.clone();
        match plan(self) {
            Ok(value) => Ok(value),
            Err(error) => {
                *self.prepared = previous;
                Err(error)
            }
        }
    }

    pub(in crate::retained) fn responsive_values(
        &mut self,
        values: Vec<(Signal<LayoutStyle>, LayoutStyle)>,
    ) -> Result<(), Error> {
        let mut next = self.prepared.clone();
        let mut changed = BTreeSet::new();
        for (signal, value) in values {
            let source = self.source(signal.id)?;
            if self.removed_sources.contains(&signal.id) {
                return Err(Error::InvalidSignal);
            }
            if source.computation.is_some() {
                return Err(Error::ReadonlySignal);
            }
            next.writes += 1;
            let value: Rc<dyn Value> = Rc::new(value);
            if next
                .values
                .get(&signal.id)
                .unwrap_or(&source.value)
                .equal(value.as_ref())
            {
                next.equal += 1;
                continue;
            }
            changed.insert(signal.id);
            if source.value.equal(value.as_ref()) {
                next.values.remove(&signal.id);
            } else {
                next.values.insert(signal.id, value);
            }
        }
        // Draft-created computations have no live reverse edges until commit.
        // Rank order handles both live and newly constructed dependencies.
        if changed.is_empty() {
            *self.prepared = next;
            return Ok(());
        }
        for (_, id) in self.derived_sources() {
            let source = self.source(id)?;
            let computation = source.computation.as_ref().unwrap();
            if !computation
                .dependencies
                .iter()
                .any(|id| changed.contains(id))
            {
                continue;
            }
            let value = (computation.evaluate)(&ComputedInputs {
                context: self.context(),
                dependencies: &computation.dependencies,
                values: &next.values,
            })?;
            next.computed_evaluations += 1;
            if next
                .values
                .get(&id)
                .unwrap_or(&source.value)
                .equal(value.as_ref())
            {
                next.computed_equal += 1;
                continue;
            }
            changed.insert(id);
            if source.value.equal(value.as_ref()) {
                next.values.remove(&id);
            } else {
                next.values.insert(id, value);
            }
        }
        *self.prepared = next;
        Ok(())
    }

    pub(in crate::retained) fn responsive_layout_targets(
        &self,
        signal: Signal<LayoutStyle>,
    ) -> Result<Vec<NodeId>, Error> {
        let source = self.source(signal.id)?;
        if self.removed_sources.contains(&signal.id) {
            return Err(Error::InvalidSignal);
        }
        if source.computation.is_some() {
            return Err(Error::ReadonlySignal);
        }
        let mut reachable = BTreeSet::from([signal.id]);
        for (_, id) in self.derived_sources() {
            if self
                .source(id)?
                .computation
                .as_ref()
                .unwrap()
                .dependencies
                .iter()
                .any(|id| reachable.contains(id))
            {
                reachable.insert(id);
            }
        }
        let targets = self
            .live_bindings
            .iter()
            .filter(|(id, _)| !self.unbound.contains(id))
            .chain(self.bindings.iter().map(|(id, binding)| (id, binding)))
            .filter(|(_, binding)| {
                reachable.contains(&binding.source)
                    && match binding.field {
                        Field::Style
                        | Field::Measure
                        | Field::ParentLayout
                        | Field::ParentData
                        | Field::Image => true,
                        #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                        Field::Text => true,
                        _ => false,
                    }
            })
            .map(|(_, binding)| binding.node)
            .collect::<BTreeSet<_>>();
        Ok(targets.into_iter().collect())
    }

    fn derived_sources(&self) -> Vec<(u64, SourceId)> {
        let mut derived =
            self.sources
                .iter()
                .enumerate()
                .filter_map(|(slot, entry)| {
                    entry.source.as_ref().and_then(|source| {
                        source.computation.as_ref().map(|_| {
                            (
                                source.rank,
                                SourceId {
                                    owner: self.identity,
                                    slot,
                                    generation: entry.generation,
                                },
                            )
                        })
                    })
                })
                .chain(self.created.iter().filter_map(|(id, source)| {
                    source.computation.as_ref().map(|_| (source.rank, *id))
                }))
                .filter(|(_, id)| !self.removed_sources.contains(id))
                .collect::<Vec<_>>();
        derived.sort_unstable();
        derived
    }

    pub(in crate::retained) fn validate_responsive_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        id: BindingId,
    ) -> Result<(), Error> {
        self.read(source)?;
        let binding = self
            .bindings
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, b)| b)
            .or_else(|| self.live_bindings.get(&id))
            .ok_or(Error::InvalidBinding)?;
        if self.unbound.contains(&id)
            || self.removed_sources.contains(&source.id)
            || binding.source != source.id
            || binding.node != target
            || binding.field != Field::Style
        {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }

    pub(in crate::retained) fn preview(
        &self,
        capabilities: BackendCapabilities,
        removed: &[NodeId],
    ) -> Result<UiTree, Error> {
        let mut tx = Transaction::default();
        for (_, binding) in &self.bindings {
            if !self.removed_sources.contains(&binding.source) {
                let value = self
                    .prepared
                    .values
                    .get(&binding.source)
                    .unwrap_or(&self.source(binding.source)?.value);
                (binding.write)(value.as_ref(), &mut tx);
            }
        }
        for (id, binding) in self.live_bindings {
            if !self.unbound.contains(id)
                && !self.removed_sources.contains(&binding.source)
                && let Some(value) = self.prepared.values.get(&binding.source)
            {
                (binding.write)(value.as_ref(), &mut tx);
            }
        }
        self.tree.preview(capabilities, removed, tx)
    }

    pub fn tree(&mut self) -> &mut TreeEdit<'tree> {
        self.tree
    }
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        let source = self.source(signal.id)?;
        self.prepared
            .values
            .get(&signal.id)
            .unwrap_or(&source.value)
            .as_any()
            .downcast_ref()
            .ok_or(Error::InvalidSignal)
    }
    fn source(&self, id: SourceId) -> Result<&Source, Error> {
        if let Some(source) = self.created.get(&id) {
            return Ok(source);
        }
        if id.owner != self.identity {
            return Err(Error::InvalidSignal);
        }
        self.sources
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.source.as_ref())
            .ok_or(Error::InvalidSignal)
    }
    pub fn bind<T: 'static>(
        &mut self,
        signal: Signal<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.tree.children(property.node)?;
        if self
            .endpoints
            .get(&(property.node, property.field))
            .is_some_and(|id| {
                !self.unbound.contains(id)
                    && !self
                        .removed_sources
                        .contains(&self.live_bindings[id].source)
            })
            || self
                .bound_endpoints
                .contains(&(property.node, property.field))
        {
            return Err(Error::BindingConflict);
        }
        self.read(signal)?;
        // Reserve monotonically, even on failure: an escaped provisional binding
        // ID must never identify an unrelated endpoint from a subsequent edit.
        *self.next_binding = self
            .next_binding
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let id = BindingId {
            owner: self.identity,
            serial: *self.next_binding,
        };
        let binding = Binding {
            source: signal.id,
            node: property.node,
            field: property.field,
            write: Box::new(move |value, tx| {
                (property.write)(
                    property.node,
                    value
                        .as_any()
                        .downcast_ref::<T>()
                        .expect("typed draft sink"),
                    tx,
                )
            }),
        };
        self.bound_endpoints.insert((binding.node, binding.field));
        self.bindings.push((id, binding));
        Ok(id)
    }
    /// Detach after validation. The current material stays until another write;
    /// a later bind in this draft may claim the same endpoint immediately.
    pub fn unbind(&mut self, id: BindingId) -> Result<(), Error> {
        if let Some(index) = self.bindings.iter().position(|(binding, _)| *binding == id) {
            let (_, binding) = self.bindings.remove(index);
            self.bound_endpoints.remove(&(binding.node, binding.field));
            return Ok(());
        }
        if self.unbound.contains(&id) || !self.live_bindings.contains_key(&id) {
            return Err(Error::InvalidBinding);
        }
        self.unbound.insert(id);
        Ok(())
    }
    fn context(&self) -> SourceContext<'_> {
        SourceContext::Draft {
            identity: self.identity,
            sources: self.sources,
            created: &self.created,
        }
    }
    fn insert_source<T>(&mut self, source: Source) -> Result<Signal<T>, Error> {
        let generation = *self.next_source_generation;
        *self.next_source_generation = generation.checked_add(1).ok_or(Error::RevisionExhausted)?;
        let slot = self
            .free_sources
            .get_or_insert_with(|| self.available_sources.to_vec())
            .pop()
            .unwrap_or_else(|| {
                let slot = self.source_slots;
                self.source_slots += 1;
                slot
            });
        let id = SourceId {
            owner: self.identity,
            slot,
            generation,
        };
        self.created.insert(id, source);
        Ok(Signal {
            id,
            marker: PhantomData,
        })
    }
    pub fn signal<T: PartialEq + 'static>(&mut self, value: T) -> Result<Signal<T>, Error> {
        self.insert_source(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
            dependents: BTreeSet::new(),
            effects: BTreeSet::new(),
            computation: None,
            rank: 0,
            scheduled_epoch: Cell::new(0),
            delivery_lifetime: None,
        })
    }
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        let dependencies: BTreeSet<_> = dependencies.iter().map(|d| d.0).collect();
        let mut rank = 0;
        for id in &dependencies {
            rank = rank.max(
                self.source(*id)?
                    .rank
                    .checked_add(1)
                    .ok_or(Error::RevisionExhausted)?,
            );
        }
        let value = evaluate(&ComputedInputs {
            context: self.context(),
            dependencies: &dependencies,
            values: &self.prepared.values,
        })?;
        self.insert_source(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
            dependents: BTreeSet::new(),
            effects: BTreeSet::new(),
            computation: Some(computed::Computation {
                dependencies,
                evaluate: Box::new(move |i| Ok(Rc::new(evaluate(i)?))),
            }),
            rank,
            scheduled_epoch: Cell::new(0),
            delivery_lifetime: None,
        })
    }
    pub fn dispose_signal<T>(&mut self, source: Signal<T>) -> Result<(), Error> {
        self.source(source.id)?;
        self.removed_sources.insert(source.id);
        Ok(())
    }
    pub(in crate::retained) fn dispose_dependency(
        &mut self,
        source: Dependency,
    ) -> Result<(), Error> {
        self.source(source.0)?;
        self.removed_sources.insert(source.0);
        Ok(())
    }
    pub(in crate::retained) fn retire_output_bindings(&mut self, nodes: &BTreeSet<NodeId>) {
        for (_, binding) in &self.bindings {
            if nodes.contains(&binding.node) {
                self.bound_endpoints.remove(&(binding.node, binding.field));
            }
        }
        self.bindings
            .retain(|(_, binding)| !nodes.contains(&binding.node));
    }
    pub fn effect(
        &mut self,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        let dependencies: BTreeSet<_> = dependencies.iter().map(|d| d.0).collect();
        for id in &dependencies {
            self.source(*id)?;
        }
        *self.next_effect = self
            .next_effect
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let id = EffectId {
            owner: self.identity,
            serial: *self.next_effect,
        };
        self.effects.insert(
            id,
            effects::Effect {
                dependencies,
                observe: Box::new(observe),
                cleanup: None,
            },
        );
        Ok(id)
    }
    pub fn dispose_effect(&mut self, id: EffectId) -> Result<(), Error> {
        if self.effects.remove(&id).is_some() {
            return Ok(());
        }
        if self.removed_effects.contains(&id) || !self.live_effects.contains_key(&id) {
            return Err(Error::InvalidEffect);
        }
        self.removed_effects.insert(id);
        Ok(())
    }
    pub(in crate::retained) fn validate_scoped_source<T: 'static>(
        &self,
        signal: Signal<T>,
        dependencies: Option<&[Dependency]>,
    ) -> Result<(), Error> {
        self.read(signal)?;
        match (&self.source(signal.id)?.computation, dependencies) {
            (None, None) => Ok(()),
            (Some(c), Some(d)) if c.dependencies == d.iter().map(|d| d.0).collect() => Ok(()),
            _ => Err(Error::InvalidSignal),
        }
    }
    pub(in crate::retained) fn validate_scoped_effect(
        &self,
        id: EffectId,
        dependencies: &[Dependency],
    ) -> Result<(), Error> {
        let effect = self
            .effects
            .get(&id)
            .or_else(|| self.live_effects.get(&id))
            .ok_or(Error::InvalidEffect)?;
        if effect.dependencies != dependencies.iter().map(|d| d.0).collect() {
            return Err(Error::InvalidEffect);
        }
        Ok(())
    }
}

impl ReactiveUi {
    pub(super) fn prepare_update<R>(
        &self,
        update: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<R, Error>,
    ) -> Result<(R, PreparedUpdate), Error> {
        let mut tx = ReactiveTransaction {
            runtime: self,
            values: BTreeMap::new(),
            writes: 0,
            equal: 0,
            dirty_roots: BTreeMap::new(),
            computed_evaluations: 0,
            computed_equal: 0,
        };
        let value = update(&mut tx)?;
        Ok((value, tx.into_prepared()?))
    }
    pub(super) fn collect_sinks(
        &self,
        update: &PreparedUpdate,
        include: impl Fn(NodeId) -> bool,
    ) -> (Transaction, usize, BTreeSet<EffectId>) {
        let mut tx = Transaction::default();
        let mut count = 0;
        let mut effects = BTreeSet::new();
        for (id, value) in &update.values {
            let source = self.source(*id).expect("settled source");
            effects.extend(&source.effects);
            for id in &source.bindings {
                let b = &self.bindings[id];
                if include(b.node) {
                    (b.write)(value.as_ref(), &mut tx);
                    count += 1;
                }
            }
        }
        (tx, count, effects)
    }
    pub(super) fn publish_update<R>(
        &mut self,
        value: R,
        prepared: PreparedUpdate,
        commit: Commit,
        sink_writes: usize,
        effects: BTreeSet<EffectId>,
    ) -> ReactiveCommit<R> {
        let PreparedUpdate {
            values,
            writes,
            equal,
            computed_evaluations,
            computed_equal,
        } = prepared;
        let changed_sources = values.len();
        for (id, value) in values {
            self.sources[id.slot].source.as_mut().unwrap().value = value;
        }
        self.stats.transactions += 1;
        self.stats.source_writes += writes;
        self.stats.equal_writes += equal;
        self.stats.changed_sources += changed_sources as u64;
        self.stats.sink_writes += sink_writes as u64;
        self.stats.frame_requests += u64::from(commit.request_frame);
        self.stats.computed_evaluations += computed_evaluations;
        self.stats.computed_equal += computed_equal;
        let effect_cleanups = effects.iter().filter(|id| self.run_effect(**id)).count();
        self.stats.effect_runs += effects.len() as u64;
        self.stats.effect_cleanups += effect_cleanups as u64;
        ReactiveCommit {
            #[cfg(feature = "retained-diagnostics")]
            transaction_phase_times: None,
            value,
            commit,
            changed_sources,
            sink_writes,
            computed_evaluations,
            computed_equal,
            effect_runs: effects.len(),
            effect_cleanups,
        }
    }

    /// Two synchronous phases of one owner event: settle source writes, then
    /// construct/edit output using final reads. Neither phase publishes on error.
    /// Existing sinks into removed output are retired rather than dispatched.
    /// Source/effect construction and disposal join the same preflight.
    pub fn edit<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut ReactiveEdit<'_, '_>) -> Result<R, Error>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        let pending = self.edit_deferred(sources, render)?;
        Ok(self.finish_edit(pending))
    }
    pub(in crate::retained) fn finish_edit<R>(
        &mut self,
        mut pending: PendingReactiveEdit<R>,
    ) -> ReactiveEditCommit<R> {
        let mut cleaned = 0;
        for cleanup in pending.cleanups {
            cleanup();
            cleaned += 1;
        }
        for id in &pending.effects {
            cleaned += usize::from(self.run_effect(*id));
        }
        pending.output.result.effect_runs = pending.effects.len();
        pending.output.result.effect_cleanups = cleaned;
        self.stats.effect_runs += pending.effects.len() as u64;
        self.stats.effect_cleanups += cleaned as u64;
        pending.output
    }
    pub(in crate::retained) fn edit_deferred<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut ReactiveEdit<'_, '_>) -> Result<R, Error>,
    ) -> Result<PendingReactiveEdit<R>, Error> {
        self.edit_deferred_validated(
            sources,
            render,
            None::<fn(&mut UiTree) -> Result<(), Error>>,
        )
    }
    pub(in crate::retained) fn edit_deferred_validated<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut ReactiveEdit<'_, '_>) -> Result<R, Error>,
        validate: Option<impl FnOnce(&mut UiTree) -> Result<(), Error>>,
    ) -> Result<PendingReactiveEdit<R>, Error> {
        let (source_result, mut prepared) = self.prepare_update(sources)?;
        let mut effects = BTreeSet::new();
        let mut new_bindings = vec![];
        let mut unbound = BTreeSet::new();
        let mut created = BTreeMap::new();
        let mut removed_sources = BTreeSet::new();
        let mut created_effects = BTreeMap::new();
        let mut removed_effects = BTreeSet::new();
        let mut sink_writes = 0;
        let identity = self.identity;
        let render_tree = |tree: &mut TreeEdit<'_>| {
            let mut edit = ReactiveEdit {
                tree,
                identity,
                sources: &self.sources,
                prepared: &mut prepared,
                endpoints: &self.endpoints,
                next_binding: &mut self.next_binding,
                bindings: vec![],
                bound_endpoints: BTreeSet::new(),
                live_bindings: &self.bindings,
                unbound: BTreeSet::new(),
                created: BTreeMap::new(),
                free_sources: None,
                source_slots: self.sources.len(),
                available_sources: &self.free,
                next_source_generation: &mut self.next_source_generation,
                removed_sources: BTreeSet::new(),
                effects: BTreeMap::new(),
                live_effects: &self.effects,
                removed_effects: BTreeSet::new(),
                next_effect: &mut self.next_effect,
            };
            let value = render(source_result, &mut edit)?;
            let dead = edit.tree.removed_ids()?;
            edit.bindings
                .retain(|(_, binding)| !edit.removed_sources.contains(&binding.source));
            for id in &edit.removed_sources {
                let source = edit.source(*id)?;
                if source
                    .dependents
                    .iter()
                    .any(|d| !edit.removed_sources.contains(d))
                    || source
                        .effects
                        .iter()
                        .any(|e| !edit.removed_effects.contains(e))
                {
                    return Err(Error::SignalInUse);
                }
            }
            for (id, source) in &edit.created {
                if !edit.removed_sources.contains(id)
                    && source.computation.as_ref().is_some_and(|c| {
                        c.dependencies
                            .iter()
                            .any(|d| edit.removed_sources.contains(d))
                    })
                {
                    return Err(Error::SignalInUse);
                }
            }
            for effect in edit.effects.values() {
                if effect
                    .dependencies
                    .iter()
                    .any(|d| edit.removed_sources.contains(d))
                {
                    return Err(Error::SignalInUse);
                }
            }
            for (_, binding) in &edit.bindings {
                if dead.contains(&binding.node) || edit.removed_sources.contains(&binding.source) {
                    return Err(Error::InvalidNode);
                }
                let value = edit
                    .prepared
                    .values
                    .get(&binding.source)
                    .unwrap_or(&edit.source(binding.source)?.value)
                    .clone();
                (binding.write)(value.as_ref(), edit.tree.properties());
            }
            sink_writes = edit.bindings.len();
            // Collect once from final settled values, including responsive stages
            // performed during construction. Restored values emit no transient sink.
            for (source_id, value) in &edit.prepared.values {
                if edit.removed_sources.contains(source_id) {
                    continue;
                }
                if edit.created.contains_key(source_id) {
                    // New sources only have draft bindings/effects, handled
                    // separately. They have no published reverse edges yet.
                    continue;
                }
                let source = edit.sources[source_id.slot]
                    .source
                    .as_ref()
                    .expect("validated live source");
                effects.extend(&source.effects);
                for id in &source.bindings {
                    let binding = &self.bindings[id];
                    if !dead.contains(&binding.node) && !edit.unbound.contains(id) {
                        (binding.write)(value.as_ref(), edit.tree.properties());
                        sink_writes += 1;
                    }
                }
            }
            new_bindings = edit.bindings;
            unbound = edit.unbound;
            created = edit.created;
            removed_sources = edit.removed_sources;
            created_effects = edit.effects;
            removed_effects = edit.removed_effects;
            Ok(value)
        };
        let result = if let Some(validate) = validate {
            self.tree
                .edit_validated(self.capabilities, render_tree, validate)
        } else {
            self.tree.edit(self.capabilities, render_tree)
        }?;
        let mut cleanups = vec![];
        for id in &removed_effects {
            if let Some(cleanup) = self.detach_effect(*id).expect("validated effect disposal") {
                cleanups.push(cleanup);
            }
        }
        for id in unbound {
            self.unbind(id).expect("prevalidated draft unbinding");
        }
        if !result.removed.is_empty() {
            self.retire_dead_bindings();
        }
        let mut dead_sources: Vec<_> = removed_sources
            .iter()
            .filter(|id| !created.contains_key(id))
            .copied()
            .collect();
        dead_sources.sort_by_key(|id| std::cmp::Reverse(self.source(*id).unwrap().rank));
        for id in dead_sources {
            self.dispose_source(id).expect("validated source disposal");
        }
        created.retain(|id, _| !removed_sources.contains(id));
        let dependencies: Vec<_> = created
            .iter()
            .filter_map(|(id, s)| {
                s.computation
                    .as_ref()
                    .map(|c| (*id, c.dependencies.clone()))
            })
            .collect();
        for (id, source) in created {
            if id.slot >= self.sources.len() {
                for slot in self.sources.len()..id.slot {
                    self.free.push(slot);
                }
                self.sources.resize_with(id.slot + 1, || SourceSlot {
                    generation: 0,
                    source: None,
                });
            }
            self.free.retain(|slot| *slot != id.slot);
            self.sources[id.slot] = SourceSlot {
                generation: id.generation,
                source: Some(source),
            };
        }
        for (id, dependencies) in dependencies {
            for dependency in dependencies {
                self.sources[dependency.slot]
                    .source
                    .as_mut()
                    .unwrap()
                    .dependents
                    .insert(id);
            }
        }
        effects.retain(|id| !removed_effects.contains(id));
        for (id, effect) in created_effects {
            for source in &effect.dependencies {
                self.sources[source.slot]
                    .source
                    .as_mut()
                    .unwrap()
                    .effects
                    .insert(id);
            }
            self.effects.insert(id, effect);
            effects.insert(id);
        }
        for (id, binding) in new_bindings {
            self.sources[binding.source.slot]
                .source
                .as_mut()
                .unwrap()
                .bindings
                .insert(id);
            self.endpoints.insert((binding.node, binding.field), id);
            self.bindings.insert(id, binding);
        }
        prepared
            .values
            .retain(|id, _| !removed_sources.contains(id));
        let published = self.publish_update(
            result.value,
            prepared,
            result.commit,
            sink_writes,
            BTreeSet::new(),
        );
        Ok(PendingReactiveEdit {
            output: ReactiveEditCommit {
                result: published,
                inserted: result.inserted,
                removed: result.removed,
            },
            effects,
            cleanups,
        })
    }
}

#[cfg(test)]
mod tests;
