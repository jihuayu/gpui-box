//! Static dependency DAG. Registration only references existing sources, so no
//! cycles can be formed. Transaction settlement never mutates committed values.
use super::*;

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
    evaluate: Box<Evaluate>,
}

/// Read-only view of declared dependencies at one consistent transaction point.
/// Computations must be pure: returned errors roll back UI state, but cannot
/// undo arbitrary external side effects performed by a callback.
pub struct ComputedInputs<'a> {
    runtime: &'a ReactiveUi,
    dependencies: &'a BTreeSet<SourceId>,
    values: &'a BTreeMap<SourceId, Rc<dyn Value>>,
    overlay: &'a BTreeMap<SourceId, Rc<dyn Value>>,
}
impl ComputedInputs<'_> {
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        if !self.dependencies.contains(&signal.id) {
            return Err(Error::UndeclaredDependency);
        }
        let source = self.runtime.source(signal.id)?;
        self.overlay
            .get(&signal.id)
            .or_else(|| self.values.get(&signal.id))
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
            runtime: self,
            dependencies: &dependencies,
            values: &empty,
            overlay: &empty,
        })?;
        let source = self.insert_source(Source {
            value: Rc::new(value),
            bindings: BTreeSet::new(),
            dependents: BTreeSet::new(),
            computation: Some(Computation {
                dependencies: dependencies.clone(),
                evaluate: Box::new(move |inputs| Ok(Rc::new(evaluate(inputs)?))),
            }),
            rank,
        });
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
        }
        Ok(())
    }
}

impl ReactiveTransaction<'_> {
    pub(super) fn settle(&mut self) -> Result<(), Error> {
        if self.dirty_roots.is_empty() {
            return Ok(());
        }
        let mut pending = BTreeSet::new();
        for id in self.dirty_roots.keys() {
            for dependent in &self.runtime.source(*id)?.dependents {
                pending.insert((self.runtime.source(*dependent)?.rank, *dependent));
            }
        }
        // Keep the entire propagation private until all callbacks succeed. A
        // caught error may be corrected and retried within the same transaction.
        let mut overlay = BTreeMap::new();
        let mut evaluations = 0;
        let mut equal = 0;
        while let Some((_, id)) = pending.pop_first() {
            let source = self.runtime.source(id)?;
            let computation = source.computation.as_ref().expect("derived dependent");
            let value = (computation.evaluate)(&ComputedInputs {
                runtime: self.runtime,
                dependencies: &computation.dependencies,
                values: &self.values,
                overlay: &overlay,
            })?;
            evaluations += 1;
            let previous = self.values.get(&id).unwrap_or(&source.value);
            if value.equal(previous.as_ref()) {
                equal += 1;
                continue;
            }
            overlay.insert(id, value);
            for dependent in &source.dependents {
                pending.insert((self.runtime.source(*dependent)?.rank, *dependent));
            }
        }
        for (id, value) in overlay {
            if self.runtime.source(id)?.value.equal(value.as_ref()) {
                self.values.remove(&id);
            } else {
                self.values.insert(id, value);
            }
        }
        self.dirty_roots.clear();
        self.computed_evaluations += evaluations;
        self.computed_equal += equal;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
