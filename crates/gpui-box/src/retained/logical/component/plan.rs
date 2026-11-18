//! Selected typed arms and lists share one source checkpoint and mount boundary.
use super::*;

trait Declaration {
    fn site(&self) -> ComponentSite;
    fn prepare(
        self: Box<Self>,
        edit: &mut LogicalEdit<'_, '_, '_>,
    ) -> Result<Box<dyn Pending>, Error>;
}
trait Pending {
    fn stage(&mut self, batch: &mut PropBatch<'_, '_, '_, '_>) -> Result<(), Error>;
    fn mount(self: Box<Self>, edit: &mut LogicalEdit<'_, '_, '_>) -> Result<(), Error>;
}
struct List<C: Component> {
    site: ComponentSite,
    host: NodeId,
    items: Vec<(u64, C::Props)>,
}
impl<C: Component> Declaration for List<C> {
    fn site(&self) -> ComponentSite {
        self.site
    }
    fn prepare(
        self: Box<Self>,
        edit: &mut LogicalEdit<'_, '_, '_>,
    ) -> Result<Box<dyn Pending>, Error> {
        Ok(Box::new(edit.prepare_components::<C>(
            self.site.scope,
            self.host,
            self.items,
        )?))
    }
}
impl<C: Component> Pending for Prepared<C> {
    fn stage(&mut self, batch: &mut PropBatch<'_, '_, '_, '_>) -> Result<(), Error> {
        self.stage(batch)
    }
    fn mount(self: Box<Self>, edit: &mut LogicalEdit<'_, '_, '_>) -> Result<(), Error> {
        (*self).mount(edit)
    }
}
struct Empty;
impl Component for Empty {
    type Props = ();
    fn mount(_: ReadProp<()>, _: &mut ComponentContext<'_, '_, '_, '_>) -> Result<(), Error> {
        Err(Error::InvalidComponent) // Empty declarations never contain instances.
    }
}

/// Owned heterogeneous declarations. The plan performs no reconciliation,
/// source writes, layout or mounts; payload factories may read selection inputs.
/// Apply once to reconcile all selected sites,
/// settle all their payloads together, then initialize only fresh instances.
/// Unmentioned sites retain their existing contents; repeated sites reject.
#[derive(Default)]
pub struct ComponentPlan {
    declarations: Vec<Box<dyn Declaration>>,
    invalid: bool,
}
impl ComponentPlan {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn list<C: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        items: impl IntoIterator<Item = (u64, C::Props)>,
    ) {
        self.declarations.push(Box::new(List::<C> {
            site,
            host,
            items: items.into_iter().collect(),
        }));
    }
    /// Select one match arm. Use distinct stable arm keys for different source
    /// arms, even when they instantiate the same component type. Same arm/type/
    /// host replaces Props without remounting; a different identity unmounts.
    pub fn select<C: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        arm: u64,
        props: C::Props,
    ) {
        self.list::<C>(site, host, [(arm, props)]);
    }
    /// Evaluate only the chosen payload factory. Arm identities are 0=false,
    /// 1=true; unchanged selection keeps the existing component/local state.
    pub fn branch<T: Component, F: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        condition: bool,
        when_true: impl FnOnce() -> Result<T::Props, Error>,
        when_false: impl FnOnce() -> Result<F::Props, Error>,
    ) -> Result<(), Error> {
        let was_invalid = self.invalid;
        self.invalid = true; // Also retain failure if a factory unwinds and is caught.
        if condition {
            self.select::<T>(site, host, 1, when_true()?);
        } else {
            self.select::<F>(site, host, 0, when_false()?);
        }
        self.invalid = was_invalid;
        Ok(())
    }
    /// Conditional content without an else arm. False retains an empty site and
    /// never invokes the payload factory; true uses the stable arm key1.
    pub fn show<C: Component>(
        &mut self,
        site: ComponentSite,
        host: NodeId,
        condition: bool,
        props: impl FnOnce() -> Result<C::Props, Error>,
    ) -> Result<(), Error> {
        let was_invalid = self.invalid;
        self.invalid = true;
        if condition {
            self.select::<C>(site, host, 1, props()?);
        } else {
            self.empty(site, host);
        }
        self.invalid = was_invalid;
        Ok(())
    }
    /// Unmount active content, retaining its logical region for later selection.
    pub fn empty(&mut self, site: ComponentSite, host: NodeId) {
        self.list::<Empty>(site, host, []);
    }

    pub(super) fn validate_parent(
        &self,
        parent: ScopeId,
        edit: &LogicalEdit<'_, '_, '_>,
    ) -> Result<(), Error> {
        for declaration in &self.declarations {
            if edit.scope(declaration.site().scope)?.parent != Some(parent) {
                return Err(Error::InvalidComponent);
            }
        }
        Ok(())
    }
}

