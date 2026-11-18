//! Typed mount-once components over ordinary logical scopes and ReadProp inputs.
use super::*;
use std::{any::TypeId, cell::Cell, rc::Rc};

/// Component initialization executes once per keyed instance. Subsequent input
/// declarations replace its live ReadProp; bindings/derived values/producers
/// update output without rerunning mount. Side effects belong in effect().
pub trait Component: 'static {
    type Props: PartialEq + 'static;
    fn mount(
        props: ReadProp<Self::Props>,
        cx: &mut ComponentContext<'_, '_, '_, '_>,
    ) -> Result<(), Error>;
}

/// Explicit initialization capability; state/effect keys are local to this
/// component scope. There is no ambient TLS scope and no extra layout box.
///
/// ```compile_fail
/// use gpui_box::retained::logical::ComponentContext;
/// fn escape(cx: &mut ComponentContext<'_, '_, '_, '_>) {
///     std::thread::spawn(move || { let _ = cx.scope(); });
/// }
/// ```
pub struct ComponentContext<'a, 'edit, 'draft, 'tree> {
    edit: &'a mut LogicalEdit<'edit, 'draft, 'tree>,
    scope: ScopeId,
    host: NodeId,
}
impl ComponentContext<'_, '_, '_, '_> {
    pub fn scope(&self) -> ScopeId {
        self.scope
    }
    pub fn host(&self) -> NodeId {
        self.host
    }
    pub fn state<T: PartialEq + 'static>(
        &mut self,
        key: u64,
        init: impl FnOnce() -> T,
    ) -> Result<Signal<T>, Error> {
        if key == u64::MAX {
            return Err(Error::InvalidComponent);
        }
        self.edit.state(self.scope, key, init)
    }
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        key: u64,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        if key == u64::MAX {
            return Err(Error::InvalidComponent);
        }
        self.edit.computed(self.scope, key, dependencies, evaluate)
    }
    pub fn effect(
        &mut self,
        key: u64,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        self.edit.effect(self.scope, key, dependencies, observe)
    }
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        self.edit.read(signal)
    }
    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.edit.read_prop(prop)
    }
    pub fn bind<T: 'static>(
        &mut self,
        signal: Signal<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.edit.bind(signal, property)
    }
    pub fn bind_prop<T: 'static>(
        &mut self,
        prop: ReadProp<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.edit.bind_prop(prop, property)
    }
    pub fn append_layout(&mut self, style: LayoutStyle, color: u32) -> Result<NodeId, Error> {
        self.edit.append_layout(self.scope, self.host, style, color)
    }
    pub fn append_child_layout(
        &mut self,
        parent: NodeId,
        style: LayoutStyle,
        color: u32,
    ) -> Result<NodeId, Error> {
        self.edit.append_layout(self.scope, parent, style, color)
    }
    pub fn properties(&mut self) -> &mut Transaction {
        self.edit.properties()
    }
    pub fn layout_producer(
        &mut self,
        key: u64,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutProducerId],
        producer: LayoutProducer,
    ) -> Result<LayoutProducerId, Error> {
        self.edit
            .layout_producer(self.scope, key, host, kind, after, producer)
    }
    /// Nested homogeneous keyed component site. This component owns its direct
    /// logical children; use explicit child sites for multiple independent lists.
    pub fn components<C: Component>(
        &mut self,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) -> Result<Vec<ScopeId>, Error> {
        self.edit.components::<C>(self.scope, host, items)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Identity {
    kind: TypeId,
    host: NodeId,
}

struct Attempt {
    failed: Rc<Cell<bool>>,
    success: bool,
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.success {
            self.failed.set(true);
        }
    }
}
impl LogicalEdit<'_, '_, '_> {
    pub(super) fn validate_components(&self) -> Result<(), Error> {
        if self
            .component_failure
            .as_ref()
            .is_some_and(|failed| failed.get())
        {
            Err(Error::InvalidComponent)
        } else {
            Ok(())
        }
    }
    /// Reconcile one typed keyed component site, batch all payloads, then mount
    /// only new identities. The site owns all direct children of parent. Same
    /// key/type/host retains state, including components with zero outputs.
    /// A changed type remounts; a changed host also establishes a fresh instance.
    /// Any construction error/unwind poisons this outer edit even if caught.
    pub fn components<C: Component>(
        &mut self,
        parent: ScopeId,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) -> Result<Vec<ScopeId>, Error> {
        self.validate_components()?;
        let failed = self
            .component_failure
            .get_or_insert_with(|| Rc::new(Cell::new(false)))
            .clone();
        let mut attempt = Attempt {
            failed,
            success: false,
        };
        self.scope(parent)?;
        if self.retired_scopes()?.contains(&parent) {
            return Err(Error::InvalidScope);
        }
        // Reject a stale/removed host before running mount code.
        self.render.tree().children(host)?;
        if self.render.tree().removed_ids()?.contains(&host) {
            return Err(Error::InvalidNode);
        }
        let rows: Vec<_> = items.into_iter().collect();
        let keys: Vec<_> = rows.iter().map(|r| r.0).collect();
        let identity = Identity {
            kind: TypeId::of::<C>(),
            host,
        };
        let previous: BTreeSet<_> = self
            .children(parent)?
            .iter()
            .copied()
            .filter(|id| {
                self.scope(*id)
                    .is_ok_and(|scope| scope.component.as_deref() == Some(&identity))
            })
            .collect();
        let scopes = self.reconcile_scopes(parent, &keys, Some(identity))?;
        // A component payload is one value, so its fields are coherent. All row
        // payloads settle together before any new mount/derived checkpoint.
        let inputs = self.props(|batch| {
            scopes
                .iter()
                .copied()
                .zip(rows)
                .map(|(scope, (_, props))| Ok((scope, batch.prop(scope, u64::MAX, props)?)))
                .collect::<Result<Vec<_>, Error>>()
        })?;
        for (scope, props) in inputs {
            if !previous.contains(&scope) {
                C::mount(
                    props,
                    &mut ComponentContext {
                        edit: self,
                        scope,
                        host,
                    },
                )?;
            }
        }
        self.validate_components()?;
        attempt.success = true;
        Ok(scopes)
    }
}

#[cfg(test)]
mod tests;
