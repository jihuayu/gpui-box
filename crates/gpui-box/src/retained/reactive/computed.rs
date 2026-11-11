//! Static dependency DAG. Registration only references existing sources, so no
//! cycles can be formed. Transaction settlement never mutates committed values.
use super::*;
use std::{cmp::Reverse, collections::BinaryHeap};

/// Erased dependency identity; obtained from a typed signal. Runtime ownership
/// and generation are checked before a computation or its edges are installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Dependency(pub(super) SourceId);

impl<T> Signal<T> {
    pub fn dependency(self) -> Dependency {
        Dependency(self.id)
    }
}

type Evaluate = dyn Fn(&ComputedInputs<'_>) -> Result<Rc<dyn Value>, Error>;
pub(super) struct Computation {
    pub(super) dependencies: BTreeSet<SourceId>,
    pub(super) evaluate: Box<Evaluate>,
}

/// Read-only view of declared dependencies at one consistent transaction point.
/// Computations must be pure: returned errors roll back UI state, but cannot
/// undo arbitrary external side effects performed by a callback.
pub struct ComputedInputs<'a> {
    pub(super) context: SourceContext<'a>,
    pub(super) dependencies: &'a BTreeSet<SourceId>,
    pub(super) values: &'a BTreeMap<SourceId, Rc<dyn Value>>,
}
impl ComputedInputs<'_> {
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        if !self.dependencies.contains(&signal.id) {
            return Err(Error::UndeclaredDependency);
        }
        let source = self.context.source(signal.id)?;
        self.values
            .get(&signal.id)
            .unwrap_or(&source.value)
            .as_any()
            .downcast_ref::<T>()
            .ok_or(Error::InvalidSignal)
    }
}

impl ReactiveUi {
    /// Initialize an immutable set of dependencies and a read-only derived
    /// signal. A batch evaluates reachable nodes in rank order; equal outputs
    /// do not propagate. Dependencies cannot be rebound after registration.
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        let dependencies: BTreeSet<_> =
            dependencies.iter().map(|dependency| dependency.0).collect();
        let mut rank = 0;
        for id in &dependencies {
            rank = rank.max(
                self.source(*id)?
                    .rank
                    .checked_add(1)
                    .ok_or(Error::RevisionExhausted)?,
            );
        }
        let empty = BTreeMap::new();
        let value = evaluate(&ComputedInputs {
            context: SourceContext::Runtime(self),
            dependencies: &dependencies,
            values: &empty,
        })?;
        let source = self.insert_source(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
            dependents: BTreeSet::new(),
            effects: BTreeSet::new(),
            computation: Some(Computation {
                dependencies: dependencies.clone(),
                evaluate: Box::new(move |inputs| Ok(Rc::new(evaluate(inputs)?))),
            }),
            rank,
            scheduled_epoch: Cell::new(0),
            delivery_lifetime: None,
        })?;
        for id in dependencies {
            self.sources[id.slot]
                .source
                .as_mut()
                .unwrap()
                .dependents
                .insert(source.id);
        }
        Ok(source)
    }

    pub(in crate::retained) fn validate_scoped_source<T: 'static>(
        &self,
        signal: Signal<T>,
        dependencies: Option<&[Dependency]>,
    ) -> Result<(), Error> {
        self.read(signal)?;
        match (&self.source(signal.id)?.computation, dependencies) {
            (None, None) => Ok(()),
            (Some(computation), Some(dependencies))
                if computation.dependencies == dependencies.iter().map(|d| d.0).collect() =>
            {
                Ok(())
            }
            _ => Err(Error::InvalidSignal),
        }
    }

    /// Scope reconciliation preflights all removed scopes as a single set, so
    /// sibling/ancestor dependencies are independent of source-key order.
    pub(in crate::retained) fn validate_scope_disposal(
        &self,
        sources: &BTreeSet<Dependency>,
        effects: &BTreeSet<EffectId>,
    ) -> Result<(), Error> {
        for id in sources {
            if self
                .source(id.0)?
                .dependents
                .iter()
                .any(|dependent| !sources.contains(&Dependency(*dependent)))
            {
                return Err(Error::SignalInUse);
            }
            if self
                .source(id.0)?
                .effects
                .iter()
                .any(|effect| !effects.contains(effect))
            {
                return Err(Error::SignalInUse);
            }
        }
        Ok(())
    }
}

impl ReactiveTransaction<'_> {
    pub(super) fn settle(&mut self) -> Result<(), Error> {
        if self.dirty_roots.is_empty() {
            return Ok(());
        }
        let epoch = self
            .runtime
            .schedule_epoch
            .get()
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        self.runtime.schedule_epoch.set(epoch);
        let mut pending = BinaryHeap::new();
        for id in self.dirty_roots.keys() {
            for dependent in &self.runtime.source(*id)?.dependents {
                self.runtime
                    .source(*dependent)?
                    .enqueue(*dependent, epoch, &mut pending);
            }
        }
        // Journal only replaced staged values, avoiding a second full ordered
        // map of derived outputs. None of these values are committed yet.
        let mut undo = vec![];
        let mut evaluations = 0;
        let mut equal = 0;
        while let Some(Reverse((_, id))) = pending.pop() {
            let source = self.runtime.source(id)?;
            let computation = source.computation.as_ref().expect("derived dependent");
            let value = match (computation.evaluate)(&ComputedInputs {
                context: SourceContext::Runtime(self.runtime),
                dependencies: &computation.dependencies,
                values: &self.values,
            }) {
                Ok(value) => value,
                Err(error) => {
                    for (id, previous) in undo {
                        if let Some(previous) = previous {
                            self.values.insert(id, previous);
                        } else {
                            self.values.remove(&id);
                        }
                    }
                    return Err(error);
                }
            };
            evaluations += 1;
            let previous = self.values.get(&id).unwrap_or(&source.value);
            if value.equal(previous.as_ref()) {
                equal += 1;
                continue;
            }
            let previous = if source.value.equal(value.as_ref()) {
                self.values.remove(&id)
            } else {
                self.values.insert(id, value)
            };
            undo.push((id, previous));
            for dependent in &source.dependents {
                self.runtime
                    .source(*dependent)?
                    .enqueue(*dependent, epoch, &mut pending);
            }
        }
        self.dirty_roots.clear();
        self.computed_evaluations += evaluations;
        self.computed_equal += equal;
        Ok(())
    }
}

impl Source {
    fn enqueue(
        &self,
        id: SourceId,
        epoch: u64,
        pending: &mut BinaryHeap<Reverse<(u64, SourceId)>>,
    ) {
        // Queue stamps are transient scheduler metadata. Failed settlements and
        // nested rollback never reuse an epoch, so their stamps cannot suppress
        // later work. No O(all sources) bitmap initialization is required.
        if self.scheduled_epoch.get() != epoch {
            self.scheduled_epoch.set(epoch);
            pending.push(Reverse((self.rank, id)));
        }
    }
}

#[cfg(test)]
mod tests;
