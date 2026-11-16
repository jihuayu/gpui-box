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

#[test]
fn last_baseline_geometry_matches_independent_browser_markers() {
    use crate::retained::BaselineMeasureSnapshot;
    let mut failures = vec![];
    let mut count = 0;
    let exports: Vec<_> = include_str!(
        "../../../../../docs/performance/2026-11-17-last-baseline/container-baselines.tsv"
    )
    .lines()
    .collect();
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-last-baseline/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let n = |i: usize| f[i].parse::<f32>().unwrap();
        let direction = match f[0] {
            "row" => FlexDirection::Row,
            "row-reverse" => FlexDirection::RowReverse,
            "column" => FlexDirection::Column,
            "column-reverse" => FlexDirection::ColumnReverse,
            _ => unreachable!(),
        };
        let wrap = match f[2] {
            "nowrap" => FlexWrap::NoWrap,
            "wrap" => FlexWrap::Wrap,
            "wrap-reverse" => FlexWrap::WrapReverse,
            _ => unreachable!(),
        };
        let root = LayoutStyle::row()
            .size(
                Dimension::Px(n(6)),
                if f[7] == "auto" {
                    Dimension::Auto
                } else {
                    Dimension::Px(n(7))
                },
            )
            .flex_direction(direction)
            .flex_wrap(wrap)
            .direction(if f[1] == "true" {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            })
            .align_content(AlignContent::Start);
        let mut tree = UiTree::new_layout(root, 0).unwrap();
        let variant = f[5] == "1";
        let mut nodes = vec![];
        let mut tx = Transaction::default();
        for i in 0..4 {
            let at = 10 + i * 11;
            let align = match f[at] {
                "baseline" => AlignSelf::Baseline,
                "last baseline" => AlignSelf::LastBaseline,
                "start" => AlignSelf::Start,
                _ => unreachable!(),
            };
            let margin = Edges {
                top: if f[at + 1] == "auto" {
                    Length::Auto
                } else {
                    Length::Px(n(at + 1))
                },
                bottom: if f[at + 2] == "auto" {
                    Length::Auto
                } else {
                    Length::Px(n(at + 2))
                },
                left: if f[at + 3] == "auto" {
                    Length::Auto
                } else {
                    Length::Px(n(at + 3))
                },
                right: if f[at + 4] == "auto" {
                    Length::Auto
                } else {
                    Length::Px(n(at + 4))
                },
            };
            let mut style = fixed(n(at + 9), n(at + 10))
                .align_self(align)
                .margin(margin)
                .direction(if i % 2 == 1 {
                    LayoutDirection::Rtl
                } else {
                    LayoutDirection::Ltr
                });
            if variant {
                style = style.padding(Edges::all(Length::Px(2.0)));
                style.border = Edges::all(Length::Px(1.0));
            }
            let inset = if variant { 3.0 } else { 0.0 };
            let id = tree.append_layout(tree.root(), style, 1).unwrap();
            nodes.push(id);
            tx.measure(
                id,
                Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                    width: n(at + 9) - inset * 2.0,
                    height: (n(at + 6) + 1.0).max(n(at + 10) - inset * 2.0),
                    first_baseline: Some(n(at + 5) - inset),
                    last_baseline: Some(n(at + 6) - inset),
                })),
            );
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([200.0, 200.0]).unwrap();
        let root = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![root.size[0], root.size[1]];
        let mut expected = vec![n(8), n(9)];
        let exported: Vec<f32> = exports[count]
            .split('\t')
            .map(|v| v.parse().unwrap())
            .collect();
        actual.extend([root.baseline.unwrap(), root.last_baseline.unwrap()]);
        expected.extend(exported);
        for (i, id) in nodes.into_iter().enumerate() {
            let r = tree.layout_result(id).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
            expected.extend((17 + i * 11..21 + i * 11).map(n));
        }
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.02)
        {
            failures.push(format!(
                "{} actual={actual:?} expected={expected:?}",
                f[..8].join("/")
            ));
        }
        count += 1;
    }
    assert_eq!(count, 1440);
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.into_iter().take(24).collect::<Vec<_>>().join("\n")
    );
}

