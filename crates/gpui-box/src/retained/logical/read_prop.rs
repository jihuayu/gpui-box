//! Component payloads share ordinary source settlement and scope retirement.
use super::*;

impl LogicalEdit<'_, '_, '_> {
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
mod tests;
