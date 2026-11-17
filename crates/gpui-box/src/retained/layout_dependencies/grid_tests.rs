use super::{row_tests::*, *};
use crate::retained::{logical::LogicalUi, reactive::Property, responsive::*};

fn minfr(min: f32, fr: f32) -> GridTrack {
    GridTrack::minmax(GridMinTrack::Px(min), GridMaxTrack::Fr(fr))
}
fn tracks(template: &str, flow: &str) -> GridStyle {
    GridStyle::default()
        .columns(match template {
            "fraction" => vec![minfr(0.0, 1.0).into(), minfr(20.0, 2.0).into()],
            "bounded" => vec![
                GridTrack::minmax(GridMinTrack::Px(30.0), GridMaxTrack::Px(90.0)).into(),
                GridTrack::minmax(GridMinTrack::Px(10.0), GridMaxTrack::Px(120.0)).into(),
            ],
            "percent" => vec![GridTrack::percent(0.3).into(), minfr(20.0, 1.0).into()],
            "repeat" => vec![GridTemplate::repeat(
                GridRepetition::Count(2),
                vec![minfr(20.0, 1.0)],
            )],
            "fit" | "fill" => vec![GridTemplate::repeat(
                if template == "fit" {
                    GridRepetition::AutoFit
                } else {
                    GridRepetition::AutoFill
                },
                vec![minfr(60.0, 1.0)],
            )],
            _ => unreachable!(),
        })
        .rows(vec![GridTrack::auto().into(), GridTrack::auto().into()])
        .auto_columns(vec![minfr(20.0, 1.0)])
        .auto_flow(match flow {
            "row" => GridAutoFlow::Row,
            "row dense" => GridAutoFlow::RowDense,
            "column" => GridAutoFlow::Column,
            "column dense" => GridAutoFlow::ColumnDense,
            _ => unreachable!(),
        })
}
fn grid() -> LayoutStyle {
    LayoutStyle::grid(tracks("fraction", "row"))
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .align_items(LayoutAlignItems::Start)
}
fn column() -> LayoutStyle {
    LayoutStyle::column()
}

