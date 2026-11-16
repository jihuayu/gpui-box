use super::*;
use crate::retained::{
    BackendCapabilities, CustomMeasureSnapshot, MeasureSnapshot, Transaction, UiTree,
};

fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(Dimension::Px(width), Dimension::Px(height))
        .flex_shrink(0.0)
        .align_items(AlignItems::Start)
}

fn measure(tx: &mut Transaction, node: crate::retained::NodeId, baseline: Option<f32>) {
    tx.measure(
        node,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 20.0,
            height: 12.0,
            baseline,
        })),
    );
}

#[test]
fn baseline_only_changes_publish_through_cached_ancestors_without_paint_work() {
    for local in [false, true] {
        let mut tree = UiTree::new_layout(fixed(100.0, 40.0), 0).unwrap();
        let group = tree
            .append_layout(tree.root(), fixed(50.0, 25.0), 1)
            .unwrap();
        let leaf = tree.append_layout(group, fixed(20.0, 12.0), 2).unwrap();
        let mut tx = Transaction::default();
        measure(&mut tx, leaf, Some(5.0));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let before = tree.frame([100.0, 40.0]).unwrap();
        if local {
            tree.local_frame([100.0, 40.0]).unwrap();
        }
        for id in [leaf, group, tree.root()] {
            assert_eq!(tree.layout_result(id).unwrap().baseline, Some(5.0));
        }
        let mut tx = Transaction::default();
        measure(&mut tx, leaf, Some(9.0));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        // Queries observe the last published result, never half-applied inputs.
        assert_eq!(tree.layout_result(leaf).unwrap().baseline, Some(5.0));
        if local {
            tree.local_frame([100.0, 40.0]).unwrap();
        }
        let after = tree.frame([100.0, 40.0]).unwrap();
        for id in [leaf, group, tree.root()] {
            assert_eq!(tree.layout_result(id).unwrap().baseline, Some(9.0));
        }
        assert!(std::sync::Arc::ptr_eq(&before.items, &after.items));
        assert!(after.updates.is_empty());
        assert_eq!(after.recorded_chunks, 0);
        assert!(after.revision > before.revision);

        // A paint-only frame does not invalidate or recompute the baseline.
        let mut tx = Transaction::default();
        tx.color(leaf, 0xffffffff);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(tree.frame([100.0, 40.0]).unwrap().layout_runs, 0);
        assert_eq!(tree.layout_result(group).unwrap().baseline, Some(9.0));
    }
}

#[test]
fn hidden_subtrees_clear_baselines_and_restoration_uses_latest_measure() {
    let mut tree = UiTree::new_layout(fixed(100.0, 40.0), 0).unwrap();
    let group = tree
        .append_layout(tree.root(), fixed(50.0, 25.0), 1)
        .unwrap();
    let leaf = tree.append_layout(group, fixed(20.0, 12.0), 2).unwrap();
    let mut tx = Transaction::default();
    measure(&mut tx, leaf, Some(6.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 40.0]).unwrap();
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(50.0, 25.0).display(LayoutDisplay::None));
    measure(&mut tx, leaf, Some(8.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 40.0]).unwrap();
    for id in [leaf, group, tree.root()] {
        assert_eq!(tree.layout_result(id).unwrap().baseline, None);
    }
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(50.0, 25.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 40.0]).unwrap();
    for id in [leaf, group, tree.root()] {
        assert_eq!(tree.layout_result(id).unwrap().baseline, Some(8.0));
    }
    let mut tx = Transaction::default();
    tx.measure(leaf, None);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 40.0]).unwrap();
    assert_eq!(tree.layout_result(leaf).unwrap().baseline, None);
    // A flex container synthesizes its baseline from the child's bottom edge.
    assert_eq!(tree.layout_result(group).unwrap().baseline, Some(12.0));
}

