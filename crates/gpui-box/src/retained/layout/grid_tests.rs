use super::*;
#[cfg(feature = "retained-wgpu")]
use crate::retained::Transaction;
use crate::retained::{
    BackendCapabilities, FrameBuildPolicy, FramePacket, NodeId, UiTree, logical::LogicalUi, pacing,
    reactive::Property, responsive::*, software,
};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(Dimension::Px(w), Dimension::Px(h))
        .flex_shrink(0.0)
}
fn minfr(min: f32, fr: f32) -> GridTrack {
    GridTrack::minmax(GridMinTrack::Px(min), GridMaxTrack::Fr(fr))
}
fn template(name: &str, flow: &str) -> GridStyle {
    let columns = match name {
        "fraction" => vec![minfr(0.0, 1.0).into(), minfr(30.0, 2.0).into()],
        "percent" => vec![GridTrack::percent(0.3).into(), minfr(20.0, 1.0).into()],
        "repeat" => vec![GridTemplate::repeat(
            GridRepetition::Count(2),
            vec![minfr(20.0, 1.0)],
        )],
        "fit" | "fill" => vec![GridTemplate::repeat(
            if name == "fit" {
                GridRepetition::AutoFit
            } else {
                GridRepetition::AutoFill
            },
            vec![minfr(60.0, 1.0)],
        )],
        "intrinsic" => vec![
            GridTrack::min_content().into(),
            GridTrack::fit_content_px(70.0).into(),
            GridTrack::max_content().into(),
        ],
        _ => unreachable!(),
    };
    GridStyle::default()
        .columns(columns)
        .rows(vec![
            GridTrack::minmax(GridMinTrack::Px(30.0), GridMaxTrack::Auto).into(),
            GridTrack::px(40.0).into(),
        ])
        .auto_columns(vec![GridTrack::px(45.0), GridTrack::px(65.0)])
        .auto_rows(vec![GridTrack::px(35.0), GridTrack::px(25.0)])
        .auto_flow(match flow {
            "row" => GridAutoFlow::Row,
            "column" => GridAutoFlow::Column,
            "row dense" => GridAutoFlow::RowDense,
            "column dense" => GridAutoFlow::ColumnDense,
            _ => unreachable!(),
        })
}
fn browser_tree(f: &[&str]) -> (UiTree, Vec<NodeId>) {
    let align = match f[5] {
        "stretch" => AlignItems::Stretch,
        "start" => AlignItems::Start,
        "center" => AlignItems::Center,
        _ => unreachable!(),
    };
    let style = LayoutStyle::grid(template(f[0], f[1]))
        .size(Dimension::Px(f[3].parse().unwrap()), Dimension::Px(200.0))
        .padding(Edges::all(Length::Px(3.0)))
        .gap(Length::Px(6.0), Length::Px(4.0))
        .direction(if f[2] == "true" {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        })
        .align_items(align)
        .justify_items(align)
        .align_content(AlignContent::Start)
        .justify_content(JustifyContent::Start);
    let mut tree = UiTree::new_layout(style, 0).unwrap();
    let mut nodes = vec![];
    for i in 0..if matches!(f[0], "fit" | "fill") { 3 } else { 5 } {
        let mut style = LayoutStyle::column()
            .min_size(Dimension::Px(0.0), Dimension::Auto)
            .padding(Edges::all(Length::Px(2.0)))
            .margin(Edges::all(Length::Px(1.0)));
        style.border = Edges::all(Length::Px(1.0));
        if f[4] == "true" {
            match i {
                0 => {
                    style = style
                        .grid_column(GridPlacement::Span(2), GridPlacement::Auto)
                        .order(1);
                }
                1 => {
                    style = style
                        .grid_column(GridPlacement::Line(-2), GridPlacement::Line(-1))
                        .grid_row(GridPlacement::Line(1), GridPlacement::Auto);
                }
                2 => {
                    style = style
                        .grid_row(GridPlacement::Span(2), GridPlacement::Auto)
                        .justify_self(AlignSelf::End)
                        .align_self(AlignSelf::Center);
                }
                _ => {}
            }
        }
        let node = tree.append_layout(tree.root(), style, 0).unwrap();
        tree.append_layout(
            node,
            fixed(
                [30.0, 80.0, 25.0, 55.0, 70.0][i],
                [20.0, 30.0, 10.0, 35.0, 15.0][i] + if f[6] == "true" { 20.0 } else { 0.0 },
            ),
            0,
        )
        .unwrap();
        nodes.push(node);
    }
    (tree, nodes)
}
#[test]
fn public_grid_geometry_matches_independent_chromium() {
    let mut failures = vec![];
    let mut count = 0;
    for line in
        include_str!("../../../../../docs/performance/2026-11-17-public-grid/browser-reference.tsv")
            .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let (mut tree, nodes) = browser_tree(&f);
        tree.frame([400.0; 2]).unwrap();
        let root = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![root.size[0], root.size[1]];
        for node in nodes {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = f[7..].iter().map(|v| v.parse().unwrap()).collect();
        if actual
            .iter()
            .zip(&expected)
            .any(|(a, b)| (a - b).abs() > 0.06)
        {
            failures.push(format!("{:?}: {actual:?} != {expected:?}", &f[..7]));
        }
        count += 1;
    }
    assert_eq!(count, 1152);
    assert!(
        failures.is_empty(),
        "{} failures: {:?}",
        failures.len(),
        &failures[..failures.len().min(10)]
    );
}

