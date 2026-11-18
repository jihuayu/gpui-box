//! Logical-scope capabilities, resolved once per consumer lifetime. Store values
//! remain one captured UI input, never an uncoordinated business snapshot read.
use super::*;
use crate::{
    command::{Command, CommandSender},
    store::{StoreKey, StoreSnapshot},
};
use std::{
    any::{Any, TypeId, type_name},
    rc::Rc,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Store(TypeId),
    Command(TypeId),
}
struct Entry {
    value: Box<dyn Any>,
    dependency: Option<Dependency>,
}
#[derive(Clone, Default)]
pub(super) struct Environment {
    provided: BTreeMap<Key, Rc<Entry>>,
    resolved: BTreeMap<Key, Option<Rc<Entry>>>,
}
pub(super) type Registry = BTreeMap<ScopeId, Rc<Environment>>;

pub(super) fn validate_retirement(
    registry: &Registry,
    dead: &BTreeSet<ScopeId>,
    sources: &BTreeSet<Dependency>,
) -> Result<(), Error> {
    for (_, environment) in registry.iter().filter(|(scope, _)| !dead.contains(scope)) {
        for entry in environment
            .provided
            .values()
            .chain(environment.resolved.values().flatten())
        {
            if entry
                .dependency
                .is_some_and(|dependency| sources.contains(&dependency))
            {
                return Err(Error::InvalidProvider);
            }
        }
    }
    Ok(())
}

