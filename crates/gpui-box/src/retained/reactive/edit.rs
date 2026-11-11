//! Source settlement and render construction share one validation/publication.
use super::*;

pub(super) struct PreparedUpdate {
    values: BTreeMap<SourceId, Rc<dyn Value>>,
    writes: u64,
    equal: u64,
    computed_evaluations: u64,
    computed_equal: u64,
}

pub struct ReactiveEditCommit<R> {
    pub result: ReactiveCommit<R>,
    pub inserted: Vec<NodeId>,
    pub removed: Vec<NodeId>,
}

/// Reads see the settled source draft. New bindings synchronize with those
/// values and publish only after the complete render draft validates.
pub struct ReactiveEdit<'a, 'tree> {
    tree: &'a mut TreeEdit<'tree>,
    identity: u64,
    sources: &'a [SourceSlot],
    values: &'a BTreeMap<SourceId, Rc<dyn Value>>,
    endpoints: &'a BTreeMap<(NodeId, Field), BindingId>,
    next_binding: &'a mut u64,
    bindings: Vec<(BindingId, Binding)>,
}
impl<'tree> ReactiveEdit<'_, 'tree> {
    pub fn tree(&mut self) -> &mut TreeEdit<'tree> {
        self.tree
    }
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        let source = self.source(signal.id)?;
        self.values
            .get(&signal.id)
            .unwrap_or(&source.value)
            .as_any()
            .downcast_ref()
            .ok_or(Error::InvalidSignal)
    }
    fn source(&self, id: SourceId) -> Result<&Source, Error> {
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
            .contains_key(&(property.node, property.field))
            || self
                .bindings
                .iter()
                .any(|(_, b)| b.node == property.node && b.field == property.field)
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
        self.bindings.push((id, binding));
        Ok(id)
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
        tx.settle()?;
        Ok((
            value,
            PreparedUpdate {
                values: tx.values,
                writes: tx.writes,
                equal: tx.equal,
                computed_evaluations: tx.computed_evaluations,
                computed_equal: tx.computed_equal,
            },
        ))
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
    /// Sources/scopes/effects themselves retain their existing lifetimes.
    pub fn edit<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut ReactiveEdit<'_, '_>) -> Result<R, Error>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        let (source_result, prepared) = self.prepare_update(sources)?;
        // Capture each endpoint separately so deletions can skip its whole write.
        let mut sinks = vec![];
        let mut effects = BTreeSet::new();
        for (id, value) in &prepared.values {
            let source = self.source(*id)?;
            effects.extend(&source.effects);
            for id in &source.bindings {
                let b = &self.bindings[id];
                let mut tx = Transaction::default();
                (b.write)(value.as_ref(), &mut tx);
                sinks.push((b.node, tx));
            }
        }
        let mut new_bindings = vec![];
        let mut sink_writes = 0;
        let identity = self.identity;
        let result = self.tree.edit(self.capabilities, |tree| {
            let mut edit = ReactiveEdit {
                tree,
                identity,
                sources: &self.sources,
                values: &prepared.values,
                endpoints: &self.endpoints,
                next_binding: &mut self.next_binding,
                bindings: vec![],
            };
            let value = render(source_result, &mut edit)?;
            let dead = edit.tree.removed_ids()?;
            for (_, binding) in &edit.bindings {
                if dead.contains(&binding.node) {
                    return Err(Error::InvalidNode);
                }
                let value = edit
                    .values
                    .get(&binding.source)
                    .unwrap_or(&edit.source(binding.source)?.value)
                    .clone();
                (binding.write)(value.as_ref(), edit.tree.properties());
            }
            sink_writes = edit.bindings.len();
            for (node, tx) in sinks {
                if !dead.contains(&node) {
                    edit.tree.properties().patches.extend(tx.patches);
                    sink_writes += 1;
                }
            }
            new_bindings = edit.bindings;
            Ok(value)
        })?;
        if !result.removed.is_empty() {
            self.retire_dead_bindings();
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
        Ok(ReactiveEditCommit {
            result: self.publish_update(
                result.value,
                prepared,
                result.commit,
                sink_writes,
                effects,
            ),
            inserted: result.inserted,
            removed: result.removed,
        })
    }
}

#[cfg(test)]
mod tests;