#[test]
fn public_grid_rejects_invalid_tracks_repeats_and_placements_before_mutation() {
    let mut invalid = vec![];
    for v in [-1.0, f32::NAN, f32::INFINITY] {
        for track in [
            GridTrack::px(v),
            GridTrack::percent(v),
            GridTrack::fr(v),
            GridTrack::fit_content_px(v),
            GridTrack::fit_content_percent(v),
        ] {
            invalid.push(LayoutStyle::grid(
                GridStyle::default().columns(vec![track.into()]),
            ));
        }
    }
    for count in [GridRepetition::Count(0), GridRepetition::AutoFit] {
        invalid.push(LayoutStyle::grid(
            GridStyle::default().columns(vec![GridTemplate::repeat(count, vec![])]),
        ));
    }
    invalid.push(LayoutStyle::grid(GridStyle::default().columns(vec![
        GridTemplate::repeat(GridRepetition::AutoFit, vec![GridTrack::fr(1.0)]),
    ])));
    invalid.push(LayoutStyle::grid(GridStyle::default().columns(vec![
        GridTemplate::repeat(GridRepetition::AutoFill, vec![GridTrack::px(30.0)]),
        GridTemplate::repeat(GridRepetition::AutoFit, vec![GridTrack::px(40.0)]),
    ])));
    invalid.push(
        LayoutStyle::grid(GridStyle::default())
            .grid_column(GridPlacement::Line(0), GridPlacement::Auto),
    );
    invalid.push(
        LayoutStyle::grid(GridStyle::default())
            .grid_row(GridPlacement::Auto, GridPlacement::Span(0)),
    );
    invalid.push(
        LayoutStyle::grid(GridStyle::default()).main_cross_gap(Length::Px(2.0), Length::Px(3.0)),
    );
    invalid.push(LayoutStyle::grid(GridStyle::default()).justify_self(AlignSelf::Stretch.safe()));
    invalid.push(LayoutStyle::grid(GridStyle::default().columns(vec![
        GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::FitContentPx(50.0)).into(),
    ])));
    let good = LayoutStyle::grid(template("fraction", "row"))
        .size(Dimension::Px(160.0), Dimension::Px(200.0));
    let mut ui = LogicalUi::new_layout(good.clone(), 0, BackendCapabilities::default()).unwrap();
    let source = ui.state(ui.root_scope(), 0, || good.clone()).unwrap();
    ui.bind(source, Property::layout_style(ui.root_render()))
        .unwrap();
    let old = ui.frame([300.0; 2]).unwrap();
    let stats = ui.stats();
    for bad in invalid {
        assert!(matches!(
            ui.transaction(|tx| tx.set(source, bad)),
            Err(Error::InvalidStyle)
        ));
        assert_eq!(ui.read(source).unwrap(), &good);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

fn dynamic_style(mode: usize) -> LayoutStyle {
    if mode == 2 {
        return LayoutStyle::column().size(Dimension::Px(180.0), Dimension::Px(180.0));
    }
    LayoutStyle::grid(
        GridStyle::default()
            .columns(vec![GridTemplate::repeat(
                GridRepetition::Count(if mode == 0 { 2 } else { 3 }),
                vec![minfr(0.0, 1.0)],
            )])
            .auto_rows(vec![GridTrack::px(45.0)]),
    )
    .size(Dimension::Px(180.0), Dimension::Px(180.0))
    .padding(Edges::all(Length::Px(3.0)))
    .gap(Length::Px(4.0), Length::Px(4.0))
    .align_content(AlignContent::Start)
}
fn cell(index: usize, mode: usize) -> LayoutStyle {
    let mut style = LayoutStyle::column().min_size(Dimension::Px(0.0), Dimension::Px(0.0));
    if mode == 1 && index == 0 {
        style = style
            .grid_column(GridPlacement::Span(2), GridPlacement::Auto)
            .order(2);
    }
    style
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
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
fn reference(mode: usize) -> FramePacket {
    let mut tree = UiTree::new_layout(dynamic_style(mode), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    for i in 0..4 {
        let host = tree
            .append_layout(
                tree.root(),
                cell(i, mode),
                [0xff0000ff, 0x00ff00ff, 0x0000ffff, 0xffff00ff][i],
            )
            .unwrap();
        tree.append_layout(host, fixed(20.0, 15.0 + i as f32 * 3.0), 0x00ffffff)
            .unwrap();
    }
    tree.frame([180.0; 2]).unwrap()
}
#[test]
fn public_grid_typed_track_placement_order_and_display_updates_match_complete_cpu_frames() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(dynamic_style(0), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let source = ui.state(scope, 0, || dynamic_style(0)).unwrap();
        ui.bind(source, Property::layout_style(root)).unwrap();
        let mut nodes = vec![];
        for i in 0..4 {
            let host = ui
                .append_render_layout(
                    scope,
                    root,
                    cell(i, 0),
                    [0xff0000ff, 0x00ff00ff, 0x0000ffff, 0xffff00ff][i],
                )
                .unwrap();
            ui.append_render_layout(scope, host, fixed(20.0, 15.0 + i as f32 * 3.0), 0x00ffffff)
                .unwrap();
            nodes.push(host);
        }
        let mut old = None;
        for mode in [0, 1, 2, 0] {
            let build = |(), edit: &mut crate::retained::logical::LogicalEdit<'_, '_, '_>| {
                for (i, &node) in nodes.iter().enumerate() {
                    edit.properties().layout_style(node, cell(i, mode));
                }
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(180.0, 180.0),
                    [180.0; 2],
                    |tx| tx.set(source, dynamic_style(mode)).map(|_| ()),
                    build,
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(180.0, 180.0),
                    [180.0; 2],
                    |tx| tx.set(source, dynamic_style(mode)).map(|_| ()),
                    build,
                )
                .unwrap()
                .1
            };
            assert!(
                pixels(frame.clone()) == pixels(reference(mode)),
                "mode={mode} local={local}"
            );
            old.get_or_insert(frame);
        }
        let before = ui.published_frame().unwrap().revision;
        ui.transaction(|tx| {
            tx.set(source, dynamic_style(1))?;
            tx.set(source, dynamic_style(0))
        })
        .unwrap();
        assert_eq!(ui.published_frame().unwrap().revision, before);
        assert_eq!(ui.frame([180.0; 2]).unwrap().layout_runs, 0);
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn public_grid_auto_repeat_updates_and_layout_queries_use_existing_session_boundary() {
    let grid = |width: f32| {
        LayoutStyle::grid(GridStyle::default().columns(vec![GridTemplate::repeat(
            GridRepetition::AutoFit,
            vec![minfr(50.0, 1.0)],
        )]))
        .size(Dimension::Px(width), Dimension::Auto)
        .fixed_inline_size(width)
        .gap(Length::Px(3.0), Length::Px(3.0))
    };
    let mut ui = LogicalUi::new_layout(grid(160.0), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let source = ui.state(scope, 0, || grid(160.0)).unwrap();
    ui.bind(source, Property::layout_style(root)).unwrap();
    let target = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    for _ in 0..3 {
        ui.append_render_layout(scope, root, fixed(20.0, 10.0), 1)
            .unwrap();
    }
    let value = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                0,
                value,
                target,
                LayoutQuery::ContainerInline(root),
                LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![(200.0, fixed(20.0, 30.0))])?,
            )
        },
    )
    .unwrap();
    ui.frame_with_environment(
        &LayoutEnvironment::new([500.0; 2]),
        LayoutConstraints::viewport(300.0, 300.0),
        [300.0; 2],
    )
    .unwrap();
    assert_eq!(ui.layout_result(root).unwrap().size, [160.0, 23.0]);
    ui.transaction(|tx| tx.set(source, grid(240.0))).unwrap();
    assert_eq!(ui.layout_result(root).unwrap().size, [240.0, 30.0]);
    assert_eq!(ui.layout_result(target).unwrap().size[1], 30.0);
}

