//! Typed mount-once components over ordinary logical scopes and ReadProp inputs.
use super::*;
use std::{any::TypeId, cell::Cell, rc::Rc};
mod plan;
pub use plan::{ComponentInputs, ComponentPlan, ComponentProducer};

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

/// A keyed logical region, with no render node or layout box. Its parent owns
/// the region's placement; declarations at this handle own only its contents.
/// Different sites have independent item-key namespaces. Removal retires the
/// ordinary scope subtree; this generation-checked handle cannot revive it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentSite {
    scope: ScopeId,
}
impl ComponentSite {
    /// Identity for inspection and existing explicit scope operations. Low-level
    /// writers must respect the site's exclusive direct-child-list ownership.
    pub fn scope(self) -> ScopeId {
        self.scope
    }
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
    pub fn event_handler(
        &mut self,
        key: u64,
        handler: EventHandler,
    ) -> Result<input::UiHandlerId, Error> {
        self.edit.event_handler(self.scope, key, handler)
    }
    pub fn interaction(
        &mut self,
        node: NodeId,
        policy: Option<input::UiInteraction>,
    ) -> Result<(), Error> {
        self.edit.interaction(self.scope, node, policy)
    }
    pub fn interaction_state(
        &mut self,
        key: u64,
        node: NodeId,
        disabled: bool,
        selected: bool,
    ) -> Result<ReadProp<style::StyleState>, Error> {
        if key == u64::MAX {
            return Err(Error::InvalidComponent);
        }
        self.edit
            .interaction_state(self.scope, key, node, disabled, selected)
    }
    pub fn provide_store<K: crate::store::StoreKey>(
        &mut self,
        input: ReadProp<crate::store::StoreSnapshot<K::State>>,
    ) -> Result<(), Error> {
        self.edit.provide_store::<K>(self.scope, input)
    }
    pub fn use_store<K: crate::store::StoreKey>(
        &mut self,
    ) -> Result<ReadProp<crate::store::StoreSnapshot<K::State>>, Error> {
        self.edit.use_store::<K>(self.scope)
    }
    pub fn try_use_store<K: crate::store::StoreKey>(
        &mut self,
    ) -> Result<Option<ReadProp<crate::store::StoreSnapshot<K::State>>>, Error> {
        self.edit.try_use_store::<K>(self.scope)
    }
    pub fn provide_command<C: crate::command::Command>(
        &mut self,
        sender: crate::command::CommandSender<C>,
    ) -> Result<(), Error> {
        self.edit.provide_command(self.scope, sender)
    }
    pub fn use_command<C: crate::command::Command>(
        &mut self,
    ) -> Result<crate::command::CommandSender<C>, Error> {
        self.edit.use_command::<C>(self.scope)
    }
    pub fn try_use_command<C: crate::command::Command>(
        &mut self,
    ) -> Result<Option<crate::command::CommandSender<C>>, Error> {
        self.edit.try_use_command::<C>(self.scope)
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
    pub fn read_selector<T: 'static>(&self, selector: StoreSelector<T>) -> Result<&T, Error> {
        self.edit.read_selector(selector)
    }
    pub fn select_store<S: Send + Sync + 'static, T: PartialEq + 'static>(
        &mut self,
        key: u64,
        input: ReadProp<crate::store::StoreSnapshot<S>>,
        select: impl Fn(&S) -> Result<T, Error> + 'static,
    ) -> Result<StoreSelector<T>, Error> {
        if key == u64::MAX {
            return Err(Error::InvalidComponent);
        }
        self.edit.select_store(self.scope, key, input, select)
    }
    pub fn bind_selector<T: 'static>(
        &mut self,
        selector: StoreSelector<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.edit.bind_selector(selector, property)
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
    /// Declare the complete direct list without independent child regions.
    pub fn components<C: Component>(
        &mut self,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) -> Result<Vec<ScopeId>, Error> {
        self.edit.components::<C>(self.scope, host, items)
    }

    /// Declare this component's complete ordered list of independent regions.
    pub fn component_sites(&mut self, keys: &[u64]) -> Result<Vec<ComponentSite>, Error> {
        self.edit.component_sites(self.scope, keys)
    }

