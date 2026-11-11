//! Component scopes have identities and source lifetimes independent of render nodes.
use super::{
    reactive::{
        BindingId, ComputedInputs, Dependency, EffectCleanup, EffectId, EffectInputs, Property,
        ReactiveCommit, ReactiveTransaction, ReactiveUi, ScopedSource, Signal,
    },
    *,
};
use std::collections::BTreeSet;
mod edit;
pub use edit::LogicalEdit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeId {
    owner: u64,
    slot: usize,
    generation: u64,
}

struct Scope {
    parent: Option<ScopeId>,
    key: u64,
    children: Vec<ScopeId>,
    sources: BTreeMap<u64, Box<dyn ScopedSource>>,
    effects: BTreeMap<u64, EffectId>,
    renders: Vec<NodeId>,
}
struct Slot {
    generation: u64,
    scope: Option<Scope>,
}

pub struct ScopeChanges {
    pub order: Vec<ScopeId>,
    pub inserted: Vec<ScopeId>,
    pub removed: Vec<ScopeId>,
    pub request_frame: bool,
}

/// Atomic existing-scope sibling reorders and final render-host moves. Scope
/// lifetime/ownership stays stable; creation and disposal are separate APIs.
#[derive(Default)]
pub struct LogicalStructureTransaction {
    scopes: BTreeMap<ScopeId, Vec<ScopeId>>,
    hosts: BTreeMap<NodeId, NodeId>,
}
impl LogicalStructureTransaction {
    pub fn reorder(&mut self, parent: ScopeId, children: Vec<ScopeId>) {
        self.scopes.insert(parent, children);
    }
    pub fn reparent(&mut self, node: NodeId, host: NodeId) {
        self.hosts.insert(node, host);
    }
}

/// Logical ownership can contain zero or several render nodes. Render parentage
/// is explicit: rendering into another host is a portal, without changing scope
/// parentage or source ownership. Keys identify direct children of one scope.
pub struct LogicalUi {
    ui: ReactiveUi,
    identity: u64,
    scopes: Vec<Slot>,
    free: Vec<usize>,
    root: ScopeId,
    render_owners: BTreeMap<NodeId, ScopeId>,
    next_scope_generation: u64,
}