#[test]
fn public_grid_inline_builder_on_grid_host_preserves_content_height() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(Dimension::Percent(1.0), Dimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let grid = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::grid(
                GridStyle::default().columns(vec![minfr(0.0, 1.0).into(), minfr(0.0, 1.0).into()]),
            ),
            0,
        )
        .unwrap();
    let (_, frame) = ui
        .edit_local_frame(
            LayoutConstraints::new(AvailableSpace::Definite(180.0), AvailableSpace::Unbounded),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(grid, |input, edit| {
                    assert_eq!(input.known_width, Some(180.0));
                    edit.append_layout(scope, grid, fixed(20.0, 30.0), 1)?;
                    edit.append_layout(scope, grid, fixed(20.0, 50.0), 2)?;
                    Ok(())
                })
            },
        )
        .unwrap();
    assert_eq!(ui.layout_result(grid).unwrap().size, [180.0, 50.0]);
    assert!(!frame.resolve().items.is_empty());
}

#[test]
fn public_grid_hard_bounds_containment_and_intrinsic_queries_preserve_public_contracts() {
    let style = LayoutStyle::grid(
        GridStyle::default().columns(vec![GridTrack::px(60.0).into(), GridTrack::px(80.0).into()]),
    )
    .size(Dimension::Px(300.0), Dimension::Auto)
    .gap(Length::Px(0.0), Length::Px(4.0));
    let mut tree = UiTree::new_layout(style, 0).unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 30.0), 1)
        .unwrap();
    tree.append_layout(tree.root(), fixed(30.0, 50.0), 2)
        .unwrap();
    let root = tree.root();
    let before = tree.frame([300.0; 2]).unwrap();
    for width in [AvailableSpace::MinContent, AvailableSpace::MaxContent] {
        let r = tree
            .measure_node_with_constraints(
                root,
                LayoutConstraints::new(width, AvailableSpace::Unbounded),
            )
            .unwrap();
        assert_eq!(r.size, [300.0, 50.0]);
    }
    assert_eq!(tree.frame([300.0; 2]).unwrap().revision, before.revision);
    let bounds = LayoutConstraints::new(AvailableSpace::Unbounded, AvailableSpace::Unbounded)
        .with_bounds(BoxConstraints::tight([100.0, 40.0]));
    tree.frame_in_viewport(bounds, [180.0; 2]).unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, [100.0, 40.0]);
    let mut tx = crate::retained::Transaction::default();
    tx.layout_style(
        root,
        LayoutStyle::grid(GridStyle::default())
            .size(Dimension::Px(100.0), Dimension::Auto)
            .container_type(ContainerType::Size)
            .contain_intrinsic_size(None, Some(12.0)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([300.0; 2]).unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, [100.0, 12.0]);
}

