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
}
impl LogicalEdit<'_, '_, '_> {
    pub fn append(
        &mut self,
        owner: ScopeId,
        parent: NodeId,
        style: Style,
        color: u32,
    ) -> Result<NodeId, Error> {
        if owner.owner != self.identity
            || self
                .scopes
                .get(owner.slot)
                .filter(|s| s.generation == owner.generation)
                .and_then(|s| s.scope.as_ref())
                .is_none()
        {
            return Err(Error::InvalidScope);
        }
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
}
impl LogicalUi {
    /// Atomically edit render output and existing scoped sources. Scope/state/
    /// effect construction and disposal continue through their explicit APIs.
    pub fn edit<S, R>(
        &mut self,
        sources: impl FnOnce(&mut ReactiveTransaction<'_>) -> Result<S, Error>,
        render: impl FnOnce(S, &mut LogicalEdit<'_, '_, '_>) -> Result<R, Error>,
    ) -> Result<ReactiveEditCommit<R>, Error> {
        let mut inserted = BTreeMap::new();
        let result = self.ui.edit(sources, |value, render_draft| {
            let mut draft = LogicalEdit {
                render: render_draft,
                identity: self.identity,
                scopes: &self.scopes,
                owners: &self.render_owners,
                inserted: BTreeMap::new(),
                removals: BTreeSet::new(),
            };
            let value = render(value, &mut draft)?;
            draft.render.tree().removed_ids()?; // settle property child-list writes
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
            inserted = draft.inserted;
            Ok(value)
        })?;
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
        for node in &result.inserted {
            let owner = inserted[node];
            self.scope_mut(owner).renders.push(*node);
            self.render_owners.insert(*node, owner);
        }
        Ok(result)
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