#[test]
fn grid_inline_proof_and_geometry_match_browser_when_content_heights_change() {
    let mut count = 0;
    let mut failures = vec![];
    for line in include_str!(
        "../../../../../docs/performance/2026-11-17-grid-inline-allocation/browser-reference.tsv"
    )
    .lines()
    {
        let f: Vec<_> = line.split('\t').collect();
        let w: f32 = f[3].parse().unwrap();
        let style = LayoutStyle::grid(tracks(f[0], f[1]))
            .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
            .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
            .gap(LayoutLength::Px(4.0), LayoutLength::Px(4.0))
            .align_items(LayoutAlignItems::Start)
            .align_content(LayoutAlignContent::Start)
            .justify_content(LayoutJustifyContent::Start)
            .direction(if f[2] == "true" {
                LayoutDirection::Rtl
            } else {
                LayoutDirection::Ltr
            });
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let mut nodes = vec![];
        for i in 0..4 {
            let mut style = column()
                .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
                .margin(LayoutEdges {
                    left: LayoutLength::Px(2.0),
                    right: LayoutLength::Px(2.0),
                    top: LayoutLength::Px(0.0),
                    bottom: LayoutLength::Px(0.0),
                });
            style.border = LayoutEdges::all(LayoutLength::Px(1.0));
            if f[5] == "content-box" {
                style.box_sizing = LayoutBoxSizing::ContentBox;
            }
            if f[4] == "true" {
                match i {
                    0 => {
                        style = style
                            .grid_column(GridPlacement::Span(2), GridPlacement::Auto)
                            .order(1)
                    }
                    1 => {
                        style = style
                            .grid_column(GridPlacement::Line(-2), GridPlacement::Line(-1))
                            .grid_row(GridPlacement::Line(1), GridPlacement::Auto)
                    }
                    2 => style = style.grid_row(GridPlacement::Span(2), GridPlacement::Auto),
                    _ => {}
                }
            }
            let node = tree.append_layout(tree.root(), style, 0).unwrap();
            nodes.push(node);
            tree.append_layout(
                node,
                fixed(
                    400.0,
                    if f[6] == "true" {
                        [80.0, 10.0, 100.0, 30.0][i]
                    } else {
                        [20.0, 60.0, 40.0, 10.0][i]
                    },
                ),
                0,
            )
            .unwrap();
        }
        for &node in &nodes {
            assert!(
                independent_inline_allocation_chain(&tree, node, &tree.node(node).unwrap().style)
                    .unwrap(),
                "{:?}",
                &f[..7]
            );
        }
        tree.frame_in_viewport(constraints(w), [400.0; 2]).unwrap();
        let r = tree.layout_result(tree.root()).unwrap();
        let mut actual = vec![r.size[0], r.size[1]];
        for node in nodes {
            let r = tree.layout_result(node).unwrap();
            actual.extend([r.location[0], r.location[1], r.size[0], r.size[1]]);
        }
        let expected: Vec<f32> = f[7..].iter().map(|v| v.parse().unwrap()).collect();
        if actual
            .iter()
            .zip(expected)
            .any(|(a, b)| (a - b).abs() > 0.06)
        {
            failures.push(format!("{:?}: {actual:?}", &f[..7]));
        }
        count += 1;
    }
    assert_eq!(count, 1152);
    assert!(
        failures.is_empty(),
        "{} failures: {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

fn reference(w: f32, nested: bool) -> FramePacket {
    let mut tree = UiTree::new_layout(grid(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let mut host = tree
        .append_layout(tree.root(), column(), 0x0000ffff)
        .unwrap();
    tree.append_layout(tree.root(), fixed(10.0, 10.0), 0x00ff00ff)
        .unwrap();
    if nested {
        let inner = tree.append_layout(host, grid(), 0).unwrap();
        host = tree.append_layout(inner, column(), 0x0000ffff).unwrap();
    }
    tree.append_layout(
        host,
        fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}
#[test]
fn grid_item_builder_composes_nested_grid_and_full_cpu_reference() {
    for nested in [false, true] {
        for local in [false, true] {
            let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
            let scope = ui.root_scope();
            let root = ui.root_render();
            let mut host = ui
                .append_render_layout(scope, root, column(), 0x0000ffff)
                .unwrap();
            ui.append_render_layout(scope, root, fixed(10.0, 10.0), 0x00ff00ff)
                .unwrap();
            if nested {
                let inner = ui.append_render_layout(scope, host, grid(), 0).unwrap();
                host = ui
                    .append_render_layout(scope, inner, column(), 0x0000ffff)
                    .unwrap();
            }
            let mut old = None;
            for w in [120.0, 240.0, 160.0, 120.0] {
                let build = |(), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        let width = if nested { w / 9.0 } else { w / 3.0 };
                        assert!((input.known_width.unwrap() - width).abs() < 0.01);
                        assert!(input.known_width_is_definite);
                        let branch = edit.reconcile_keys(scope, &[u64::from(w >= 180.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?;
                        }
                        Ok(())
                    })
                };
                let frame = if local {
                    ui.edit_local_frame(constraints(w), [180.0; 2], |_| Ok(()), build)
                        .unwrap()
                        .1
                        .resolve()
                } else {
                    ui.edit_frame(constraints(w), [180.0; 2], |_| Ok(()), build)
                        .unwrap()
                        .1
                };
                assert!(
                    pixels(frame.clone()) == pixels(reference(w, nested)),
                    "nested={nested} w={w}"
                );
                old.get_or_insert(frame);
            }
            let old = old.unwrap();
            let expected = pixels(old.clone());
            drop(ui);
            assert_eq!(pixels(old), expected);
        }
    }
}

#[test]
fn grid_inline_allocations_compose_row_percentage_basis_and_numeric_item_alignment() {
    for preferred in [false, true] {
        let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
        let parent = ui.root_render();
        let scope = ui.root_scope();
        let style = if preferred {
            column()
                .size(LayoutDimension::Percent(0.5), LayoutDimension::Auto)
                .justify_self(LayoutAlignSelf::Center)
        } else {
            column()
        };
        let host = ui.append_render_layout(scope, parent, style, 0).unwrap();
        let row = ui
            .append_render_layout(
                scope,
                host,
                LayoutStyle::row()
                    .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                    .align_items(LayoutAlignItems::Start),
                0,
            )
            .unwrap();
        let item = || {
            column()
                .flex_basis(LayoutDimension::Percent(0.25))
                .flex_grow(1.0)
                .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        };
        let inner = ui.append_render_layout(scope, row, item(), 0).unwrap();
        ui.append_render_layout(
            scope,
            row,
            item().size(LayoutDimension::Auto, LayoutDimension::Px(10.0)),
            0,
        )
        .unwrap();
        ui.edit_frame(
            constraints(240.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(inner, |input, edit| {
                    assert!(
                        (input.known_width.unwrap() - if preferred { 20.0 } else { 40.0 }).abs()
                            < 0.01
                    );
                    assert!(input.known_width_is_definite);
                    edit.append_layout(scope, inner, fixed(400.0, 30.0), 1)
                })
            },
        )
        .unwrap();
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            [if preferred { 40.0 } else { 80.0 }, 30.0]
        );
    }
}

#[test]
fn grid_item_queries_stage_later_sibling_placement_before_consumption() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let outer = ui.root_render();
    let parent = ui.append_render_layout(scope, outer, grid(), 0).unwrap();
    let host = ui.append_render_layout(scope, parent, column(), 0).unwrap();
    let sibling = ui.append_render_layout(scope, parent, column(), 0).unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let side = ui.state(scope, 1, column).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            source,
            target,
            host,
            LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                .unwrap(),
        )
        .unwrap();
    let displaced = column()
        .grid_column(GridPlacement::Line(1), GridPlacement::Auto)
        .grid_row(GridPlacement::Line(1), GridPlacement::Auto);
    rules
        .bind_container(
            &mut ui,
            side,
            sibling,
            outer,
            LayoutBreakpointStyles::new(column(), vec![(180.0, displaced)]).unwrap(),
        )
        .unwrap();
    for (w, expected) in [(120.0, 40.0), (180.0, 120.0), (240.0, 160.0), (120.0, 40.0)] {
        rules
            .local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([500.0; 2]),
                constraints(w),
                [180.0; 2],
            )
            .unwrap();
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            [expected, if expected < 90.0 { 20.0 } else { 60.0 }]
        );
    }
}