#[test]
fn nested_flex_baseline_matches_alignment_and_measure_only_stays_isolated() {
    let root_style = fixed(100.0, 40.0).align_items(AlignItems::Baseline);
    let mut tree = UiTree::new_layout(root_style, 0).unwrap();
    let group = tree
        .append_layout(
            tree.root(),
            fixed(35.0, 25.0).padding(Edges {
                top: Length::Px(3.0),
                ..Edges::zero()
            }),
            1,
        )
        .unwrap();
    let nested = tree.append_layout(group, fixed(20.0, 12.0), 2).unwrap();
    let peer = tree
        .append_layout(tree.root(), fixed(20.0, 12.0), 3)
        .unwrap();
    let mut tx = Transaction::default();
    measure(&mut tx, nested, Some(6.0));
    measure(&mut tx, peer, Some(11.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([100.0, 40.0]).unwrap();
    let a = tree.layout_result(group).unwrap();
    let b = tree.layout_result(peer).unwrap();
    assert_eq!(a.baseline, Some(9.0));
    assert_eq!(a.location[1], 2.0);
    assert_eq!(
        a.location[1] + a.baseline.unwrap(),
        b.location[1] + b.baseline.unwrap()
    );
    assert_eq!(
        tree.layout_result(tree.root()).unwrap().baseline,
        Some(11.0)
    );
    let measured = tree
        .measure_node_with_constraints(
            group,
            LayoutConstraints::new(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        )
        .unwrap();
    assert_eq!(measured.baseline, Some(9.0));
    assert_eq!(tree.layout_result(group).unwrap(), a);
    assert_eq!(tree.published_frame().unwrap().revision, frame.revision);
}

#[test]
fn baseline_alignment_updates_paint_and_hit_geometry_when_nested_metrics_change() {
    let root_style = fixed(100.0, 40.0).align_items(AlignItems::Baseline);
    let mut tree = UiTree::new_layout(root_style, 0).unwrap();
    let group = tree
        .append_layout(tree.root(), fixed(35.0, 20.0), 1)
        .unwrap();
    let nested = tree.append_layout(group, fixed(20.0, 12.0), 2).unwrap();
    let peer = tree
        .append_layout(tree.root(), fixed(20.0, 12.0), 3)
        .unwrap();
    let mut tx = Transaction::default();
    measure(&mut tx, nested, Some(6.0));
    measure(&mut tx, peer, Some(10.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let before = tree.frame([100.0, 40.0]).unwrap();
    assert_eq!(tree.layout_result(group).unwrap().location[1], 4.0);
    assert_eq!(before.hit_test(5.0, 2.0), Some(tree.root()));

    let mut tx = Transaction::default();
    measure(&mut tx, nested, Some(11.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let after = tree.frame([100.0, 40.0]).unwrap();
    let group_layout = tree.layout_result(group).unwrap();
    let peer_layout = tree.layout_result(peer).unwrap();
    assert_eq!(group_layout.location[1], 0.0);
    assert_eq!(peer_layout.location[1], 1.0);
    assert_eq!(group_layout.baseline, Some(11.0));
    assert_eq!(
        tree.layout_result(tree.root()).unwrap().baseline,
        Some(11.0)
    );
    assert_eq!(after.recorded_chunks, 0);
    assert_eq!(after.hit_test(5.0, 2.0), Some(nested));
    assert_eq!(before.hit_test(5.0, 2.0), Some(tree.root()));
    // Inherited world placement must follow the alignment change even when
    // the nested child's parent-local position stayed a cache hit.
    let old_nested = before
        .items
        .iter()
        .find(|item| item.node == nested)
        .unwrap();
    let new_nested = after.items.iter().find(|item| item.node == nested).unwrap();
    assert_eq!(old_nested.bounds.y, 4.0);
    assert_eq!(new_nested.bounds.y, 0.0);
    assert!(std::sync::Arc::ptr_eq(
        &old_nested.commands,
        &new_nested.commands
    ));
}

#[test]
fn flex_baseline_selection_follows_order_and_wrap_reverse() {
    let mut tree = UiTree::new_layout(fixed(40.0, 40.0).flex_wrap(FlexWrap::Wrap), 0).unwrap();
    let mut nodes = vec![];
    let mut tx = Transaction::default();
    for baseline in [4.0, 8.0, 10.0] {
        let id = tree
            .append_layout(tree.root(), fixed(20.0, 12.0), 1)
            .unwrap();
        nodes.push(id);
        measure(&mut tx, id, Some(baseline));
    }
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([40.0, 40.0]).unwrap();
    assert_eq!(tree.layout_result(tree.root()).unwrap().baseline, Some(4.0));
    let mut tx = Transaction::default();
    tx.layout_style(nodes[2], fixed(20.0, 12.0).order(-1));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([40.0, 40.0]).unwrap();
    assert_eq!(
        tree.layout_result(tree.root()).unwrap().baseline,
        Some(10.0)
    );
    let mut tx = Transaction::default();
    tx.layout_style(
        tree.root(),
        fixed(40.0, 40.0).flex_wrap(FlexWrap::WrapReverse),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([40.0, 40.0]).unwrap();
    let last = tree.layout_result(nodes[1]).unwrap();
    assert_eq!(
        tree.layout_result(tree.root()).unwrap().baseline,
        Some(last.location[1] + 8.0)
    );
}

#[test]
fn legacy_grid_block_and_absolute_nodes_publish_final_baseline_results() {
    use crate::retained::{Style, length};
    for display in [
        taffy::Display::Grid,
        taffy::Display::Block,
        taffy::Display::Flex,
    ] {
        let mut tree = UiTree::new_layout(fixed(200.0, 80.0), 0).unwrap();
        let group = tree
            .append(
                tree.root(),
                Style {
                    display,
                    padding: taffy::Rect {
                        top: length(3.0),
                        ..taffy::Rect::zero()
                    },
                    align_items: taffy::AlignItems::START,
                    ..Style::default()
                },
                1,
            )
            .unwrap();
        let leaf = tree.append_layout(group, fixed(20.0, 12.0), 2).unwrap();
        let mut tx = Transaction::default();
        measure(&mut tx, leaf, Some(7.0));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([200.0, 80.0]).unwrap();
        assert_eq!(tree.layout_result(group).unwrap().baseline, Some(10.0));
        // Out-of-flow layout uses a distinct final Layout constructor.
        let mut style = tree.style(group).unwrap().clone();
        style.position = taffy::Position::Absolute;
        let mut tx = Transaction::default();
        tx.style(group, style);
        measure(&mut tx, leaf, Some(9.0));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([200.0, 80.0]).unwrap();
        assert_eq!(tree.layout_result(group).unwrap().baseline, Some(12.0));
        assert_eq!(tree.layout_result(leaf).unwrap().baseline, Some(9.0));
    }
}