impl LogicalUi {
    pub fn open_updates(&mut self, capacity: usize) -> Result<reactive::UpdateReceiver, Error> {
        self.ui.open_updates(capacity)
    }
    pub fn update_sender<T: PartialEq + Send + 'static>(
        &mut self,
        receiver: &reactive::UpdateReceiver,
        signal: Signal<T>,
    ) -> Result<reactive::UpdateSender<T>, Error> {
        self.ui.update_sender(receiver, signal)
    }
    pub fn apply_updates(
        &mut self,
        receiver: &mut reactive::UpdateReceiver,
    ) -> Result<reactive::UpdateBatch, Error> {
        self.ui.apply_updates(receiver)
    }

    pub fn new(style: Style, color: u32, capabilities: BackendCapabilities) -> Result<Self, Error> {
        Self::from_tree(UiTree::new(style, color)?, capabilities)
    }

    /// Adopt existing nodes as root-scope output without rebuilding render identity.
    pub fn from_tree(tree: UiTree, capabilities: BackendCapabilities) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let identity = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| Error::RevisionExhausted)?;
        let renders: Vec<_> = tree
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                entry.node.as_ref().map(|_| NodeId {
                    tree: tree.identity,
                    slot,
                    generation: entry.generation,
                })
            })
            .collect();
        let ui = ReactiveUi::new(tree, capabilities)?;
        let root = ScopeId {
            owner: identity,
            slot: 0,
            generation: 0,
        };
        let render_owners = renders.iter().map(|node| (*node, root)).collect();
        let owned = renders
            .into_iter()
            .filter(|node| *node != ui.root())
            .collect();
        Ok(Self {
            ui,
            identity,
            scopes: vec![Slot {
                generation: 0,
                scope: Some(Scope {
                    parent: None,
                    key: 0,
                    children: vec![],
                    sources: BTreeMap::new(),
                    effects: BTreeMap::new(),
                    renders: owned,
                }),
            }],
            free: vec![],
            root,
            render_owners,
            next_scope_generation: 1,
        })
    }

    pub fn root_scope(&self) -> ScopeId {
        self.root
    }
    pub fn root_render(&self) -> NodeId {
        self.ui.root()
    }
    pub fn reactive(&self) -> &ReactiveUi {
        &self.ui
    }
    pub fn published_frame(&self) -> Option<&FramePacket> {
        self.ui.published_frame()
    }
    pub fn stats(&self) -> reactive::ReactiveStats {
        self.ui.stats()
    }
    pub fn set_capabilities(&mut self, capabilities: BackendCapabilities) {
        self.ui.set_capabilities(capabilities);
    }
    pub fn frame(&mut self, viewport: [f32; 2]) -> Result<FramePacket, Error> {
        self.ui.frame(viewport)
    }
    pub fn children(&self, scope: ScopeId) -> Result<&[ScopeId], Error> {
        Ok(&self.scope(scope)?.children)
    }
    pub fn renders(&self, scope: ScopeId) -> Result<&[NodeId], Error> {
        Ok(&self.scope(scope)?.renders)
    }
    pub fn key(&self, scope: ScopeId) -> Result<u64, Error> {
        Ok(self.scope(scope)?.key)
    }
    pub fn parent(&self, scope: ScopeId) -> Result<Option<ScopeId>, Error> {
        Ok(self.scope(scope)?.parent)
    }

    fn scope(&self, id: ScopeId) -> Result<&Scope, Error> {
        if id.owner != self.identity {
            return Err(Error::InvalidScope);
        }
        self.scopes
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.scope.as_ref())
            .ok_or(Error::InvalidScope)
    }
    fn scope_mut(&mut self, id: ScopeId) -> &mut Scope {
        self.scopes[id.slot]
            .scope
            .as_mut()
            .expect("validated scope")
    }

    /// Named state is initialized once per scope lifetime and retains its value
    /// across keyed reorder. Reusing a state key with a different type is an error.
    pub fn state<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        init: impl FnOnce() -> T,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self.scope(scope)?.sources.get(&key) {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.ui.validate_scoped_source(signal, None)?;
            return Ok(signal);
        }
        let source = self.ui.try_signal(init())?;
        self.scope_mut(scope).sources.insert(key, Box::new(source));
        Ok(source)
    }

    /// A keyed derived source keeps its original pure computation for the
    /// scope lifetime. Reuse requires the same type and dependency identities.
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self.scope(scope)?.sources.get(&key) {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.ui.validate_scoped_source(signal, Some(dependencies))?;
            return Ok(signal);
        }
        let signal = self.ui.computed(dependencies, evaluate)?;
        self.scope_mut(scope).sources.insert(key, Box::new(signal));
        Ok(signal)
    }

    pub fn read<T: 'static>(&self, source: Signal<T>) -> Result<&T, Error> {
        self.ui.read(source)
    }

    /// Keyed post-commit observation. Setup runs once; subsequent key reuse
    /// retains the original callback and requires the same dependency set.
    pub fn effect(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        if let Some(id) = self.scope(scope)?.effects.get(&key) {
            self.ui.validate_scoped_effect(*id, dependencies)?;
            return Ok(*id);
        }
        let id = self.ui.effect(dependencies, observe)?;
        self.scope_mut(scope).effects.insert(key, id);
        Ok(id)
    }
    pub fn bind<T: 'static>(
        &mut self,
        source: Signal<T>,
        property: Property<T>,
    ) -> Result<(BindingId, Commit), Error> {
        self.ui.bind(source, property)
    }
    pub fn transaction<R>(
        &mut self,
        update: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<R, Error>,
    ) -> Result<ReactiveCommit<R>, Error> {
        self.ui.transaction(update)
    }

    pub fn append_render(
        &mut self,
        owner: ScopeId,
        parent: NodeId,
        style: Style,
        color: u32,
    ) -> Result<NodeId, Error> {
        self.scope(owner)?;
        let node = self.ui.append(parent, style, color)?;
        self.scope_mut(owner).renders.push(node);
        self.render_owners.insert(node, owner);
        Ok(node)
    }

    fn subtree(&self, root: ScopeId, output: &mut BTreeSet<ScopeId>) -> Result<(), Error> {
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            output.insert(id);
            stack.extend(&self.scope(id)?.children);
        }
        Ok(())
    }

    /// Reconcile keys, retain scope/state/render identity, retire absent subtrees
    /// and update sibling render order. Construction of inserted scopes is explicit.
    pub fn reconcile_keys(&mut self, parent: ScopeId, keys: &[u64]) -> Result<ScopeChanges, Error> {
        let children = self.scope(parent)?.children.clone();
        let unique: BTreeSet<_> = keys.iter().copied().collect();
        if unique.len() != keys.len() {
            return Err(Error::DuplicateKey);
        }
        let old: BTreeMap<_, _> = children
            .iter()
            .map(|id| (self.scope(*id).unwrap().key, *id))
            .collect();
        if old.len() == keys.len() && keys.iter().all(|key| old.contains_key(key)) {
            let order: Vec<_> = keys.iter().map(|key| old[key]).collect();
            let mut tx = LogicalStructureTransaction::default();
            tx.reorder(parent, order.clone());
            let commit = self.commit_structure(tx)?;
            return Ok(ScopeChanges {
                order,
                inserted: vec![],
                removed: vec![],
                request_frame: commit.request_frame,
            });
        }
        let mut removed = BTreeSet::new();
        for (key, id) in &old {
            if !unique.contains(key) {
                self.subtree(*id, &mut removed)?;
            }
        }
        let dead_sources = removed
            .iter()
            .flat_map(|id| {
                self.scope(*id)
                    .unwrap()
                    .sources
                    .values()
                    .map(|source| source.dependency())
            })
            .collect();
        let dead_effects = removed
            .iter()
            .flat_map(|id| self.scope(*id).unwrap().effects.values().copied())
            .collect();
        self.ui
            .validate_scope_disposal(&dead_sources, &dead_effects)?;
        // A surviving portal cannot retain a host which is about to be destroyed.
        // Reject before modifying logical state, bindings or render topology.
        for (node, owner) in &self.render_owners {
            if removed.contains(owner) {
                continue;
            }
            let mut ancestor = self.ui.tree().node(*node)?.parent;
            while let Some(id) = ancestor {
                if self
                    .render_owners
                    .get(&id)
                    .is_some_and(|owner| removed.contains(owner))
                {
                    return Err(Error::ScopeInUse);
                }
                ancestor = self.ui.tree().node(id)?.parent;
            }
        }
        let mut roots = vec![];
        for (node, owner) in &self.render_owners {
            if !removed.contains(owner) {
                continue;
            }
            if self.ui.tree().node(*node)?.parent.is_none_or(|parent| {
                !self
                    .render_owners
                    .get(&parent)
                    .is_some_and(|owner| removed.contains(owner))
            }) {
                roots.push(*node);
            }
        }
        // Plan the complete surviving render order and all removals before
        // invoking user cleanup or changing logical generations/source state.
        let surviving_order: Vec<_> = keys
            .iter()
            .filter_map(|key| old.get(key).copied())
            .collect();
        let mut ranks = BTreeMap::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if removed.contains(&id) {
                continue;
            }
            ranks.insert(id, ranks.len());
            let children = if id == parent {
                &surviving_order
            } else {
                &self.scope(id)?.children
            };
            stack.extend(children.iter().rev());
        }
        let mut render_tx = StructureTransaction::default();
        for node in roots {
            render_tx.remove(node);
        }
        let mut hosts = BTreeSet::new();
        for (node, owner) in &self.render_owners {
            if !removed.contains(owner)
                && let Some(host) = self.ui.tree().node(*node)?.parent
            {
                hosts.insert(host);
            }
        }
        for host in hosts {
            let mut children: Vec<_> = self
                .ui
                .tree()
                .node(host)?
                .children
                .iter()
                .filter(|child| !removed.contains(&self.render_owners[child]))
                .copied()
                .collect();
            children.sort_by_key(|node| ranks[&self.render_owners[node]]);
            render_tx.children(host, children);
        }
        let count = keys.iter().filter(|key| !old.contains_key(key)).count() as u64;
        let generation_end = self
            .next_scope_generation
            .checked_add(count)
            .ok_or(Error::RevisionExhausted)?;
        let prepared = self.ui.prepare_structure(render_tx)?;
        // Cleanup observers before retiring any of their dependencies, including
        // inputs owned by other scopes in the same removal set.
        for effect in dead_effects.iter().rev() {
            self.ui.dispose_effect(*effect)?;
        }
        let request_frame = self.ui.apply_structure(prepared).request_frame;
        self.render_owners
            .retain(|_, owner| !removed.contains(owner));
        let mut dead_sources = vec![];
        for id in &removed {
            let scope = self.scopes[id.slot].scope.take().expect("removed scope");
            dead_sources.extend(scope.sources.into_values());
            let slot = &mut self.scopes[id.slot];
            if let Some(next) = slot.generation.checked_add(1) {
                slot.generation = next;
                self.free.push(id.slot);
            }
        }
        self.ui.dispose_scope_sources(dead_sources);
        let mut inserted = vec![];
        let mut order = vec![];
        for key in keys {
            if let Some(id) = old.get(key) {
                order.push(*id);
                continue;
            }
            let slot = self.free.pop().unwrap_or(self.scopes.len());
            if slot == self.scopes.len() {
                self.scopes.push(Slot {
                    generation: 0,
                    scope: None,
                });
            }
            let entry = &mut self.scopes[slot];
            let generation = self.next_scope_generation;
            self.next_scope_generation += 1;
            entry.generation = generation;
            let id = ScopeId {
                owner: self.identity,
                slot,
                generation: entry.generation,
            };
            entry.scope = Some(Scope {
                parent: Some(parent),
                key: *key,
                children: vec![],
                sources: BTreeMap::new(),
                effects: BTreeMap::new(),
                renders: vec![],
            });
            order.push(id);
            inserted.push(id);
        }
        debug_assert_eq!(self.next_scope_generation, generation_end);
        self.scope_mut(parent).children = order.clone();
        Ok(ScopeChanges {
            order,
            inserted,
            removed: removed.into_iter().collect(),
            request_frame,
        })
    }

    /// Sort owned siblings by logical preorder, preserving each scope's render
    /// insertion order. Portal nodes keep their explicit render host.
    pub fn sync_render_order(&mut self) -> Result<bool, Error> {
        Ok(self
            .commit_structure(LogicalStructureTransaction::default())?
            .request_frame)
    }

    pub fn reparent_render(&mut self, node: NodeId, host: NodeId) -> Result<Commit, Error> {
        let mut tx = LogicalStructureTransaction::default();
        tx.reparent(node, host);
        self.commit_structure(tx)
    }

    /// Commit sibling scope permutations and portal moves together. Logical
    /// identity/source/effect ownership survives. A failed final render graph
    /// publishes neither logical order nor render parentage.
    pub fn commit_structure(&mut self, tx: LogicalStructureTransaction) -> Result<Commit, Error> {
        for (parent, children) in &tx.scopes {
            let current = &self.scope(*parent)?.children;
            let mut a = current.clone();
            let mut b = children.clone();
            for id in children {
                self.scope(*id)?;
            }
            a.sort_unstable();
            b.sort_unstable();
            if a != b {
                return Err(Error::InvalidChildren);
            }
        }
        let mut ranks = BTreeMap::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            ranks.insert(id, ranks.len());
            stack.extend(
                tx.scopes
                    .get(&id)
                    .unwrap_or(&self.scope(id)?.children)
                    .iter()
                    .rev(),
            );
        }
        let mut hosts = BTreeMap::new();
        for node in self.render_owners.keys() {
            if let Some(parent) = self.ui.tree().node(*node)?.parent {
                hosts
                    .entry(parent)
                    .or_insert_with(|| self.ui.tree().node(parent).unwrap().children.clone());
            }
        }
        for (node, host) in &tx.hosts {
            self.ui.tree().node(*host)?;
            let previous = self
                .ui
                .tree()
                .node(*node)?
                .parent
                .ok_or(Error::CannotRemoveRoot)?;
            if previous == *host {
                continue;
            }
            hosts
                .get_mut(&previous)
                .expect("existing render host")
                .retain(|id| id != node);
            hosts.entry(*host).or_default().push(*node);
        }
        let mut render_tx = StructureTransaction::default();
        for (host, mut children) in hosts {
            children.sort_by_key(|node| ranks[&self.render_owners[node]]);
            render_tx.children(host, children);
        }
        let commit = self.ui.commit_structure(render_tx)?;
        for (parent, children) in tx.scopes {
            self.scope_mut(parent).children = children;
        }
        Ok(commit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> LogicalUi {
        LogicalUi::new(
            Style {
                size: Size {
                    width: length(160.0),
                    height: length(80.0),
                },
                ..Default::default()
            },
            0x171d28ff,
            BackendCapabilities::default(),
        )
        .unwrap()
    }

    #[test]
    fn scoped_effects_retain_observation_and_cleanup_before_cross_scope_inputs_retire() {
        use std::{cell::RefCell, rc::Rc};
        let mut ui = fixture();
        let root = ui.root_scope();
        let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
        let input = ui.state(scopes[0], 0, || 0u32).unwrap();
        let events = Rc::new(RefCell::new(vec![]));
        let log = events.clone();
        let effect = ui
            .effect(scopes[1], 0, &[input.dependency()], move |i| {
                let value = *i.read(input).unwrap();
                log.borrow_mut().push(("run", value));
                let log = log.clone();
                Some(Box::new(move || log.borrow_mut().push(("cleanup", value))))
            })
            .unwrap();
        ui.reconcile_keys(root, &[2, 1]).unwrap();
        assert_eq!(
            ui.effect(scopes[1], 0, &[input.dependency()], |_| panic!(
                "retained callback"
            ))
            .unwrap(),
            effect
        );
        assert!(matches!(
            ui.effect(scopes[1], 0, &[], |_| None),
            Err(Error::InvalidEffect)
        ));
        assert!(matches!(
            ui.reconcile_keys(root, &[2]),
            Err(Error::SignalInUse)
        ));
        assert_eq!(*events.borrow(), [("run", 0)]);
        ui.transaction(|tx| {
            tx.set(input, 1)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(*events.borrow(), [("run", 0), ("cleanup", 0), ("run", 1)]);
        ui.reconcile_keys(root, &[]).unwrap();
        assert_eq!(events.borrow().last(), Some(&("cleanup", 1)));
        assert_eq!(ui.reactive().effect_count(), 0);
        assert_eq!(ui.read(input), Err(Error::InvalidSignal));
        drop(ui);
        assert_eq!(events.borrow().len(), 4);
    }

    #[test]
    fn keyed_computed_sources_retain_edges_and_dispose_in_dependency_order() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
        let input = ui.state(scopes[0], 99, || 1u32).unwrap();
        let derived = ui
            .computed(scopes[1], 0, &[input.dependency()], move |i| {
                Ok(*i.read(input)? * 2)
            })
            .unwrap();
        let leaf = ui
            .computed(scopes[1], 1, &[derived.dependency()], move |i| {
                Ok(*i.read(derived)? + 1)
            })
            .unwrap();
        let node = ui
            .append_render(scopes[1], ui.root_render(), Style::default(), 0)
            .unwrap();
        ui.bind(leaf, Property::color(node)).unwrap();
        let before = ui.frame([160.0, 80.0]).unwrap();
        assert!(matches!(
            ui.reconcile_keys(root, &[2]),
            Err(Error::SignalInUse)
        ));
        assert_eq!(ui.children(root).unwrap(), scopes);
        assert_eq!(ui.reactive().source_count(), 3);
        assert_eq!(ui.reactive().binding_count(), 1);
        assert_eq!(ui.frame([160.0, 80.0]).unwrap().revision, before.revision);
        ui.reconcile_keys(root, &[2, 1]).unwrap();
        assert_eq!(
            ui.computed(scopes[1], 0, &[input.dependency()], |_| panic!(
                "must retain original callback"
            ))
            .unwrap(),
            derived
        );
        assert!(matches!(
            ui.state(scopes[1], 0, || 0u32),
            Err(Error::InvalidSignal)
        ));
        assert!(matches!(
            ui.computed(scopes[0], 99, &[], |_| Ok(0u32)),
            Err(Error::InvalidSignal)
        ));
        assert!(matches!(
            ui.computed(scopes[1], 0, &[leaf.dependency()], |_| Ok(0u32)),
            Err(Error::InvalidSignal)
        ));
        ui.transaction(|tx| {
            tx.set(input, 4)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(*ui.read(leaf).unwrap(), 9);
        // All removed scopes are disposed as one topologically sorted set.
        ui.reconcile_keys(root, &[]).unwrap();
        assert_eq!(ui.reactive().source_count(), 0);
        assert_eq!(ui.reactive().binding_count(), 0);
        assert_eq!(ui.read(input), Err(Error::InvalidSignal));
        assert_eq!(ui.read(derived), Err(Error::InvalidSignal));
        assert_eq!(ui.read(leaf), Err(Error::InvalidSignal));
    }

    #[test]
    fn keyed_reorder_preserves_scopes_sources_render_ids_and_chunks() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let first = ui.reconcile_keys(root, &[10, 20, 30]).unwrap();
        assert!(!first.request_frame);
        let mut states = vec![];
        let mut nodes = vec![];
        for scope in &first.order {
            let color = ui.state(*scope, 7, || 0x334455ff).unwrap();
            let node = ui
                .append_render(*scope, ui.root_render(), Style::default(), 0x334455ff)
                .unwrap();
            ui.bind(color, Property::color(node)).unwrap();
            states.push(color);
            nodes.push(node);
        }
        let before = ui.frame([160.0, 80.0]).unwrap();
        let moved = ui.reconcile_keys(root, &[30, 10, 20]).unwrap();
        assert_eq!(
            moved.order,
            vec![first.order[2], first.order[0], first.order[1]]
        );
        assert!(moved.inserted.is_empty() && moved.removed.is_empty());
        assert!(moved.request_frame);
        assert_eq!(
            ui.state(first.order[0], 7, || panic!(
                "retained state must not initialize"
            ))
            .unwrap(),
            states[0]
        );
        let after = ui.frame([160.0, 80.0]).unwrap();
        assert_eq!(
            after
                .items
                .iter()
                .skip(1)
                .map(|item| item.node)
                .collect::<Vec<_>>(),
            vec![nodes[2], nodes[0], nodes[1]]
        );
        for item in after.items.iter().skip(1) {
            let old = before
                .items
                .iter()
                .find(|old| old.node == item.node)
                .unwrap();
            assert!(Arc::ptr_eq(&old.commands, &item.commands));
        }
        assert!(
            !ui.reconcile_keys(root, &[30, 10, 20])
                .unwrap()
                .request_frame
        );
        assert!(ui.state::<String>(first.order[0], 7, String::new).is_err());
    }

    #[test]
    fn removing_scope_retires_owned_state_and_portal_renders_but_keeps_shared_sources() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
        let nested = ui.reconcile_keys(scopes[0], &[9]).unwrap().order[0];
        let shared = ui.state(root, 0, || 0x112233ff).unwrap();
        let state = ui.state(nested, 0, || 0x556677ff).unwrap();
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap();
        let portal = ui
            .append_render(nested, host, Style::default(), 0x556677ff)
            .unwrap();
        ui.bind(state, Property::color(portal)).unwrap();
        let survivor = ui
            .append_render(scopes[1], ui.root_render(), Style::default(), 0x112233ff)
            .unwrap();
        ui.bind(shared, Property::color(survivor)).unwrap();
        let before = ui.frame([160.0, 80.0]).unwrap();
        let change = ui.reconcile_keys(root, &[2, 3]).unwrap();
        assert!(change.removed.contains(&nested));
        assert_eq!(ui.read(state), Err(Error::InvalidSignal));
        assert_eq!(*ui.read(shared).unwrap(), 0x112233ff);
        assert_eq!(ui.reactive().binding_count(), 1);
        assert!(ui.renders(nested).is_err());
        let after = ui.frame([160.0, 80.0]).unwrap();
        assert!(!after.items.iter().any(|item| item.node == portal));
        assert!(before.items.iter().any(|item| item.node == portal));
        let replacement = ui.state(change.inserted[0], 0, || 42u32).unwrap();
        assert_ne!(replacement, state);
    }

    #[test]
    fn duplicate_keys_foreign_scopes_and_live_portal_host_removal_are_rejected() {
        let mut ui = fixture();
        let root = ui.root_scope();
        let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
        let host = ui
            .append_render(scopes[0], ui.root_render(), Style::default(), 0)
            .unwrap();
        let portal = ui
            .append_render(scopes[1], host, Style::default(), 0)
            .unwrap();
        let before = ui.frame([160.0, 80.0]).unwrap();
        assert!(matches!(
            ui.reconcile_keys(root, &[2, 2]),
            Err(Error::DuplicateKey)
        ));
        assert!(matches!(
            ui.reconcile_keys(root, &[2]),
            Err(Error::ScopeInUse)
        ));
        assert_eq!(ui.children(root).unwrap(), scopes);
        assert!(Arc::ptr_eq(
            &before.items,
            &ui.frame([160.0, 80.0]).unwrap().items
        ));
        let other = fixture();
        assert!(matches!(
            ui.reconcile_keys(other.root_scope(), &[]),
            Err(Error::InvalidScope)
        ));
        // Removing both owners is safe regardless of logical/render parentage.
        ui.reconcile_keys(root, &[]).unwrap();
        assert!(ui.ui.tree().node(portal).is_err());
        assert_eq!(ui.frame([160.0, 80.0]).unwrap().items.len(), 1);
    }
}