impl LogicalEdit<'_, '_, '_> {
    fn providers(&self) -> &Registry {
        self.providers.as_ref().unwrap_or(self.live_providers)
    }
    fn environment_mut(&mut self, scope: ScopeId) -> &mut Environment {
        let registry = self
            .providers
            .get_or_insert_with(|| self.live_providers.clone());
        Rc::make_mut(registry.entry(scope).or_default())
    }
    fn provider_scope(&self, scope: ScopeId) -> Result<(), Error> {
        self.scope(scope)?;
        if self.retired_scopes()?.contains(&scope) {
            return Err(Error::InvalidScope);
        }
        Ok(())
    }
    fn declare_provider(
        &mut self,
        scope: ScopeId,
        key: Key,
        entry: Entry,
        same: impl Fn(&dyn Any) -> bool,
    ) -> Result<(), Error> {
        self.provider_scope(scope)?;
        if let Some(environment) = self.providers().get(&scope) {
            if let Some(old) = environment.provided.get(&key) {
                return if same(old.value.as_ref()) {
                    Ok(())
                } else {
                    Err(Error::InvalidProvider)
                };
            }
            // A component must declare its providers before consuming that key.
            if environment.resolved.contains_key(&key) {
                return Err(Error::InvalidProvider);
            }
        }
        self.environment_mut(scope)
            .provided
            .insert(key, Rc::new(entry));
        Ok(())
    }
    /// Publish a captured, read-only UI Store input under an explicit key. Same
    /// handle is idempotent; another input requires a new provider scope lifetime.
    ///
    /// ```compile_fail
    /// use gpui_box::{store::{StoreKey, StoreSnapshot}, retained::logical::{LogicalEdit, ReadProp, ScopeId}};
    /// struct Document;
    /// impl StoreKey for Document { type State = String; }
    /// fn wrong_state(edit: &mut LogicalEdit<'_, '_, '_>, scope: ScopeId, input: ReadProp<StoreSnapshot<u32>>) {
    ///     edit.provide_store::<Document>(scope, input).unwrap();
    /// }
    /// ```
    pub fn provide_store<K: StoreKey>(
        &mut self,
        scope: ScopeId,
        input: ReadProp<StoreSnapshot<K::State>>,
    ) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        self.render.validate_live_dependency(input.dependency())?;
        self.render.read(input.signal)?;
        self.declare_provider(
            scope,
            Key::Store(TypeId::of::<K>()),
            Entry {
                value: Box::new(input),
                dependency: Some(input.dependency()),
            },
            |old| {
                old.downcast_ref::<ReadProp<StoreSnapshot<K::State>>>()
                    .is_some_and(|old| *old == input)
            },
        )?;
        attempt.success = true;
        Ok(())
    }
    /// Publish only one command type's sending capability. No business work is
    /// sent during this declaration. Identity is the channel, not payload type.
    pub fn provide_command<C: Command>(
        &mut self,
        scope: ScopeId,
        sender: CommandSender<C>,
    ) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        let identity = sender.clone();
        self.declare_provider(
            scope,
            Key::Command(TypeId::of::<C>()),
            Entry {
                value: Box::new(sender),
                dependency: None,
            },
            |old| {
                old.downcast_ref::<CommandSender<C>>()
                    .is_some_and(|old| old.same_channel(&identity))
            },
        )?;
        attempt.success = true;
        Ok(())
    }
    fn resolve_provider(&mut self, scope: ScopeId, key: Key) -> Result<Option<Rc<Entry>>, Error> {
        self.provider_scope(scope)?;
        if let Some(cached) = self
            .providers()
            .get(&scope)
            .and_then(|env| env.resolved.get(&key))
        {
            return Ok(cached.clone());
        }
        let mut current = Some(scope);
        let mut selected = None;
        while let Some(id) = current {
            if let Some(entry) = self
                .providers()
                .get(&id)
                .and_then(|env| env.provided.get(&key))
            {
                selected = Some(entry.clone());
                break;
            }
            current = self.scope(id)?.parent;
        }
        self.environment_mut(scope)
            .resolved
            .insert(key, selected.clone());
        Ok(selected)
    }
    fn missing_provider(&self, scope: ScopeId, dependency: &'static str) -> Error {
        let mut path = vec![];
        let mut current = Some(scope);
        while let Some(id) = current {
            let Ok(scope) = self.scope(id) else { break };
            path.push(scope.key);
            current = scope.parent;
        }
        path.reverse();
        Error::MissingProvider { path, dependency }
    }
    pub fn try_use_store<K: StoreKey>(
        &mut self,
        scope: ScopeId,
    ) -> Result<Option<ReadProp<StoreSnapshot<K::State>>>, Error> {
        let mut attempt = self.component_attempt()?;
        let selected = self.resolve_provider(scope, Key::Store(TypeId::of::<K>()))?;
        let input = selected
            .map(|entry| {
                entry
                    .value
                    .downcast_ref::<ReadProp<StoreSnapshot<K::State>>>()
                    .copied()
                    .ok_or(Error::InvalidProvider)
            })
            .transpose()?;
        if let Some(input) = input {
            self.render.validate_live_dependency(input.dependency())?;
        }
        attempt.success = true;
        Ok(input)
    }
    pub fn use_store<K: StoreKey>(
        &mut self,
        scope: ScopeId,
    ) -> Result<ReadProp<StoreSnapshot<K::State>>, Error> {
        let mut attempt = self.component_attempt()?;
        let input = self
            .try_use_store::<K>(scope)?
            .ok_or_else(|| self.missing_provider(scope, type_name::<K>()))?;
        attempt.success = true;
        Ok(input)
    }
    pub fn try_use_command<C: Command>(
        &mut self,
        scope: ScopeId,
    ) -> Result<Option<CommandSender<C>>, Error> {
        let mut attempt = self.component_attempt()?;
        let selected = self.resolve_provider(scope, Key::Command(TypeId::of::<C>()))?;
        let sender = selected
            .map(|entry| {
                entry
                    .value
                    .downcast_ref::<CommandSender<C>>()
                    .cloned()
                    .ok_or(Error::InvalidProvider)
            })
            .transpose()?;
        attempt.success = true;
        Ok(sender)
    }
    pub fn use_command<C: Command>(&mut self, scope: ScopeId) -> Result<CommandSender<C>, Error> {
        let mut attempt = self.component_attempt()?;
        let sender = self
            .try_use_command::<C>(scope)?
            .ok_or_else(|| self.missing_provider(scope, type_name::<C>()))?;
        attempt.success = true;
        Ok(sender)
    }
    pub(super) fn retire_and_validate_providers(
        &mut self,
        dead: &BTreeSet<ScopeId>,
    ) -> Result<(), Error> {
        for (_, environment) in self
            .providers()
            .iter()
            .filter(|(scope, _)| !dead.contains(scope))
        {
            for entry in environment
                .provided
                .values()
                .chain(environment.resolved.values().flatten())
            {
                if let Some(dependency) = entry.dependency {
                    self.render
                        .validate_live_dependency(dependency)
                        .map_err(|_| Error::InvalidProvider)?;
                }
            }
        }
        if self.providers().keys().any(|scope| dead.contains(scope)) {
            self.providers
                .get_or_insert_with(|| self.live_providers.clone())
                .retain(|scope, _| !dead.contains(scope));
        }
        Ok(())
    }
}
impl LogicalUi {
    pub fn provide_store<K: StoreKey>(
        &mut self,
        scope: ScopeId,
        input: ReadProp<StoreSnapshot<K::State>>,
    ) -> Result<(), Error> {
        self.edit(|_| Ok(()), |(), edit| edit.provide_store::<K>(scope, input))
            .map(|_| ())
    }
    pub fn provide_command<C: Command>(
        &mut self,
        scope: ScopeId,
        sender: CommandSender<C>,
    ) -> Result<(), Error> {
        self.edit(|_| Ok(()), |(), edit| edit.provide_command(scope, sender))
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
