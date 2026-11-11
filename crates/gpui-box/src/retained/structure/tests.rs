use super::*;
fn fixture() -> UiTree {
    UiTree::new(
        Style {
            size: Size {
                width: length(160.0),
                height: length(80.0),
            },
            ..Default::default()
        },
        0,
    )
    .unwrap()
}
fn leaf(tree: &mut UiTree, parent: NodeId, color: u32) -> NodeId {
    tree.append(
        parent,
        Style {
            size: Size {
                width: length(20.0),
                height: length(20.0),
            },
            ..Default::default()
        },
        color,
    )
    .unwrap()
}
#[test]
fn atomic_reparent_and_rotation_preserve_taffy_nodes_chunks_and_old_snapshot() {
    let mut tree = fixture();
    let root = tree.root();
    let a = leaf(&mut tree, root, 0xff0000ff);
    let b = leaf(&mut tree, a, 0x00ff00ff);
    let before = tree.frame([160.0, 80.0]).unwrap();
    let layouts = [tree.node(a).unwrap().layout, tree.node(b).unwrap().layout];
    // root→a→b becomes root→b→a in one final-graph update.
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![b]);
    tx.children(a, vec![]);
    tx.children(b, vec![a]);
    let commit = tree.commit_structure(tx).unwrap();
    assert_eq!(commit.revision, before.revision + 1);
    assert_eq!(commit.changed_nodes, 3);
    let after = tree.frame([160.0, 80.0]).unwrap();
    assert_eq!(after.recorded_chunks, 0);
    assert_eq!(
        after.items.iter().map(|item| item.node).collect::<Vec<_>>(),
        vec![root, b, a]
    );
    assert_eq!(tree.node(a).unwrap().layout, layouts[0]);
    assert_eq!(tree.node(b).unwrap().layout, layouts[1]);
    for id in [a, b] {
        assert_eq!(
            tree.layout.parent(tree.node(id).unwrap().layout),
            tree.node(id)
                .unwrap()
                .parent
                .map(|p| tree.node(p).unwrap().layout)
        );
        let old = before.items.iter().find(|item| item.node == id).unwrap();
        let new = after.items.iter().find(|item| item.node == id).unwrap();
        assert!(Arc::ptr_eq(&old.commands, &new.commands));
    }
    assert_eq!(
        before
            .items
            .iter()
            .map(|item| item.node)
            .collect::<Vec<_>>(),
        vec![root, a, b]
    );
    let mut fresh = fixture();
    let r = fresh.root();
    let y = leaf(&mut fresh, r, 0x00ff00ff);
    leaf(&mut fresh, y, 0xff0000ff);
    let reference = fresh.frame([160.0, 80.0]).unwrap();
    assert_eq!(
        after
            .items
            .iter()
            .map(|i| (i.bounds, i.fill_color))
            .collect::<Vec<_>>(),
        reference
            .items
            .iter()
            .map(|i| (i.bounds, i.fill_color))
            .collect::<Vec<_>>()
    );
    let mut noop = StructureTransaction::default();
    noop.children(root, vec![b]);
    noop.children(root, vec![b]);
    assert!(!tree.commit_structure(noop).unwrap().request_frame);
    let idle = tree.frame([160.0, 80.0]).unwrap();
    assert!(Arc::ptr_eq(&idle.items, &after.items));
    assert_eq!(idle.layout_runs, 0);
}
#[test]
fn invalid_final_graphs_leave_revision_layout_and_snapshot_unchanged() {
    let mut tree = fixture();
    let root = tree.root();
    let a = leaf(&mut tree, root, 1);
    let b = leaf(&mut tree, root, 2);
    let c = leaf(&mut tree, a, 3);
    let before = tree.frame([160.0, 80.0]).unwrap();
    let mut other = fixture();
    let foreign = other.append(other.root(), Style::default(), 0).unwrap();
    let mut cases = vec![];
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![b, a, a]);
    cases.push(tx);
    let mut tx = StructureTransaction::default();
    tx.children(a, vec![c, b]);
    cases.push(tx);
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![a]);
    cases.push(tx);
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![a, b, root]);
    cases.push(tx);
    // A disconnected cycle still gives every non-root exactly one parent.
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![b]);
    tx.children(c, vec![a]);
    cases.push(tx);
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![foreign]);
    cases.push(tx);
    for tx in cases {
        assert!(tree.commit_structure(tx).is_err());
        assert_eq!(tree.node(root).unwrap().children, vec![a, b]);
        assert_eq!(tree.node(a).unwrap().children, vec![c]);
        assert_eq!(tree.node(b).unwrap().parent, Some(root));
        let current = tree.frame([160.0, 80.0]).unwrap();
        assert_eq!(current.revision, before.revision);
        assert_eq!(current.layout_runs, 0);
        assert!(Arc::ptr_eq(&current.items, &before.items));
    }
    let mut measure = Transaction::default();
    measure.image(b, Some(ImageResource::rgba8([1, 1], vec![255; 4]).unwrap()));
    tree.commit(measure, BackendCapabilities::default())
        .unwrap();
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![b]);
    tx.children(b, vec![a]);
    assert!(matches!(
        tree.commit_structure(tx),
        Err(Error::MeasuredNodeHasChildren)
    ));
    tree.revision = u64::MAX;
    let mut tx = StructureTransaction::default();
    tx.children(root, vec![b, a]);
    assert!(matches!(
        tree.commit_structure(tx),
        Err(Error::RevisionExhausted)
    ));
    assert_eq!(tree.node(root).unwrap().children, vec![a, b]);
}

