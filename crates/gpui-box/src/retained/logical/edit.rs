//! Render construction retains independent, validated scope ownership.
use super::*;
use crate::retained::reactive::{ReactiveEdit, ReactiveEditCommit};

pub struct LogicalEdit<'a, 'draft, 'tree> {
    render: &'a mut ReactiveEdit<'draft, 'tree>,
    identity: u64,
    scopes: &'a [Slot],
    owners: &'a BTreeMap<NodeId, ScopeId>,
    inserted: BTreeMap<NodeId, ScopeId>,
    removals: BTreeSet<NodeId>,
    created_scopes: BTreeMap<ScopeId, Scope>,
    child_lists: BTreeMap<ScopeId, Vec<ScopeId>>,
    removed_scopes: BTreeSet<ScopeId>,
    added_sources: BTreeMap<(ScopeId, u64), Box<dyn ScopedSource>>,
    added_effects: BTreeMap<(ScopeId, u64), EffectId>,
    free_scopes: Option<Vec<usize>>,
    available_scopes: &'a [usize],
    scope_slots: usize,
    next_scope_generation: &'a mut u64,
}
impl LogicalEdit<'_, '_, '_> {
    fn scope(&self, id: ScopeId) -> Result<&Scope, Error> {
        if id.owner != self.identity {
            return Err(Error::InvalidScope);
        }
        self.created_scopes
            .get(&id)
            .or_else(|| {
                self.scopes
                    .get(id.slot)
                    .filter(|s| s.generation == id.generation)
                    .and_then(|s| s.scope.as_ref())
            })
            .ok_or(Error::InvalidScope)
    }
    pub fn children(&self, id: ScopeId) -> Result<&[ScopeId], Error> {
        Ok(self
            .child_lists
            .get(&id)
            .unwrap_or(&self.scope(id)?.children))
    }
    pub fn reconcile_keys(&mut self, parent: ScopeId, keys: &[u64]) -> Result<Vec<ScopeId>, Error> {
        self.scope(parent)?;
        if self.removed_scopes.contains(&parent) {
            return Err(Error::InvalidScope);
        }
        let unique: BTreeSet<_> = keys.iter().copied().collect();
        if unique.len() != keys.len() {
            return Err(Error::DuplicateKey);
        }
        let old: BTreeMap<_, _> = self
            .children(parent)?
            .iter()
            .map(|id| (self.scope(*id).unwrap().key, *id))
            .collect();
        self.next_scope_generation
            .checked_add(keys.iter().filter(|key| !old.contains_key(key)).count() as u64)
            .ok_or(Error::RevisionExhausted)?;
        let mut order = vec![];
        for key in keys {
            if let Some(id) = old.get(key) {
                order.push(*id);
                continue;
            }
            let generation = *self.next_scope_generation;
            *self.next_scope_generation =
                generation.checked_add(1).ok_or(Error::RevisionExhausted)?;
            let slot = self
                .free_scopes
                .get_or_insert_with(|| self.available_scopes.to_vec())
                .pop()
                .unwrap_or_else(|| {
                    let slot = self.scope_slots;
                    self.scope_slots += 1;
                    slot
                });
            let id = ScopeId {
                owner: self.identity,
                slot,
                generation,
            };
            self.created_scopes.insert(
                id,
                Scope {
                    parent: Some(parent),
                    key: *key,
                    children: vec![],
                    sources: BTreeMap::new(),
                    effects: BTreeMap::new(),
                    renders: vec![],
                },
            );
            order.push(id);
        }
        for (key, id) in old {
            if !unique.contains(&key) {
                self.removed_scopes.insert(id);
            }
        }
        self.child_lists.insert(parent, order.clone());
        Ok(order)
    }
    pub fn state<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        init: impl FnOnce() -> T,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self
            .added_sources
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.sources.get(&key))
        {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.render.validate_scoped_source(signal, None)?;
            return Ok(signal);
        }
        self.scope(scope)?;
        let signal = self.render.signal(init())?;
        self.added_sources.insert((scope, key), Box::new(signal));
        Ok(signal)
    }
    pub fn computed<T: PartialEq + 'static>(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        evaluate: impl Fn(&ComputedInputs<'_>) -> Result<T, Error> + 'static,
    ) -> Result<Signal<T>, Error> {
        if let Some(source) = self
            .added_sources
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.sources.get(&key))
        {
            let signal = source
                .as_any()
                .downcast_ref::<Signal<T>>()
                .copied()
                .ok_or(Error::InvalidSignal)?;
            self.render
                .validate_scoped_source(signal, Some(dependencies))?;
            return Ok(signal);
        }
        self.scope(scope)?;
        let signal = self.render.computed(dependencies, evaluate)?;
        self.added_sources.insert((scope, key), Box::new(signal));
        Ok(signal)
    }
    pub fn effect(
        &mut self,
        scope: ScopeId,
        key: u64,
        dependencies: &[Dependency],
        observe: impl Fn(&EffectInputs<'_>) -> Option<EffectCleanup> + 'static,
    ) -> Result<EffectId, Error> {
        if let Some(id) = self
            .added_effects
            .get(&(scope, key))
            .or_else(|| self.scope(scope).ok()?.effects.get(&key))
        {
            self.render.validate_scoped_effect(*id, dependencies)?;
            return Ok(*id);
        }
        self.scope(scope)?;
        let id = self.render.effect(dependencies, observe)?;
        self.added_effects.insert((scope, key), id);
        Ok(id)
    }
    pub fn append(
        &mut self,
        owner: ScopeId,
        parent: NodeId,
        style: Style,
        color: u32,
    ) -> Result<NodeId, Error> {
        self.scope(owner)?;
        let node = self.render.tree().append(parent, style, color)?;
        self.inserted.insert(node, owner);
        Ok(node)
    }
    pub fn reparent(&mut self, node: NodeId, host: NodeId) -> Result<(), Error> {
        self.render.tree().reparent(node, host)
    }
    /// Authorize removal for this output's owner. Foreign-owned descendants
    /// must first be moved out, or explicitly removed in this same edit.
    pub fn remove(&mut self, node: NodeId) -> Result<(), Error> {
        self.inserted
            .get(&node)
            .or_else(|| self.owners.get(&node))
            .ok_or(Error::InvalidNode)?;
        self.removals.insert(node);
        self.render.tree().remove(node)
    }
    pub fn set_children(&mut self, node: NodeId, children: Vec<NodeId>) -> Result<(), Error> {
        self.render.tree().set_children(node, children)
    }
    pub fn properties(&mut self) -> &mut Transaction {
        self.render.tree().properties()
    }
    pub fn read<T: 'static>(&self, signal: Signal<T>) -> Result<&T, Error> {
        self.render.read(signal)
    }
    pub fn bind<T: 'static>(
        &mut self,
        signal: Signal<T>,
        property: Property<T>,
    ) -> Result<BindingId, Error> {
        self.render.bind(signal, property)
    }
    pub fn unbind(&mut self, binding: BindingId) -> Result<(), Error> {
        self.render.unbind(binding)
    }
}
impl LogicalUi {
    /// Atomically edit keyed scope lifetimes, sources, effects, bindings and
    /// render output. All user validation completes before callbacks run.
    pub fn edit<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        let mut inserted = BTreeMap::new();
        let mut created_scopes = BTreeMap::new();
        let mut child_lists = BTreeMap::new();
        let mut removed_scopes = BTreeSet::new();
        let mut added_sources = BTreeMap::new();
        let mut added_effects = BTreeMap::new();
        let pending = self.ui.edit_deferred(sources, |value, render_draft| {
            let mut draft = LogicalEdit {
                render: render_draft,
                identity: self.identity,
                scopes: &self.scopes,
                owners: &self.render_owners,
                inserted: BTreeMap::new(),
                removals: BTreeSet::new(),
                created_scopes: BTreeMap::new(),
                child_lists: BTreeMap::new(),
                removed_scopes: BTreeSet::new(),
                added_sources: BTreeMap::new(),
                added_effects: BTreeMap::new(),
                free_scopes: None,
                available_scopes: &self.free,
                scope_slots: self.scopes.len(),
                next_scope_generation: &mut self.next_scope_generation,
            };
            let value = render(value, &mut draft)?;
            let mut dead = BTreeSet::new();
            let mut stack: Vec<_> = draft.removed_scopes.iter().copied().collect();
            while let Some(id) = stack.pop() {
                if dead.insert(id) {
                    stack.extend(draft.children(id)?);
                }
            }
            // Retire whole logical subtrees as a set. Dependency validation is
            // deferred to the reactive preflight, before cleanup or publication.
            for id in &dead {
                let dependencies: Vec<_> = draft
                    .scope(*id)?
                    .sources
                    .values()
                    .map(|s| s.dependency())
                    .chain(
                        draft
                            .added_sources
                            .range((*id, 0)..=(*id, u64::MAX))
                            .map(|(_, s)| s.dependency()),
                    )
                    .collect();
                let effects: Vec<_> = draft
                    .scope(*id)?
                    .effects
                    .values()
                    .copied()
                    .chain(
                        draft
                            .added_effects
                            .range((*id, 0)..=(*id, u64::MAX))
                            .map(|(_, e)| *e),
                    )
                    .collect();
                for effect in effects {
                    draft.render.dispose_effect(effect)?;
                }
                for dependency in dependencies {
                    draft.render.dispose_dependency(dependency)?;
                }
            }
            let output: Vec<_> = draft
                .owners
                .iter()
                .chain(draft.inserted.iter())
                .filter(|(_, owner)| dead.contains(owner))
                .map(|(node, _)| *node)
                .collect();
            draft
                .render
                .retire_output_bindings(&output.iter().copied().collect());
            for node in output {
                draft.removals.insert(node);
                draft.render.tree().remove(node)?;
            }
            draft.render.tree().removed_ids()?; // settle pending property child lists
            for root in &draft.removals {
                let owner = draft
                    .inserted
                    .get(root)
                    .or_else(|| draft.owners.get(root))
                    .unwrap();
                let mut visited = BTreeSet::new();
                let mut stack = vec![*root];
                while let Some(node) = stack.pop() {
                    if !visited.insert(node) {
                        continue;
                    }
                    if node != *root && draft.removals.contains(&node) {
                        continue;
                    }
                    if draft
                        .inserted
                        .get(&node)
                        .or_else(|| draft.owners.get(&node))
                        != Some(owner)
                    {
                        return Err(Error::ScopeInUse);
                    }
                    stack.extend(draft.render.tree().children(node)?);
                }
            }
            // Key changes sort all final surviving sibling outputs by logical
            // preorder, retaining same-owner insertion order and portal hosts.
            if !draft.child_lists.is_empty() {
                let mut ranks = BTreeMap::new();
                let mut stack = vec![self.root];
                while let Some(id) = stack.pop() {
                    if dead.contains(&id) {
                        continue;
                    }
                    ranks.insert(id, ranks.len());
                    stack.extend(draft.children(id)?.iter().rev());
                }
                let dead_nodes = draft.render.tree().removed_ids()?;
                let parents: Vec<_> = draft
                    .owners
                    .keys()
                    .chain(draft.inserted.keys())
                    .copied()
                    .filter(|id| !dead_nodes.contains(id))
                    .collect();
                for parent in parents {
                    let mut children: Vec<_> = draft
                        .render
                        .tree()
                        .children(parent)?
                        .iter()
                        .filter(|id| !dead_nodes.contains(id))
                        .copied()
                        .collect();
                    let before = children.clone();
                    children.sort_by_key(|node| {
                        ranks[draft
                            .inserted
                            .get(node)
                            .or_else(|| draft.owners.get(node))
                            .unwrap()]
                    });
                    if children != before {
                        draft.render.tree().set_children(parent, children)?;
                    }
                }
            }
            inserted = draft.inserted;
            created_scopes = draft.created_scopes;
            child_lists = draft.child_lists;
            removed_scopes = dead;
            added_sources = draft.added_sources;
            added_effects = draft.added_effects;
            Ok(value)
        })?;
        let result = &pending.output;
        let dead: BTreeSet<_> = result.removed.iter().copied().collect();
        let owners: BTreeSet<_> = result
            .removed
            .iter()
            .filter_map(|n| self.render_owners.remove(n))
            .collect();
        for owner in owners {
            self.scope_mut(owner)
                .renders
                .retain(|node| !dead.contains(node));
        }
        for id in &removed_scopes {
            if created_scopes.remove(id).is_none() {
                let slot = &mut self.scopes[id.slot];
                slot.scope = None;
                if let Some(next) = slot.generation.checked_add(1) {
                    slot.generation = next;
                    self.free.push(id.slot);
                }
            }
        }
        for (id, scope) in created_scopes {
            if id.slot >= self.scopes.len() {
                for slot in self.scopes.len()..id.slot {
                    self.free.push(slot);
                }
                self.scopes.resize_with(id.slot + 1, || Slot {
                    generation: 0,
                    scope: None,
                });
            }
            self.free.retain(|slot| *slot != id.slot);
            self.scopes[id.slot] = Slot {
                generation: id.generation,
                scope: Some(scope),
            };
        }
        for (id, children) in child_lists {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).children = children;
            }
        }
        for ((id, key), source) in added_sources {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).sources.insert(key, source);
            }
        }
        for ((id, key), effect) in added_effects {
            if !removed_scopes.contains(&id) {
                self.scope_mut(id).effects.insert(key, effect);
            }
        }
        for node in &result.inserted {
            let owner = inserted[node];
            self.scope_mut(owner).renders.push(*node);
            self.render_owners.insert(*node, owner);
        }
        Ok(self.ui.finish_edit(pending))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portal_ownership_survives_edits_and_foreign_descendants_preflight_before_sources() {
        let mut ui = LogicalUi::new(Style::default(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 1)
            .unwrap();
        let portal = ui.append_render(scope, host, Style::default(), 2).unwrap();
        let source = ui.state(scope, 0, || 2u32).unwrap();
        ui.bind(source, Property::color(portal)).unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        assert!(matches!(
            ui.edit(
                |tx| {
                    tx.set(source, 3)?;
                    Ok(())
                },
                |(), edit| edit.remove(host)
            ),
            Err(Error::ScopeInUse)
        ));
        assert_eq!(*ui.read(source).unwrap(), 2);
        assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
        let root_node = ui.root_render();
        let result = ui
            .edit(
                |tx| {
                    tx.set(source, 3)?;
                    Ok(())
                },
                |(), edit| {
                    let host2 = edit.append(root, root_node, Style::default(), 4)?;
                    edit.reparent(portal, host2)?;
                    edit.remove(host)?;
                    let extra = edit.append(scope, host2, Style::default(), 0)?;
                    edit.bind(source, Property::color(extra))?;
                    Ok(extra)
                },
            )
            .unwrap();
        assert_eq!(result.result.commit.revision, before.revision + 1);
        assert_eq!(ui.renders(scope).unwrap(), [portal, result.result.value]);
        assert!(!ui.renders(root).unwrap().contains(&host));
        ui.reconcile_keys(root, &[]).unwrap();
        assert!(ui.read(source).is_err());
        assert!(ui.reactive().tree().node(portal).is_err());
        assert!(ui.reactive().tree().node(result.result.value).is_err());
    }
    #[test]
    fn explicit_foreign_removal_cannot_authorize_an_unrelated_portal() {
        let mut ui = LogicalUi::new(Style::default(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let foreign = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let host = ui
            .append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap();
        let portal = ui
            .append_render(foreign, host, Style::default(), 1)
            .unwrap();
        let other = ui
            .append_render(foreign, ui.root_render(), Style::default(), 2)
            .unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        assert!(matches!(
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.remove(other)?;
                    edit.remove(host)
                }
            ),
            Err(Error::ScopeInUse)
        ));
        assert_eq!(ui.frame([100.0, 100.0]).unwrap().revision, before.revision);
        let result = ui
            .edit(
                |_| Ok(()),
                |(), edit| {
                    edit.remove(portal)?;
                    edit.remove(host)
                },
            )
            .unwrap();
        assert_eq!(result.result.commit.revision, before.revision + 1);
        assert_eq!(ui.renders(foreign).unwrap(), [other]);
    }
}

#[cfg(test)]
mod lifecycle_tests;