    /// Update one of this component's direct regions without touching siblings.
    pub fn components_at<C: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) -> Result<Vec<ScopeId>, Error> {
        let mut attempt = self.edit.component_attempt()?;
        if self.edit.scope(site.scope)?.parent != Some(self.scope) {
            return Err(Error::InvalidComponent);
        }
        let scopes = self.edit.components_at::<C>(site, host, items)?;
        attempt.success = true;
        Ok(scopes)
    }

    /// Batch heterogeneous declarations for this component's direct regions.
    pub fn apply_components(&mut self, plan: ComponentPlan) -> Result<(), Error> {
        let mut attempt = self.edit.component_attempt()?;
        plan.validate_parent(self.scope, self.edit)?;
        self.edit.apply_components(plan)?;
        attempt.success = true;
        Ok(())
    }

    /// Register source-driven selection over this component's owned render host.
    pub fn component_producer(
        &mut self,
        key: u64,
        host: NodeId,
        after: &[LayoutProducerId],
        producer: ComponentProducer,
    ) -> Result<LayoutProducerId, Error> {
        self.edit
            .component_producer(self.scope, key, host, after, producer)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Identity {
    Component { kind: TypeId, host: NodeId },
    Site,
}

pub(super) struct Attempt {
    failed: Rc<Cell<bool>>,
    pub(super) success: bool,
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.success {
            self.failed.set(true);
        }
    }
}
impl LogicalEdit<'_, '_, '_> {
    pub(super) fn component_attempt(&mut self) -> Result<Attempt, Error> {
        self.validate_components()?;
        Ok(Attempt {
            failed: self
                .component_failure
                .get_or_insert_with(|| Rc::new(Cell::new(false)))
                .clone(),
            success: false,
        })
    }
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
    /// Reconcile the complete ordered list of logical regions under parent.
    /// Each returned site owns a separate child-key namespace. Updating content
    /// later does not require redeclaring these sites; an omitted site retires
    /// its entire subtree. No layout box, output, source or producer is added.
    /// Manual/typed component scopes with equal keys are replaced, not adopted.
    pub fn component_sites(
        &mut self,
        parent: ScopeId,
        keys: &[u64],
    ) -> Result<Vec<ComponentSite>, Error> {
        let mut attempt = self.component_attempt()?;
        self.scope(parent)?;
        if self.retired_scopes()?.contains(&parent) {
            return Err(Error::InvalidScope);
        }
        let scopes = self.reconcile_scopes(parent, keys, Some(Identity::Site))?;
        attempt.success = true;
        Ok(scopes
            .into_iter()
            .map(|scope| ComponentSite { scope })
            .collect())
    }

    /// Reconcile one independent homogeneous list/selected arm. Equal item keys
    /// in other sites are unrelated. A changed component type or arm key mounts
    /// fresh; an empty declaration unmounts the active contents, retaining site.
    /// Props settle together within this call, not across separate site calls.
    /// Content scheduling still uses the existing host-writer/ordering contract.
    pub fn components_at<C: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) -> Result<Vec<ScopeId>, Error> {
        let mut attempt = self.component_attempt()?;
        if self.scope(site.scope)?.component.as_deref() != Some(&Identity::Site) {
            return Err(Error::InvalidComponent);
        }
        let scopes = self.components::<C>(site.scope, host, items)?;
        attempt.success = true;
        Ok(scopes)
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
        let mut attempt = self.component_attempt()?;
        let mut pending =
            self.prepare_components::<C>(parent, host, items.into_iter().collect())?;
        let scopes = pending.scopes.clone();
        self.props(|batch| pending.stage(batch))?;
        pending.mount(self)?;
        self.validate_components()?;
        attempt.success = true;
        Ok(scopes)
    }
    fn prepare_components<C: Component>(
        &mut self,
        parent: ScopeId,
        host: NodeId,
        rows: Vec<(u64, C::Props)>,
    ) -> Result<Prepared<C>, Error> {
        self.scope(parent)?;
        if self.retired_scopes()?.contains(&parent) {
            return Err(Error::InvalidScope);
        }
        // Reject a stale/removed host before running mount code.
        self.render.tree().children(host)?;
        if self.render.tree().removed_ids()?.contains(&host) {
            return Err(Error::InvalidNode);
        }
        let keys: Vec<_> = rows.iter().map(|r| r.0).collect();
        let identity = Identity::Component {
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
        Ok(Prepared {
            host,
            previous,
            scopes,
            values: rows.into_iter().map(|(_, props)| props).collect(),
            inputs: vec![],
        })
    }
}

/// Shared by the original single-list path and heterogeneous plans. Preparation
/// only reconciles scopes; all values stage before any mount or source read.
struct Prepared<C: Component> {
    host: NodeId,
    previous: BTreeSet<ScopeId>,
    scopes: Vec<ScopeId>,
    values: Vec<C::Props>,
    inputs: Vec<ReadProp<C::Props>>,
}
impl<C: Component> Prepared<C> {
    fn stage(&mut self, batch: &mut PropBatch<'_, '_, '_, '_>) -> Result<(), Error> {
        for (scope, props) in self
            .scopes
            .iter()
            .copied()
            .zip(std::mem::take(&mut self.values))
        {
            self.inputs.push(batch.prop(scope, u64::MAX, props)?);
        }
        Ok(())
    }
    fn mount(self, edit: &mut LogicalEdit<'_, '_, '_>) -> Result<(), Error> {
        for (scope, props) in self.scopes.into_iter().zip(self.inputs) {
            if !self.previous.contains(&scope) {
                C::mount(
                    props,
                    &mut ComponentContext {
                        edit,
                        scope,
                        host: self.host,
                    },
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod site_tests;
#[cfg(test)]
mod tests;