#[test]
fn logical_permutations_and_portal_moves_publish_once_and_failed_cycle_keeps_sources() {
    use super::super::logical::{LogicalStructureTransaction, LogicalUi};
    use super::super::reactive::Property;
    let mut ui = LogicalUi::from_tree(fixture(), BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let scopes = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let host_a = ui
        .append_render(root, ui.root_render(), Style::default(), 0)
        .unwrap();
    let host_b = ui
        .append_render(root, ui.root_render(), Style::default(), 0)
        .unwrap();
    let a = ui
        .append_render(scopes[0], host_a, Style::default(), 1)
        .unwrap();
    let b = ui
        .append_render(scopes[1], host_a, Style::default(), 2)
        .unwrap();
    let c = ui
        .append_render(scopes[0], host_b, Style::default(), 3)
        .unwrap();
    let d = ui
        .append_render(scopes[1], host_b, Style::default(), 4)
        .unwrap();
    let color = ui.state(scopes[0], 0, || 1u32).unwrap();
    ui.bind(color, Property::color(a)).unwrap();
    let before = ui.frame([160.0, 80.0]).unwrap();
    ui.reconcile_keys(root, &[2, 1]).unwrap();
    let reordered = ui.frame([160.0, 80.0]).unwrap();
    assert_eq!(
        reordered.revision,
        before.revision + 1,
        "two changed hosts are one revision"
    );
    let mut tx = LogicalStructureTransaction::default();
    tx.reorder(root, scopes.clone());
    tx.reparent(a, host_b);
    tx.reparent(d, host_a);
    let commit = ui.commit_structure(tx).unwrap();
    assert_eq!(commit.revision, reordered.revision + 1);
    let frame = ui.frame([160.0, 80.0]).unwrap();
    assert_eq!(frame.recorded_chunks, 0);
    assert_eq!(
        ui.reactive().tree().node(host_a).unwrap().children,
        vec![b, d]
    );
    assert_eq!(
        ui.reactive().tree().node(host_b).unwrap().children,
        vec![c, a]
    );
    assert_eq!(ui.renders(scopes[0]).unwrap(), [a, c]);
    assert_eq!(ui.parent(scopes[0]).unwrap(), Some(root));
    let mut bad = LogicalStructureTransaction::default();
    bad.reorder(root, vec![scopes[1], scopes[0]]);
    bad.reparent(host_b, a);
    assert!(ui.commit_structure(bad).is_err());
    assert_eq!(ui.children(root).unwrap(), scopes);
    assert_eq!(ui.frame([160.0, 80.0]).unwrap().revision, frame.revision);
    ui.transaction(|tx| {
        tx.set(color, 0x4488ffff)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(*ui.read(color).unwrap(), 0x4488ffff);
    assert!(!ui.reparent_render(a, host_b).unwrap().request_frame);
}

#[test]
fn removals_coalesce_validate_before_retirement_and_preserve_new_generations() {
    let mut tree = fixture();
    let root = tree.root();
    let a = leaf(&mut tree, root, 1);
    let b = leaf(&mut tree, a, 2);
    let c = leaf(&mut tree, root, 3);
    let old = tree.frame([160.0, 80.0]).unwrap();
    let mut invalid = StructureTransaction::default();
    invalid.remove(a);
    invalid.children(c, vec![b]);
    assert!(tree.commit_structure(invalid).is_err());
    assert_eq!(tree.frame([160.0, 80.0]).unwrap().revision, old.revision);
    let mut tx = StructureTransaction::default();
    tx.remove(b);
    tx.remove(a);
    tx.remove(b);
    let result = tree.commit_structure(tx).unwrap();
    assert_eq!(result.revision, old.revision + 1);
    assert!(tree.node(a).is_err() && tree.node(b).is_err());
    assert_eq!(tree.node(root).unwrap().children, vec![c]);
    let after = tree.frame([160.0, 80.0]).unwrap();
    assert_eq!(after.removed.as_ref(), &[a, b]);
    assert_eq!(old.items.len(), 4);
    let new = leaf(&mut tree, root, 4);
    assert_ne!(new, a);
    assert_ne!(new, b);
    let mut stale = StructureTransaction::default();
    stale.remove(a);
    assert!(tree.commit_structure(stale).is_err());
    assert!(tree.node(new).is_ok());
    tree.revision = u64::MAX;
    let mut exhausted = StructureTransaction::default();
    exhausted.remove(new);
    assert!(matches!(
        tree.commit_structure(exhausted),
        Err(Error::RevisionExhausted)
    ));
    assert!(tree.node(new).is_ok());
}

#[test]
fn keyed_mixed_removal_preflights_before_cleanup_and_commits_one_revision() {
    use super::super::logical::LogicalUi;
    use super::super::reactive::Property;
    use std::{cell::Cell, rc::Rc};
    let mut ui = LogicalUi::from_tree(fixture(), BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let scopes = ui.reconcile_keys(root, &[1, 2, 3]).unwrap().order;
    let hosts = [
        ui.append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap(),
        ui.append_render(root, ui.root_render(), Style::default(), 0)
            .unwrap(),
    ];
    let source = ui.state(scopes[0], 0, || 0x112233ffu32).unwrap();
    for host in hosts {
        for scope in &scopes {
            let node = ui.append_render(*scope, host, Style::default(), 0).unwrap();
            if *scope == scopes[0] {
                ui.bind(source, Property::color(node)).unwrap();
            }
        }
    }
    let cleanups = Rc::new(Cell::new(0));
    let log = cleanups.clone();
    ui.effect(scopes[0], 0, &[source.dependency()], move |_| {
        let log = log.clone();
        Some(Box::new(move || log.set(log.get() + 1)))
    })
    .unwrap();
    let before = ui.frame([160.0, 80.0]).unwrap();
    assert!(matches!(
        ui.reconcile_keys(root, &[3, 3, 4]),
        Err(Error::DuplicateKey)
    ));
    assert_eq!(cleanups.get(), 0);
    assert_eq!(ui.children(root).unwrap(), scopes);
    assert!(ui.read(source).is_ok());
    let changes = ui.reconcile_keys(root, &[3, 2, 4]).unwrap();
    assert_eq!(changes.inserted.len(), 1);
    assert_eq!(cleanups.get(), 1);
    assert!(ui.read(source).is_err());
    assert_eq!(ui.reactive().binding_count(), 0);
    let after = ui.frame([160.0, 80.0]).unwrap();
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.recorded_chunks, 0);
}