fn dual(width: f32, height: f32, first: f32, last: f32) -> MeasureSnapshot {
    MeasureSnapshot::FixedBaselines(crate::retained::BaselineMeasureSnapshot {
        width,
        height,
        first_baseline: Some(first),
        last_baseline: Some(last),
    })
}
fn baseline_pixels(frame: crate::retained::FramePacket) -> Vec<u8> {
    use crate::retained::{pacing, software};
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [120, 80],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn last_positions(last: f32) -> crate::retained::FramePacket {
    let mut tree = UiTree::new_layout(fixed(120.0, 80.0), 0).unwrap();
    tree.set_frame_build_policy(crate::retained::FrameBuildPolicy::Full);
    let a = tree
        .append_layout(tree.root(), fixed(30.0, 24.0), 0xff0000ff)
        .unwrap();
    let b = tree
        .append_layout(tree.root(), fixed(30.0, 20.0), 0x00ff00ff)
        .unwrap();
    let baseline = 80.0 - (24.0 - last).max(10.0);
    let mut tx = Transaction::default();
    tx.translation(a, [0.0, baseline - last]);
    tx.translation(b, [0.0, baseline - 10.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([120.0, 80.0]).unwrap()
}

#[test]
fn last_baseline_sources_publish_pixels_noops_rollback_and_old_frames() {
    use crate::retained::logical::LogicalUi;
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(
            fixed(120.0, 80.0).align_items(AlignItems::LastBaseline),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let scope = ui.root_scope();
        let a = ui
            .append_render_layout(scope, ui.root_render(), fixed(30.0, 24.0), 0xff0000ff)
            .unwrap();
        let b = ui
            .append_render_layout(scope, ui.root_render(), fixed(30.0, 20.0), 0x00ff00ff)
            .unwrap();
        let source = ui
            .state(scope, 90, || Some(dual(30.0, 24.0, 6.0, 14.0)))
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.bind(source, crate::retained::reactive::Property::measure(a))?;
                Ok(())
            },
        )
        .unwrap();
        let mut old = None;
        let mut previous = None;
        for last in [14.0, 18.0, 18.0, 14.0] {
            let render = |(): (), edit: &mut crate::retained::logical::LogicalEdit<'_, '_, '_>| {
                edit.properties()
                    .measure(b, Some(dual(30.0, 20.0, 9.0, 10.0)));
                Ok(())
            };
            let write = |tx: &mut crate::retained::reactive::ReactiveTransaction<'_>| {
                tx.set(source, Some(dual(30.0, 24.0, 6.0, last)))?;
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(120.0, 80.0),
                    [120.0, 80.0],
                    write,
                    render,
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(120.0, 80.0),
                    [120.0, 80.0],
                    write,
                    render,
                )
                .unwrap()
                .1
            };
            let r = ui.layout_result(a).unwrap();
            assert_eq!(r.baseline, Some(6.0));
            assert_eq!(r.last_baseline, Some(last));
            let baseline = 80.0 - (24.0 - last).max(10.0);
            assert_eq!(r.location, [0.0, baseline - last]);
            assert_eq!(
                frame.hit_test(1.0, 55.0),
                if last == 18.0 {
                    Some(a)
                } else {
                    Some(ui.root_render())
                }
            );
            assert_eq!(
                baseline_pixels(frame.clone()),
                baseline_pixels(last_positions(last))
            );
            if let Some((v, revision)) = previous {
                if v == last {
                    assert_eq!(frame.revision, revision);
                }
            }
            previous = Some((last, frame.revision));
            old.get_or_insert_with(|| (frame.clone(), baseline_pixels(frame)));
        }
        let before = ui.published_frame().unwrap().revision;
        let stats = ui.stats();
        assert!(matches!(
            ui.edit_frame(
                LayoutConstraints::viewport(120.0, 80.0),
                [120.0, 80.0],
                |tx| {
                    tx.set(source, Some(dual(30.0, 24.0, 6.0, 25.0)))?;
                    Ok(())
                },
                |(), _| Ok(())
            ),
            Err(Error::InvalidMeasure)
        ));
        assert_eq!(ui.published_frame().unwrap().revision, before);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.read(source).unwrap(), &Some(dual(30.0, 24.0, 6.0, 14.0)));
        drop(ui);
        let (old, expected) = old.unwrap();
        assert_eq!(baseline_pixels(old), expected);
    }
}

