//! Component payloads share ordinary source settlement and scope retirement.
use super::*;

/// Restricted component input declaration batch. No source reads, computations,
/// layout or user mount callbacks run until all final inputs settle together.
/// Failed declarations poison this batch; other edits may handle its error.
///
/// ```compile_fail
/// use gpui_box::retained::logical::{LogicalEdit, ReadProp};
/// fn partial(edit: &mut LogicalEdit<'_, '_, '_>, input: ReadProp<u32>) {
///     edit.props(|batch| { batch.read_prop(input)?; Ok(()) }).unwrap();
/// }
/// ```
pub struct PropBatch<'a, 'edit, 'draft, 'tree> {
    edit: &'a mut LogicalEdit<'edit, 'draft, 'tree>,
    values: reactive::SourceValues,
    sources: BTreeMap<(ScopeId, u64), Box<dyn ScopedSource>>,
    allocated: Vec<Dependency>,
    checkpoint: Option<reactive::PropSourceCheckpoint>,
    retired: BTreeSet<ScopeId>,
    invalid: bool,
}
impl PropBatch<'_, '_, '_, '_> {
    pub fn prop<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        value: T,
    ) -> Result<ReadProp<T>, Error> {
        let result = self.declare(scope, key, value);
        self.invalid |= result.is_err();
        result
    }
    fn declare<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        value: T,
    ) -> Result<ReadProp<T>, Error> {
        if self.invalid {
            return Err(Error::InvalidPropBatch);
        }
        self.edit.scope(scope)?;
        if self.retired.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        let previous = self
            .sources
            .get(&(scope, key))
            .or_else(|| self.edit.added_sources.get(&(scope, key)))
            .or_else(|| self.edit.scope(scope).ok()?.sources.get(&key));
        let prop = if let Some(source) = previous {
            *source
                .as_any()
                .downcast_ref::<ReadProp<T>>()
                .ok_or(Error::InvalidSignal)?
        } else {
            let prop = ReadProp {
                signal: self.edit.render.signal(value)?,
                owner_local: std::marker::PhantomData,
            };
            self.allocated.push(prop.dependency());
            self.sources.insert((scope, key), Box::new(prop));
            return Ok(prop);
        };
        self.values.push(prop.signal, value);
        Ok(prop)
    }
    fn finish(mut self) -> Result<(), Error> {
        if self.invalid {
            return Err(Error::InvalidPropBatch);
        }
        self.edit
            .render
            .prop_values(std::mem::take(&mut self.values))?;
        self.edit.added_sources.append(&mut self.sources);
        self.checkpoint = None;
        Ok(())
    }
}
impl Drop for PropBatch<'_, '_, '_, '_> {
    fn drop(&mut self) {
        if let Some(checkpoint) = self.checkpoint.take() {
            self.edit
                .render
                .rollback_prop_sources(checkpoint, &self.allocated);
        }
    }
}

impl LogicalEdit<'_, '_, '_> {
    /// Declare heterogeneous keyed inputs, then settle their final tuple once.
    /// Returned handles become visible only on success. No general LogicalEdit
    /// capability is exposed inside the batch, so partial tuple reads cannot
    /// escape through derived/mount/layout checkpoints. Allocations roll back
    /// on error or unwind, while consumed generations stay retired.
    pub fn props<R>(
        &mut self,
        declare: impl FnOnce(&mut PropBatch<'_, '_, '_, '_>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let retired = self.retired_scopes()?;
        let checkpoint = self.render.prop_source_checkpoint();
        let mut batch = PropBatch {
            edit: self,
            values: reactive::SourceValues::default(),
            sources: BTreeMap::new(),
            allocated: vec![],
            checkpoint: Some(checkpoint),
            retired,
            invalid: false,
        };
        let result = declare(&mut batch)?;
        batch.finish()?;
        Ok(result)
    }
    /// Create or replace a keyed live input. This is the input writer; unlike
    /// state(), reading the existing value is not an implicit producer read.
    /// Consumers explicitly read_prop/bind_prop and cannot obtain a setter.
    pub fn prop<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        value: T,
    ) -> Result<ReadProp<T>, Error> {
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        if let Some(source) = self
            .added_sources
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.sources.get(&key))
        {
            let prop = *source
                .as_any()
                .downcast_ref::<ReadProp<T>>()
                .ok_or(Error::InvalidSignal)?;
            self.render.prop_value(prop.signal, value)?;
            return Ok(prop);
        }
        let prop = ReadProp {
            signal: self.render.signal(value)?,
            owner_local: std::marker::PhantomData,
        };
        self.added_sources.insert((scope, key), Box::new(prop));
        Ok(prop)
    }

    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.read(prop.signal)
    }
    pub fn bind_prop<T: 'static>(
        &mut self,
        prop: ReadProp<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.bind(prop.signal, property)
    }
}
impl LogicalUi {
    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.ui.read_prop(prop)
    }
    pub fn bind_prop<T: 'static>(
        &mut self,
        prop: ReadProp<T>,
        property: Property<T>,
    ) -> Result<(BindingId, Commit), Error> {
        self.bind(prop.signal, property)
    }
}

#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod tests;