#[test]
fn grid_item_query_captured_session_resettles_on_sibling_source_and_p0() {
    let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let parent = ui.root_render();
    let host = ui.append_render_layout(scope, parent, column(), 0).unwrap();
    let sibling = ui.append_render_layout(scope, parent, column(), 0).unwrap();
    let target = ui
        .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let side = ui.state(scope, 1, column).unwrap();
    ui.bind(side, Property::layout_style(sibling)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                0,
                source,
                target,
                LayoutQuery::ContainerInline(host),
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])?,
            )
        },
    )
    .unwrap();
    ui.frame_with_environment(
        &LayoutEnvironment::new([500.0; 2]),
        constraints(180.0),
        [180.0; 2],
    )
    .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [60.0, 20.0]);
    let changed = column()
        .grid_column(GridPlacement::Line(1), GridPlacement::Auto)
        .grid_row(GridPlacement::Line(1), GridPlacement::Auto);
    ui.transaction(|tx| tx.set(side, changed.clone())).unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 60.0]);
    let rev = ui.published_frame().unwrap().revision;
    ui.transaction(|tx| tx.set(side, changed)).unwrap();
    assert_eq!(ui.published_frame().unwrap().revision, rev);
}

#[test]
fn grid_inline_proof_rejects_intrinsic_implicit_rows_ratio_and_unanchored_inputs() {
    for mode in 0..12 {
        let mut style = grid();
        match mode {
            0 => {
                style.grid = Some(std::sync::Arc::new(
                    tracks("fraction", "row").auto_columns(vec![]),
                ))
            }
            1 => {
                style.grid = Some(std::sync::Arc::new(
                    tracks("fraction", "row").columns(vec![GridTrack::fr(1.0).into()]),
                ))
            }
            2 => {
                style.grid = Some(std::sync::Arc::new(tracks("fraction", "row").rows(vec![
                    GridTemplate::repeat(GridRepetition::AutoFit, vec![GridTrack::px(30.0)]),
                ])))
            }
            _ => {}
        }
        let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let parent = ui.root_render();
        let host = ui.append_render_layout(scope, parent, column(), 0).unwrap();
        let mut side = column();
        if mode == 3 {
            side.aspect_ratio = Some(2.0);
        }
        if mode == 4 {
            side.justify_self = LayoutAlignSelf::Start.into();
        }
        if mode == 8 {
            side.margin.left = LayoutLength::Auto;
        }
        if mode == 9 {
            side.size.width = LayoutDimension::MaxContent;
        }
        if mode == 10 {
            side.min_size.width = LayoutDimension::MinContent;
        }
        if mode == 11 {
            side.max_size.width = LayoutDimension::MaxContent;
        }
        let sibling = ui.append_render_layout(scope, parent, side, 0).unwrap();
        if mode == 5 {
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties()
                        .image(sibling, Some(ImageResource::rgba8([2, 1], vec![255; 8])?));
                    Ok(())
                },
            )
            .unwrap();
        }
        let input = if matches!(mode, 6 | 7) {
            LayoutConstraints::new(
                if mode == 6 {
                    LayoutAvailableSpace::Unbounded
                } else {
                    LayoutAvailableSpace::MinContent
                },
                LayoutAvailableSpace::Unbounded,
            )
        } else {
            constraints(180.0)
        };
        let mut called = false;
        assert!(
            matches!(
                ui.edit_frame(
                    input,
                    [180.0; 2],
                    |_| Ok(()),
                    |(), edit| edit.inline_allocation_builder(host, |_, _| {
                        called = true;
                        Ok(())
                    })
                ),
                Err(Error::UnsafeConstraintBuilder)
            ),
            "mode={mode}"
        );
        assert!(!called);
    }
}

