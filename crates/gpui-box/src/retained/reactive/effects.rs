//! Post-commit observers with explicit dependencies and owned cleanup.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectId {
    owner: u64,
    serial: u64,
}
/// A post-commit callback sees only declared, successfully published values.
pub type EffectInputs<'a> = ComputedInputs<'a>;
/// Cleanup runs before replacement and at explicit/scoped/runtime disposal.
pub type EffectCleanup = Box<dyn FnOnce()>;
type Observe = dyn Fn(&EffectInputs<'_>) -> Option<EffectCleanup>;

pub(super) struct Effect {
    dependencies: BTreeSet<SourceId>,
    observe: Box<Observe>,
    cleanup: Option<EffectCleanup>,
}

impl ReactiveUi {
    /// Register and immediately run a synchronous observer. Subsequent runs
    /// occur after successful outer publication, at most once per commit when
    /// any declared dependency's final value changes. Callbacks are infallible;
    /// their external side effects and panics cannot roll back a committed UI.
    /// Inputs are read-only, so an effect cannot schedule recursive UI writes.
    pub fn effect(
        &mut self,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        let dependencies: BTreeSet<_> = dependencies.iter().map(|d| d.0).collect();
        for id in &dependencies {
            self.source(*id)?;
        }
        let serial = self
            .next_effect
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        let id = EffectId {
            owner: self.identity,
            serial,
        };
        for source in &dependencies {
            self.sources[source.slot]
                .source
                .as_mut()
                .unwrap()
                .effects
                .insert(id);
        }
        self.effects.insert(
            id,
            Effect {
                dependencies,
                observe: Box::new(observe),
                cleanup: None,
            },
        );
        self.next_effect = serial;
        self.run_effect(id);
        Ok(id)
    }

    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn dispose_effect(&mut self, id: EffectId) -> Result<(), Error> {
        let effect = self.effects.remove(&id).ok_or(Error::InvalidEffect)?;
        for source in effect.dependencies {
            self.sources[source.slot]
                .source
                .as_mut()
                .unwrap()
                .effects
                .remove(&id);
        }
        if let Some(cleanup) = effect.cleanup {
            cleanup();
        }
        Ok(())
    }

    pub(in crate::retained) fn validate_scoped_effect(
        &self,
        id: EffectId,
        dependencies: &[Dependency],
    ) -> Result<(), Error> {
        let effect = self.effects.get(&id).ok_or(Error::InvalidEffect)?;
        if effect.dependencies != dependencies.iter().map(|d| d.0).collect() {
            return Err(Error::InvalidEffect);
        }
        Ok(())
    }

    /// Returns whether a previous observation supplied cleanup. Called only
    /// after source/render publication (or initial registration).
    pub(super) fn run_effect(&mut self, id: EffectId) -> bool {
        let cleanup = self.effects.get_mut(&id).unwrap().cleanup.take();
        let cleaned = cleanup.is_some();
        if let Some(cleanup) = cleanup {
            cleanup();
        }
        let effect = &self.effects[&id];
        let values = BTreeMap::new();
        let cleanup = (effect.observe)(&EffectInputs {
            runtime: self,
            dependencies: &effect.dependencies,
            values: &values,
        });
        self.effects.get_mut(&id).unwrap().cleanup = cleanup;
        cleaned
    }
}

impl Drop for ReactiveUi {
    fn drop(&mut self) {
        for effect in self.effects.values_mut().rev() {
            if let Some(cleanup) = effect.cleanup.take() {
                cleanup();
            }
        }
    }
}

#[cfg(test)]
mod tests;