#[test]
fn public_grid_content_dependent_item_queries_reject_and_keyed_topology_remains_atomic() {
    let mut ui =
        LogicalUi::new_layout(dynamic_style(0), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(scope, root, LayoutStyle::column(), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(20.0, 10.0)).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    assert!(matches!(
        rules.bind_container(
            &mut ui,
            source,
            target,
            host,
            LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![(80.0, fixed(20.0, 30.0))])
                .unwrap()
        ),
        Err(Error::UnsafeContainerQuery)
    ));
    for keys in [&[1, 2, 3][..], &[3, 1][..], &[2, 3, 1][..]] {
        let (commit, frame) = ui
            .edit_local_frame(
                LayoutConstraints::viewport(180.0, 180.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    let scopes = edit.reconcile_keys(scope, keys)?;
                    let mut nodes = vec![host];
                    for (&key, branch) in keys.iter().zip(scopes) {
                        let old = edit.renders(branch)?.to_vec();
                        let node = if let Some(&node) = old.first() {
                            node
                        } else {
                            edit.append_layout(branch, root, fixed(20.0, 20.0), 0xff0000ff)?
                        };
                        let state = edit.state(branch, 0, || key)?;
                        assert_eq!(*edit.read(state)?, key);
                        nodes.push(node);
                    }
                    edit.set_children(root, nodes.clone())?;
                    Ok(nodes)
                },
            )
            .unwrap();
        let mut full = UiTree::new_layout(dynamic_style(0), 0).unwrap();
        let a = full
            .append_layout(full.root(), LayoutStyle::column(), 0)
            .unwrap();
        full.append_layout(a, fixed(20.0, 10.0), 1).unwrap();
        for _ in keys {
            full.append_layout(full.root(), fixed(20.0, 20.0), 0xff0000ff)
                .unwrap();
        }
        assert!(pixels(frame.resolve()) == pixels(full.frame([180.0; 2]).unwrap()));
        assert_eq!(commit.result.value.len(), keys.len() + 1);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_public_grid_updates_match_complete_reference() {
    let mut tree = UiTree::new_layout(dynamic_style(0), 0).unwrap();
    let mut nodes = vec![];
    for i in 0..4 {
        let host = tree
            .append_layout(
                tree.root(),
                cell(i, 0),
                [0xff0000ff, 0x00ff00ff, 0x0000ffff, 0xffff00ff][i],
            )
            .unwrap();
        tree.append_layout(host, fixed(20.0, 15.0 + i as f32 * 3.0), 0x00ffffff)
            .unwrap();
        nodes.push(host);
    }
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("public Grid adapter: {:?}", gpu.adapter_info());
    for mode in [0, 1, 2, 0] {
        let mut tx = Transaction::default();
        tx.layout_style(tree.root(), dynamic_style(mode));
        for (i, &node) in nodes.iter().enumerate() {
            tx.layout_style(node, cell(i, mode));
        }
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([180.0; 2]).unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&reference(mode), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    gpu.finish().unwrap();
}