#[test]
fn last_baseline_only_metrics_propagate_through_caches_constraints_and_hidden_lifetimes() {
    let mut tree = UiTree::new_layout(fixed(120.0, 80.0), 0).unwrap();
    let group = tree
        .append_layout(tree.root(), fixed(60.0, 40.0), 0)
        .unwrap();
    let leaf = tree.append_layout(group, fixed(30.0, 24.0), 1).unwrap();
    let mut previous = None;
    for last in [14.0, 18.0, 18.0] {
        let mut tx = Transaction::default();
        tx.measure(leaf, Some(dual(30.0, 24.0, 6.0, last)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([120.0, 80.0]).unwrap();
        for id in [leaf, group, tree.root()] {
            let r = tree.layout_result(id).unwrap();
            assert_eq!(r.baseline, Some(6.0));
            assert_eq!(r.last_baseline, Some(last));
        }
        if let Some(before) = previous {
            assert!(std::sync::Arc::ptr_eq(&before, &frame.items));
            assert_eq!(frame.recorded_chunks, 0);
        }
        previous = Some(frame.items.clone());
        let published = tree.published_frame().unwrap().revision;
        let result = tree
            .measure_node_with_constraints(
                group,
                LayoutConstraints::new(AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .unwrap();
        assert_eq!(result.last_baseline, Some(last));
        assert_eq!(tree.published_frame().unwrap().revision, published);
    }
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(60.0, 40.0).display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([120.0, 80.0]).unwrap();
    for id in [leaf, group, tree.root()] {
        assert_eq!(tree.layout_result(id).unwrap().last_baseline, None);
    }
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(60.0, 40.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.local_frame([120.0, 80.0]).unwrap();
    assert_eq!(tree.layout_result(group).unwrap().last_baseline, Some(18.0));
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(60.0, 40.0).container_type(ContainerType::Size));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([120.0, 80.0]).unwrap();
    assert_eq!(tree.layout_result(group).unwrap().last_baseline, None);
    let mut tx = Transaction::default();
    tx.layout_style(group, fixed(60.0, 40.0));
    tx.measure(leaf, None);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([120.0, 80.0]).unwrap();
    assert_eq!(tree.layout_result(group).unwrap().last_baseline, Some(24.0));
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_last_baseline_updates_match_independent_full_positions() {
    let mut tree =
        UiTree::new_layout(fixed(120.0, 80.0).align_items(AlignItems::LastBaseline), 0).unwrap();
    let a = tree
        .append_layout(tree.root(), fixed(30.0, 24.0), 0xff0000ff)
        .unwrap();
    let b = tree
        .append_layout(tree.root(), fixed(30.0, 20.0), 0x00ff00ff)
        .unwrap();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("last baseline adapter: {:?}", renderer.adapter_info());
    for last in [14.0, 18.0, 14.0, 18.0] {
        let mut tx = Transaction::default();
        tx.measure(a, Some(dual(30.0, 24.0, 6.0, last)));
        tx.measure(b, Some(dual(30.0, 20.0, 9.0, 10.0)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([120.0, 80.0]).unwrap();
        renderer
            .submit_local(&frame, [120, 80], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            renderer.image().unwrap().as_raw(),
            renderer
                .reference_image(&last_positions(last), [120, 80], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();
}

#[test]
fn custom_parent_transports_both_baseline_sets_and_rejects_invalid_final_metrics() {
    use crate::retained::{logical::LogicalUi, parent_layout::*};
    #[derive(Debug)]
    struct Forward(bool);
    impl ParentLayout for Forward {
        fn layout(
            &self,
            input: ParentLayoutInput,
            children: &mut dyn ParentLayoutChildren,
        ) -> Result<ParentLayoutOutput, Error> {
            let slot = ChildLayoutInput::new(LayoutConstraints::new(
                AvailableSpace::Unbounded,
                AvailableSpace::Unbounded,
            ));
            let child = children.measure(0, slot)?;
            Ok(ParentLayoutOutput {
                size: [
                    input.known_size.width.unwrap_or(40.0),
                    input.known_size.height.unwrap_or(32.0),
                ],
                baseline: child.baseline.map(|b| b + 3.0),
                last_baseline: if self.0 {
                    Some(f32::NAN)
                } else {
                    child.last_baseline.map(|b| b + 3.0)
                },
                placements: vec![ParentPlacement {
                    child: 0,
                    input: slot,
                    origin: [2.0, 3.0],
                }],
            })
        }
    }
    let mut tree =
        LogicalUi::new_layout(fixed(120.0, 80.0), 0, BackendCapabilities::default()).unwrap();
    let scope = tree.root_scope();
    let host = tree
        .append_render_layout(
            scope,
            tree.root_render(),
            fixed(40.0, 32.0).padding(Edges::all(Length::Px(2.0))),
            0,
        )
        .unwrap();
    let leaf = tree
        .append_render_layout(scope, host, fixed(20.0, 24.0), 0xff0000ff)
        .unwrap();
    tree.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties()
                .measure(leaf, Some(dual(20.0, 24.0, 6.0, 18.0)));
            edit.properties()
                .parent_layout(host, Some(ParentLayoutSnapshot::new(Forward(false))));
            Ok(())
        },
    )
    .unwrap();
    tree.frame([120.0, 80.0]).unwrap();
    assert_eq!(tree.layout_result(host).unwrap().baseline, Some(11.0));
    assert_eq!(tree.layout_result(host).unwrap().last_baseline, Some(23.0));
    let before = tree.published_frame().unwrap().revision;
    assert!(
        tree.edit_frame(
            LayoutConstraints::viewport(120.0, 80.0),
            [120.0, 80.0],
            |_| Ok(()),
            |(), edit| {
                edit.properties()
                    .parent_layout(host, Some(ParentLayoutSnapshot::new(Forward(true))));
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(tree.published_frame().unwrap().revision, before);
    assert_eq!(tree.layout_result(host).unwrap().last_baseline, Some(23.0));
}

#[test]
fn last_baselines_cross_constraint_boxes_block_containers_and_single_metric_leaves() {
    for block in [false, true] {
        let mut tree = UiTree::new_layout(fixed(120.0, 80.0), 0).unwrap();
        let host = if block {
            tree.append(
                tree.root(),
                taffy::Style {
                    display: taffy::Display::Block,
                    size: taffy::Size {
                        width: taffy::style_helpers::length(60.0),
                        height: taffy::style_helpers::length(40.0),
                    },
                    padding: taffy::Rect {
                        top: taffy::style_helpers::length(2.0),
                        bottom: taffy::style_helpers::length(2.0),
                        left: taffy::style_helpers::length(2.0),
                        right: taffy::style_helpers::length(2.0),
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap()
        } else {
            tree.append_layout(
                tree.root(),
                LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
                    .size(Dimension::Px(60.0), Dimension::Px(40.0))
                    .padding(Edges::all(Length::Px(2.0))),
                0,
            )
            .unwrap()
        };
        let leaf = tree.append_layout(host, fixed(20.0, 24.0), 1).unwrap();
        let mut tx = Transaction::default();
        tx.measure(leaf, Some(dual(20.0, 24.0, 6.0, 18.0)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([120.0, 80.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().baseline, Some(8.0));
        assert_eq!(tree.layout_result(host).unwrap().last_baseline, Some(20.0));
        let mut tx = Transaction::default();
        measure(&mut tx, leaf, Some(7.0));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([120.0, 80.0]).unwrap();
        assert_eq!(tree.layout_result(host).unwrap().baseline, Some(9.0));
        assert_eq!(tree.layout_result(host).unwrap().last_baseline, Some(9.0));
    }
    for style in [
        LayoutStyle::row().align_items(AlignItems::LastBaseline.safe()),
        LayoutStyle::row().align_self(AlignSelf::LastBaseline.unsafe_overflow()),
    ] {
        assert_eq!(style.validate(), Err(Error::InvalidStyle));
    }
}