#[test]
fn grid_inline_observation_rejects_equal_allocation_track_or_sibling_changes_atomically() {
    for mode in 0..3 {
        let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let parent = ui.root_render();
        let host = ui.append_render_layout(scope, parent, column(), 0).unwrap();
        let sibling = ui.append_render_layout(scope, parent, column(), 0).unwrap();
        let source = ui.state(scope, 0, grid).unwrap();
        ui.bind(source, Property::layout_style(parent)).unwrap();
        let old = ui
            .frame_in_viewport(constraints(180.0), [180.0; 2])
            .unwrap();
        let stats = ui.stats();
        let result = ui.edit_local_frame(
            constraints(180.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |_, edit| {
                    match mode {
                        0 => edit.properties().layout_style(
                            sibling,
                            column().size(LayoutDimension::Auto, LayoutDimension::Px(15.0)),
                        ),
                        1 => {
                            let mut s = grid();
                            s.grid = Some(std::sync::Arc::new(
                                tracks("fraction", "row").auto_rows(vec![GridTrack::px(20.0)]),
                            ));
                            edit.properties().layout_style(parent, s);
                        }
                        _ => edit.set_children(parent, vec![sibling, host])?,
                    }
                    Ok(())
                })
            },
        );
        assert!(
            matches!(result, Err(Error::UnsafeConstraintBuilder)),
            "mode={mode}"
        );
        assert_eq!(ui.read(source).unwrap(), &grid());
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

#[test]
fn grid_query_sibling_alternatives_and_computed_feedback_reject() {
    for cycle in [false, true] {
        let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let parent = ui.root_render();
        let host = ui.append_render_layout(scope, parent, column(), 0).unwrap();
        let sibling = ui.append_render_layout(scope, parent, column(), 0).unwrap();
        let target = ui
            .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
            .unwrap();
        let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
        let side = ui.state(scope, 1, column).unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        rules
            .bind_container(
                &mut ui,
                source,
                target,
                host,
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                    .unwrap(),
            )
            .unwrap();
        if cycle {
            let feedback = ui
                .computed(scope, 2, &[source.dependency()], move |input| {
                    let _ = input.read(source)?;
                    Ok(column())
                })
                .unwrap();
            ui.bind(feedback, Property::layout_style(sibling)).unwrap();
            assert!(matches!(
                rules.frame_in_viewport(
                    &mut ui,
                    &LayoutEnvironment::new([500.0; 2]),
                    constraints(180.0),
                    [180.0; 2]
                ),
                Err(Error::UnsafeContainerQuery)
            ));
        } else {
            assert!(matches!(
                rules.bind(
                    &mut ui,
                    side,
                    sibling,
                    LayoutBreakpointStyles::new(
                        column(),
                        vec![(900.0, column().justify_self(LayoutAlignSelf::Start))]
                    )
                    .unwrap()
                ),
                Err(Error::UnsafeContainerQuery)
            ));
        }
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_grid_inline_builder_matches_complete_reference() {
    let mut ui = LogicalUi::new_layout(grid(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let parent = ui.root_render();
    let host = ui
        .append_render_layout(scope, parent, column(), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(scope, parent, fixed(10.0, 10.0), 0x00ff00ff)
        .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("grid inline adapter: {:?}", gpu.adapter_info());
    for w in [120.0, 240.0, 160.0, 120.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |_, edit| {
                        let branch = edit.reconcile_keys(scope, &[u64::from(w >= 180.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if w < 180.0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?;
                        }
                        Ok(())
                    })
                },
            )
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&reference(w, false), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    gpu.finish().unwrap();
}
