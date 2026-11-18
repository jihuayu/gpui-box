//! Scope-owned reusable layout construction; one execution opportunity per edit.
use super::*;
use std::{cell::RefCell, rc::Rc};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayoutProducerId {
    owner: u64,
    pub(super) serial: u64,
}
type ProducerCallback = dyn for<'a, 'draft, 'tree> Fn(
    LayoutBuildInput,
    &mut LogicalEdit<'a, 'draft, 'tree>,
) -> Result<(), Error>;

/// Immutable callback identity. Reuse clones for keyed redeclaration; replacing
/// the handle invalidates the cached construction. Captures must be owner-local
/// immutable data or signal handles; external mutable reads cannot be tracked.
#[derive(Clone)]
pub struct LayoutProducer(Rc<ProducerCallback>);
impl LayoutProducer {
    pub fn new(
        build: impl for<'a, 'draft, 'tree> Fn(
            LayoutBuildInput,
            &mut LogicalEdit<'a, 'draft, 'tree>,
        ) -> Result<(), Error>
        + 'static,
    ) -> Self {
        Self(Rc::new(build))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutProducerStats {
    pub considered: usize,
    pub callbacks_executed: usize,
    pub callbacks_reused: usize,
}

#[derive(Clone)]
pub(super) struct Registration {
    pub id: LayoutProducerId,
    pub host: NodeId,
    pub kind: LayoutBuildKind,
    pub after: BTreeSet<LayoutProducerId>,
    producer: LayoutProducer,
    observed: Option<(LayoutBuildInput, reactive::SourceObservation)>,
}
impl Registration {
    pub(super) fn task(&self) -> LayoutBuildTaskId {
        LayoutBuildTaskId(self.id.serial)
    }
}

impl LogicalEdit<'_, '_, '_> {
    pub(super) fn producers(&self) -> &BTreeMap<(ScopeId, u64), Registration> {
        self.layout_producers
            .as_ref()
            .unwrap_or(self.live_layout_producers)
    }
    fn producers_mut(&mut self) -> &mut BTreeMap<(ScopeId, u64), Registration> {
        self.layout_producers
            .get_or_insert_with(|| self.live_layout_producers.clone())
    }
    /// Register or replace a scope-owned reusable builder. Equal declarations
    /// retain observations. Dependencies read through `edit.read` are captured
    /// dynamically; other mutable captures require explicit invalidation.
    pub fn layout_producer(
        &mut self,
        scope: ScopeId,
        key: u64,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutProducerId],
        producer: LayoutProducer,
    ) -> Result<LayoutProducerId, Error> {
        let result = self.register_producer(scope, key, host, kind, after, producer);
        self.layout_build_failed |= result.is_err();
        result
    }
    fn register_producer(
        &mut self,
        scope: ScopeId,
        key: u64,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutProducerId],
        producer: LayoutProducer,
    ) -> Result<LayoutProducerId, Error> {
        self.scope(scope)?;
        if self.frame_environment.is_none() || self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        if self.inserted.get(&host).or_else(|| self.owners.get(&host)) != Some(&scope) {
            return Err(Error::ScopeInUse);
        }
        let after: BTreeSet<_> = after.iter().copied().collect();
        if after.iter().any(|id| id.owner != self.identity) {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        let old = self.producers().get(&(scope, key));
        if let Some(old) = old {
            if old.host == host
                && old.kind == kind
                && old.after == after
                && Rc::ptr_eq(&old.producer.0, &producer.0)
            {
                return Ok(old.id);
            }
            if self.producer_completed.contains(&old.id) || self.producer_active == Some(old.id) {
                return Err(Error::InvalidLayoutBuildPlan);
            }
        }
        if self
            .producers()
            .iter()
            .any(|(id, r)| *id != (scope, key) && r.host == host)
        {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        let id = match old {
            Some(old) => old.id,
            None => {
                if self.producers().len() >= LayoutBuildLimits::default().max_tasks {
                    return Err(Error::LayoutBuildPlanLimit);
                }
                *self.next_layout_producer = self
                    .next_layout_producer
                    .checked_add(1)
                    .ok_or(Error::RevisionExhausted)?;
                LayoutProducerId {
                    owner: self.identity,
                    serial: *self.next_layout_producer,
                }
            }
        };
        if after.contains(&id) || after.len() > LayoutBuildLimits::default().max_dependency_edges {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        self.producers_mut().insert(
            (scope, key),
            Registration {
                id,
                host,
                kind,
                after,
                producer,
                observed: None,
            },
        );
        Ok(id)
    }
    pub fn remove_layout_producer(&mut self, scope: ScopeId, key: u64) -> Result<(), Error> {
        let result = (|| {
            self.scope(scope)?;
            let entry = self
                .producers()
                .get(&(scope, key))
                .ok_or(Error::InvalidLayoutBuildPlan)?;
            if self.producer_completed.contains(&entry.id) || self.producer_active == Some(entry.id)
            {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            self.producers_mut().remove(&(scope, key));
            Ok(())
        })();
        self.layout_build_failed |= result.is_err();
        result
    }
    pub fn invalidate_layout_producer(&mut self, id: LayoutProducerId) -> Result<(), Error> {
        let result = (|| {
            if self.producer_completed.contains(&id) || self.producer_active == Some(id) {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            let key = self
                .producers()
                .iter()
                .find(|(_, r)| r.id == id)
                .map(|(key, _)| *key)
                .ok_or(Error::InvalidLayoutBuildPlan)?;
            self.producers_mut().get_mut(&key).unwrap().observed = None;
            Ok(())
        })();
        self.layout_build_failed |= result.is_err();
        result
    }
    pub(super) fn retire_layout_producers(&mut self) -> Result<(), Error> {
        if self.producers().is_empty() {
            return Ok(());
        }
        let dead = self.retired_scopes()?;
        if self
            .producers()
            .keys()
            .any(|(scope, _)| dead.contains(scope))
        {
            // Already executed hosts retain their ordinary final observations;
            // retiring them later cannot erase validation retroactively.
            self.producers_mut()
                .retain(|(scope, _), _| !dead.contains(scope));
        }
        Ok(())
    }
    pub(super) fn execute_layout_producer(
        &mut self,
        key: (ScopeId, u64),
        input: LayoutBuildInput,
    ) -> Result<(), Error> {
        let registration = self
            .producers()
            .get(&key)
            .cloned()
            .ok_or(Error::InvalidLayoutBuildPlan)?;
        self.producer_stats.considered += 1;
        let reusable = match &registration.observed {
            Some((previous, sources)) => {
                self.producer_logical_unchanged()
                    && self.producer_render_revision == self.live_producer_revision
                    && *previous == input
                    && self.render.matches_sources(sources)?
            }
            None => false,
        };
        if reusable {
            self.producer_stats.callbacks_reused += 1;
        } else {
            self.producer_reads = Some(RefCell::new(reactive::SourceObservation::default()));
            self.producer_active = Some(registration.id);
            let result = (registration.producer.0)(input, self);
            self.producer_active = None;
            let reads = self
                .producer_reads
                .take()
                .expect("producer read checkpoint")
                .into_inner();
            result?;
            let sources = reads;
            if !self.render.matches_sources(&sources)? {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            self.producers_mut()
                .get_mut(&key)
                .ok_or(Error::InvalidLayoutBuildPlan)?
                .observed = Some((input, sources));
            self.producer_stats.callbacks_executed += 1;
        }
        self.producer_completed.insert(registration.id);
        Ok(())
    }
    pub(super) fn settle_layout_producers(&mut self) -> Result<(), Error> {
        self.retire_layout_producers()?;
        if self.producers().is_empty() {
            return Ok(());
        }
        if self.frame_environment.is_none() {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        self.producer_scheduling = true;
        let result = self.build_layout_plan(LayoutBuildPlan::new());
        self.producer_scheduling = false;
        result?;
        self.validate_producer_sources()
    }
    pub(super) fn validate_producer_sources(&self) -> Result<(), Error> {
        for registration in self.producers().values() {
            if let Some((_, sources)) = &registration.observed
                && !self.render.matches_sources(sources)?
            {
                return Err(Error::InvalidLayoutBuildPlan);
            }
        }
        Ok(())
    }
}
