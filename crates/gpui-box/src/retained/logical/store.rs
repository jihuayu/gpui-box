//! Whole business revisions enter the ordinary scoped source/frame transaction.
use super::*;
use crate::store::{ReadStore, StoreError, StoreRevision, StoreSnapshot, StoreSubscription};
use std::{marker::PhantomData, rc::Rc};

struct ScopedInput<S> {
    prop: ReadProp<StoreSnapshot<S>>,
    _cancellation: crate::store::StoreCancellation<S>,
}
impl<S: 'static> ScopedSource for ScopedInput<S> {
    fn as_any(&self) -> &dyn std::any::Any {
        &self.prop
    }
    fn dependency(&self) -> Dependency {
        self.prop.dependency()
    }
}

/// Owner-local bridge. The UI receives a read-only source; polling and applying
/// it are explicit host operations. Dropping it cancels only visual observation,
/// never the business writer or an already accepted business operation.
pub struct StoreInput<S> {
    subscription: StoreSubscription<S>,
    prop: ReadProp<StoreSnapshot<S>>,
    applied: Option<StoreRevision>,
    owner_local: PhantomData<Rc<()>>,
}
impl<S> StoreInput<S> {
    pub fn prop(&self) -> ReadProp<StoreSnapshot<S>> {
        self.prop
    }
    pub fn dependency(&self) -> Dependency {
        self.prop.dependency()
    }
}
impl<S: Send + Sync + 'static> StoreInput<S> {
    pub fn ready(&mut self) -> impl std::future::Future<Output = Result<bool, StoreError>> + '_ {
        self.subscription.ready()
    }
    pub fn applied_revision(&self) -> Option<StoreRevision> {
        self.applied
    }
}

/// A UI-local equality-filtered derived source. It has no writer/Signal escape.
///
/// ```compile_fail
/// use gpui_box::retained::{logical::StoreSelector, reactive::ReactiveTransaction};
/// fn mutate(tx: &mut ReactiveTransaction<'_>, selector: StoreSelector<u32>) {
///     tx.set(selector, 1).unwrap();
/// }
/// ```
pub struct StoreSelector<T> {
    pub(super) signal: Signal<T>,
    owner_local: PhantomData<Rc<()>>,
}
impl<T> Copy for StoreSelector<T> {}
impl<T> Clone for StoreSelector<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> StoreSelector<T> {
    pub fn dependency(self) -> Dependency {
        self.signal.dependency()
    }
}

pub struct StoreApply {
    /// Source/frame adoption, not GPU submission or displayed evidence.
    pub revision: StoreRevision,
    pub commit: ReactiveCommit<()>,
}
impl LogicalUi {
    /// Subscribe a scoped source to one captured snapshot. A later commit stays
    /// pending; all selectors initially read exactly this captured revision.
    pub fn attach_store<S: Send + Sync + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        store: &ReadStore<S>,
    ) -> Result<StoreInput<S>, Error> {
        if self.scope(scope)?.sources.contains_key(&key) {
            return Err(Error::InvalidStore);
        }
        let mut subscription = store.subscribe().map_err(|_| Error::InvalidStore)?;
        let snapshot = subscription
            .pending()
            .map_err(|_| Error::InvalidStore)?
            .ok_or(Error::InvalidStore)?;
        let revision = snapshot.revision();
        let prop = self
            .edit(|_| Ok(()), |(), edit| edit.prop(scope, key, snapshot))?
            .result
            .value;
        subscription
            .acknowledge(revision)
            .map_err(|_| Error::InvalidStore)?;
        self.scope_mut(scope).sources.insert(
            key,
            Box::new(ScopedInput {
                prop,
                _cancellation: subscription.scope_cancellation(),
            }),
        );
        Ok(StoreInput {
            subscription,
            prop,
            applied: Some(revision),
            owner_local: PhantomData,
        })
    }
    /// Consume at most one latest complete business revision through ordinary
    /// source settlement and the active component/layout/frame session. Failure
    /// preserves old UI and leaves the latest business snapshot pending.
    pub fn apply_store<S: Send + Sync + 'static>(
        &mut self,
        input: &mut StoreInput<S>,
    ) -> Result<Option<StoreApply>, Error> {
        let previous = self.read_prop(input.prop)?;
        let Some(snapshot) = input
            .subscription
            .pending()
            .map_err(|_| Error::InvalidStore)?
        else {
            return Ok(None);
        };
        if !previous.revision().same_store(snapshot.revision()) {
            return Err(Error::InvalidStore);
        }
        let revision = snapshot.revision();
        let commit = self
            .edit(
                |_| Ok(()),
                |(), edit| {
                    edit.render.prop_value(input.prop.signal, snapshot)?;
                    Ok(())
                },
            )?
            .result;
        input
            .subscription
            .acknowledge(revision)
            .map_err(|_| Error::InvalidStore)?;
        input.applied = Some(revision);
        Ok(Some(StoreApply { revision, commit }))
    }
    /// Light presentation projection over one whole snapshot. Ordinary derived
    /// equality stops unchanged selector output propagating to its consumers.
    pub fn select_store<S: Send + Sync + 'static, T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: ReadProp<StoreSnapshot<S>>,
        select: impl Fn(&S) -> Result<T, Error> + 'static,
    ) -> Result<StoreSelector<T>, Error> {
        let signal = self.computed(scope, key, &[input.dependency()], move |i| {
            select(i.read_prop(input)?.state())
        })?;
        Ok(StoreSelector {
            signal,
            owner_local: PhantomData,
        })
    }
    pub fn read_selector<T: 'static>(&self, selector: StoreSelector<T>) -> Result<&T, Error> {
        self.read(selector.signal)
    }
    pub fn bind_selector<T: 'static>(
        &mut self,
        selector: StoreSelector<T>,
        property: Property<T>,
    ) -> Result<(BindingId, Commit), Error> {
        self.bind(selector.signal, property)
    }
}
impl LogicalEdit<'_, '_, '_> {
    pub fn select_store<S: Send + Sync + 'static, T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        input: ReadProp<StoreSnapshot<S>>,
        select: impl Fn(&S) -> Result<T, Error> + 'static,
    ) -> Result<StoreSelector<T>, Error> {
        let signal = self.computed(scope, key, &[input.dependency()], move |i| {
            select(i.read_prop(input)?.state())
        })?;
        Ok(StoreSelector {
            signal,
            owner_local: PhantomData,
        })
    }
    pub fn read_selector<T: 'static>(&self, selector: StoreSelector<T>) -> Result<&T, Error> {
        self.read(selector.signal)
    }
    pub fn bind_selector<T: 'static>(
        &mut self,
        selector: StoreSelector<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.bind(selector.signal, property)
    }
}
impl ComputedInputs<'_> {
    pub fn read_selector<T: 'static>(&self, selector: StoreSelector<T>) -> Result<&T, Error> {
        self.read(selector.signal)
    }
}

#[cfg(test)]
mod tests;