/// Read-only source capability during component selection. Reads feed the
/// ordinary Content producer observation index; conditional reads replace the
/// previous successful observation. No edit/layout/mount capability is exposed.
///
/// ```compile_fail
/// use gpui_box::retained::logical::ComponentInputs;
/// fn mutate(inputs: &ComponentInputs<'_, '_, '_, '_>) { inputs.properties(); }
/// ```
pub struct ComponentInputs<'a, 'edit, 'draft, 'tree> {
    edit: &'a LogicalEdit<'edit, 'draft, 'tree>,
}
impl ComponentInputs<'_, '_, '_, '_> {
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        self.edit.read(signal)
    }
    pub fn read_prop<T: 'static>(&self, prop: ReadProp<T>) -> Result<&T, Error> {
        self.edit.read_prop(prop)
    }
}

/// Source-driven typed selection/list construction over one owned render host.
/// Clone a handle for equal redeclaration; replacing it invalidates observations.
/// Selectors/payload factories must be pure and owner-local. Existing producer
/// source/order/final-candidate validation applies without a second scheduler.
#[derive(Clone)]
pub struct ComponentProducer(LayoutProducer);
impl ComponentProducer {
    pub fn new(
        select: impl Fn(&ComponentInputs<'_, '_, '_, '_>) -> Result<ComponentPlan, Error> + 'static,
    ) -> Self {
        Self(LayoutProducer::new(move |_, edit| {
            let plan = select(&ComponentInputs { edit })?;
            edit.apply_components(plan)
        }))
    }
}
impl LogicalEdit<'_, '_, '_> {
    /// Apply heterogeneous final declarations in three phases: reconcile all,
    /// one shared prop batch, then mount new instances. Errors/unwind poison
    /// the complete edit. New mount effects run only after final publication.
    pub fn apply_components(&mut self, plan: ComponentPlan) -> Result<(), Error> {
        let mut attempt = self.component_attempt()?;
        if plan.invalid {
            return Err(Error::InvalidComponent);
        }
        let retired = self.retired_scopes()?;
        let mut sites = BTreeSet::new();
        for declaration in &plan.declarations {
            let site = declaration.site().scope;
            if !sites.insert(site) {
                return Err(Error::DuplicateKey);
            }
            if self.scope(site)?.component.as_deref() != Some(&Identity::Site)
                || retired.contains(&site)
            {
                return Err(Error::InvalidComponent);
            }
        }
        // Nested sites cannot be written in the same batch: selecting an outer
        // arm can retire the inner site. Its own producer/mount declares it.
        for &site in &sites {
            let mut parent = self.scope(site)?.parent;
            while let Some(id) = parent {
                if sites.contains(&id) {
                    return Err(Error::InvalidComponent);
                }
                parent = self.scope(id)?.parent;
            }
        }
        let mut pending = plan
            .declarations
            .into_iter()
            .map(|declaration| declaration.prepare(self))
            .collect::<Result<Vec<_>, Error>>()?;
        self.props(|batch| {
            for declaration in &mut pending {
                declaration.stage(batch)?;
            }
            Ok(())
        })?;
        for declaration in pending {
            declaration.mount(self)?;
        }
        self.validate_components()?;
        attempt.success = true;
        Ok(())
    }
    /// Register automatic final-source selection as an ordinary Content producer.
    /// One writer per host and explicit after dependencies retain their meaning.
    pub fn component_producer(
        &mut self,
        scope: ScopeId,
        key: u64,
        host: NodeId,
        after: &[LayoutProducerId],
        producer: ComponentProducer,
    ) -> Result<LayoutProducerId, Error> {
        self.layout_producer(
            scope,
            key,
            host,
            LayoutBuildKind::Content,
            after,
            producer.0,
        )
    }
}

#[cfg(test)]
mod tests;
