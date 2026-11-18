use super::*;
use std::{cell::Cell, rc::Rc};

fn fr(value: f32) -> GridTrack {
    GridTrack::minmax(GridMinTrack::Px(0.0), GridMaxTrack::Fr(value))
}
fn tracks() -> GridStyle {
    GridStyle::default()
        .columns(vec![fr(1.0).into(), fr(2.0).into()])
        .rows(vec![fr(1.0).into(), fr(2.0).into()])
        .auto_columns(vec![fr(1.0)])
        .auto_rows(vec![fr(1.0)])
}
fn root(direction: LayoutDirection) -> LayoutStyle {
    LayoutStyle::grid(tracks())
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
        .direction(direction)
}
fn item() -> LayoutStyle {
    LayoutStyle::column().min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
}
fn grow() -> LayoutStyle {
    item()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn pixels(packet: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        packet,
        software::SoftwareOptions {
            size: [240, 180],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}
fn reference(size: [f32; 2], direction: LayoutDirection) -> FramePacket {
    let mut full = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(size[0]), LayoutDimension::Px(size[1])),
        0,
    )
    .unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    for (height, colors) in [
        (size[1] / 3.0, [0x0000ffff, 0x00ff00ff]),
        (size[1] * 2.0 / 3.0, [0x888888ff, 0xffff00ff]),
    ] {
        let row = full
            .append_layout(
                full.root(),
                fixed(size[0], height)
                    .flex_direction(LayoutFlexDirection::Row)
                    .direction(direction),
                0,
            )
            .unwrap();
        let cell = full
            .append_layout(row, fixed(size[0] / 3.0, height), colors[0])
            .unwrap();
        full.append_layout(row, fixed(size[0] * 2.0 / 3.0, height), colors[1])
            .unwrap();
        if colors[0] == 0x0000ffff {
            let inner = full
                .append_layout(
                    cell,
                    fixed(size[0] / 3.0, height).flex_direction(LayoutFlexDirection::Row),
                    0,
                )
                .unwrap();
            let leaf = full
                .append_layout(
                    inner,
                    fixed(size[0] / 6.0, height)
                        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                    0,
                )
                .unwrap();
            full.append_layout(leaf, fixed(400.0, 300.0), 0xff0000ff)
                .unwrap();
            full.append_layout(inner, fixed(size[0] / 6.0, height), 0)
                .unwrap();
        }
    }
    full.frame_in_viewport(
        LayoutConstraints::viewport(size[0], size[1]),
        [240.0, 180.0],
    )
    .unwrap()
}
#[test]
fn full_grid_allocation_composes_nested_flex_and_matches_numeric_complete_reference() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_full_grid_allocation_matches_numeric_complete_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("full Grid allocation adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
        for local in [false, true] {
            let mut ui =
                LogicalUi::new_layout(root(direction), 0, BackendCapabilities::default()).unwrap();
            let owner = ui.root_scope();
            let cell = ui
                .append_render_layout(owner, ui.root_render(), item(), 0x0000ffff)
                .unwrap();
            for color in [0x00ff00ff, 0x888888ff, 0xffff00ff] {
                ui.append_render_layout(owner, ui.root_render(), item(), color)
                    .unwrap();
            }
            let inner = ui
                .append_render_layout(
                    owner,
                    cell,
                    grow().flex_direction(LayoutFlexDirection::Row),
                    0,
                )
                .unwrap();
            let host = ui.append_render_layout(owner, inner, grow(), 0).unwrap();
            ui.append_render_layout(owner, inner, grow(), 0).unwrap();
            // The grid cell's Column must assign its sole row the full height.
            let mut old = None;
            for size in [[120.0, 90.0], [180.0, 120.0], [120.0, 90.0]] {
                let build = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(
                            input.known_size,
                            LayoutSize::new(Some(size[0] / 6.0), Some(size[1] / 3.0))
                        );
                        assert_eq!(input.known_size_is_definite, LayoutSize::new(true, true));
                        let branch = edit.reconcile_keys(owner, &[size[0] as u64])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                        }
                        Ok(())
                    })
                };
                let packet = if local {
                    ui.edit_local_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [240.0, 180.0],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap()
                    .1
                    .resolve()
                } else {
                    ui.edit_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [240.0, 180.0],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap()
                    .1
                };
                let full = reference(size, direction);
                assert_eq!(pixels(packet.clone()), pixels(full.clone()));
                #[cfg(feature = "retained-wgpu")]
                if let Some(gpu) = &mut gpu {
                    gpu.submit(&packet, [240, 180], 1.0).unwrap();
                    assert_eq!(
                        gpu.image().unwrap().as_raw(),
                        gpu.reference_image(&full, [240, 180], 1.0)
                            .unwrap()
                            .as_raw()
                    );
                }
                old.get_or_insert_with(|| (packet.clone(), pixels(packet)));
            }
            drop(ui);
            let (packet, expected) = old.unwrap();
            assert_eq!(pixels(packet), expected);
        }
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn full_grid_allocation_preserves_track_area_input_spans_percentages_and_implicit_rows() {
    for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
        let grid = GridStyle::default()
            .columns(vec![
                GridTrack::px(40.0).into(),
                GridTrack::percent(0.5).into(),
            ])
            .rows(vec![GridTrack::px(30.0).into()])
            .auto_columns(vec![GridTrack::px(20.0)])
            .auto_rows(vec![GridTrack::px(25.0)]);
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::grid(grid)
                .direction(direction)
                .size(LayoutDimension::Px(120.0), LayoutDimension::Px(90.0)),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let owner = ui.root_scope();
        let host = ui
            .append_render_layout(
                owner,
                ui.root_render(),
                item()
                    .grid_column(GridPlacement::Line(1), GridPlacement::Span(2))
                    .grid_row(GridPlacement::Line(2), GridPlacement::Auto),
                0,
            )
            .unwrap();
        ui.edit_frame(
            LayoutConstraints::viewport(120.0, 90.0),
            [240.0, 180.0],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.sizing_mode, LayoutSizingMode::Inherent);
                    assert_eq!(input.known_size, LayoutSize::new(Some(100.0), Some(25.0)));
                    assert_eq!(
                        input.percentage_basis,
                        LayoutSize::new(Some(100.0), Some(25.0))
                    );
                    assert_eq!(input.constraints, LayoutConstraints::viewport(100.0, 25.0));
                    edit.append_layout(owner, host, fixed(400.0, 300.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
        assert_eq!(ui.layout_result(host).unwrap().size, [100.0, 25.0]);
    }
}

#[test]
fn full_grid_allocation_rejects_intrinsic_tracks_baseline_feedback_and_unknown_implicit_axes() {
    for case in 0..7 {
        let mut grid = tracks();
        let mut child = item();
        match case {
            0 => grid.rows = vec![GridTrack::auto().into()],
            1 => grid.auto_rows.clear(),
            2 => grid.columns = vec![GridTrack::fr(1.0).into()],
            3 => child.align_self = LayoutAlignSelf::Baseline.into(),
            4 => child.aspect_ratio = Some(2.0),
            5 => child.justify_self = LayoutAlignSelf::Start.into(),
            _ => child.size.height = LayoutDimension::MaxContent,
        }
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::grid(grid)
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let owner = ui.root_scope();
        let host = ui
            .append_render_layout(owner, ui.root_render(), child, 0)
            .unwrap();
        assert!(
            matches!(
                ui.edit_frame(
                    LayoutConstraints::viewport(120.0, 90.0),
                    [240.0, 180.0],
                    |_| Ok(()),
                    |(), edit| {
                        edit.allocation_builder(host, |_, _| -> Result<(), Error> {
                            panic!("unproved Grid callback ran: {case}")
                        })
                    }
                ),
                Err(Error::UnsafeConstraintBuilder)
            ),
            "case {case}"
        );
    }
}

#[test]
fn full_grid_allocation_late_same_area_track_placement_order_changes_roll_back() {
    for case in 0..3 {
        let mut ui = LogicalUi::new_layout(
            root(LayoutDirection::Ltr),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let owner = ui.root_scope();
        let root = ui.root_render();
        let host = ui.append_render_layout(owner, root, item(), 0).unwrap();
        let sibling = ui.append_render_layout(owner, root, item(), 0).unwrap();
        let source = ui.state(owner, 0, || 0u32).unwrap();
        let constraints = LayoutConstraints::viewport(120.0, 90.0);
        let before = ui.frame_in_viewport(constraints, [240.0, 180.0]).unwrap();
        let stats = ui.stats();
        assert!(matches!(
            ui.edit_frame(
                constraints,
                [240.0, 180.0],
                |tx| {
                    tx.set(source, 1)?;
                    Ok(())
                },
                |(), edit| {
                    edit.allocation_builder(host, |_, edit| {
                        edit.append_layout(owner, host, fixed(1.0, 1.0), 0xff0000ff)?;
                        match case {
                            0 => edit.properties().layout_style(
                                root,
                                LayoutStyle::grid(tracks().auto_rows(vec![GridTrack::px(500.0)]))
                                    .size(
                                        LayoutDimension::Percent(1.0),
                                        LayoutDimension::Percent(1.0),
                                    ),
                            ),
                            1 => edit.properties().layout_style(
                                sibling,
                                item().grid_column(GridPlacement::Line(2), GridPlacement::Auto),
                            ),
                            _ => edit.properties().layout_style(sibling, item().order(-1)),
                        }
                        Ok(())
                    })
                }
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(ui.stats(), stats);
        assert_eq!(
            pixels(before),
            pixels(ui.frame_in_viewport(constraints, [240.0, 180.0]).unwrap())
        );
    }
}

#[test]
fn full_grid_allocation_producers_accept_source_resize_and_nested_grid_under_flex() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let grid = ui
        .append_render_layout(
            owner,
            ui.root_render(),
            root(LayoutDirection::Ltr)
                .flex_basis(LayoutDimension::Px(0.0))
                .flex_grow(1.0)
                .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0)),
            0,
        )
        .unwrap();
    let host = ui.append_render_layout(owner, grid, item(), 0).unwrap();
    let source = ui.state(owner, 0, || 10u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        let LayoutBuildInput::Full(input) = input else {
            panic!()
        };
        count.set(count.get() + 1);
        assert_eq!(input.known_size.width, input.percentage_basis.width);
        assert_eq!(input.known_size.height, input.percentage_basis.height);
        let value = *edit.read(source)?;
        let branch = edit.reconcile_keys(owner, &[value as u64])?[0];
        if edit.renders(branch)?.is_empty() {
            edit.append_layout(branch, host, fixed(20.0, value as f32), 0xff0000ff)?;
        }
        Ok(())
    });
    ui.edit_local_frame(
        LayoutConstraints::viewport(120.0, 90.0),
        [240.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Allocation, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [40.0, 30.0]);
    ui.transaction(|tx| tx.set(source, 40)).unwrap();
    assert_eq!(calls.get(), 2);
    ui.local_frame_in_viewport(LayoutConstraints::viewport(180.0, 120.0), [240.0, 180.0])
        .unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(ui.layout_result(host).unwrap().size, [60.0, 40.0]);
    ui.transaction(|tx| tx.set(source, 40)).unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn full_grid_allocation_unrelated_sibling_content_can_change_after_observation() {
    let mut ui = LogicalUi::new_layout(
        root(LayoutDirection::Ltr),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let root_id = ui.root_render();
    let host = ui.append_render_layout(owner, root_id, item(), 0).unwrap();
    let sibling = ui
        .append_render_layout(owner, root_id, item(), 0x00ff00ff)
        .unwrap();
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let (_, packet) = ui
        .edit_frame(
            constraints,
            [240.0, 180.0],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_size, LayoutSize::new(Some(40.0), Some(30.0)));
                    edit.append_layout(owner, host, fixed(40.0, 30.0), 0xff0000ff)?;
                    // Numeric tracks ignore this other cell's intrinsic
                    // contribution, so it is not a measurement prerequisite.
                    edit.append_layout(owner, sibling, fixed(400.0, 300.0), 0x0000ffff)?;
                    Ok(())
                })
            },
        )
        .unwrap();
    let mut full = UiTree::new_layout(root(LayoutDirection::Ltr), 0).unwrap();
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let host_ref = full.append_layout(full.root(), item(), 0).unwrap();
    let sibling_ref = full.append_layout(full.root(), item(), 0x00ff00ff).unwrap();
    full.append_layout(host_ref, fixed(40.0, 30.0), 0xff0000ff)
        .unwrap();
    full.append_layout(sibling_ref, fixed(400.0, 300.0), 0x0000ffff)
        .unwrap();
    let reference = full.frame_in_viewport(constraints, [240.0, 180.0]).unwrap();
    assert_eq!(
        ui.layout_result(host).unwrap(),
        full.layout_result(host_ref).unwrap()
    );
    assert_eq!(pixels(packet), pixels(reference));
}

#[test]
fn full_grid_allocation_automatic_repeats_nested_grid_and_style_box_model_use_real_slots() {
    for repeat in [
        GridRepetition::Count(2),
        GridRepetition::AutoFit,
        GridRepetition::AutoFill,
    ] {
        for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
            let track = GridTrack::minmax(GridMinTrack::Px(40.0), GridMaxTrack::Fr(1.0));
            let grid = GridStyle::default()
                .columns(vec![GridTemplate::repeat(repeat, vec![track])])
                .rows(vec![GridTrack::px(80.0).into()])
                .auto_columns(vec![track])
                .auto_rows(vec![GridTrack::px(80.0)]);
            let style = LayoutStyle::grid(grid)
                .direction(direction)
                .size(LayoutDimension::Px(120.0), LayoutDimension::Px(80.0));
            let mut ui =
                LogicalUi::new_layout(style.clone(), 0, BackendCapabilities::default()).unwrap();
            let owner = ui.root_scope();
            let nested = ui
                .append_render_layout(
                    owner,
                    ui.root_render(),
                    root(direction)
                        .size(LayoutDimension::Auto, LayoutDimension::Auto)
                        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0)),
                    0,
                )
                .unwrap();
            // Count2 makes60px columns; auto-repeat makes three40px columns.
            let width = if repeat == GridRepetition::Count(2) {
                60.0
            } else {
                40.0
            };
            for _ in 0..2 {
                ui.append_render_layout(owner, ui.root_render(), item(), 0)
                    .unwrap();
            }
            let child_style = item()
                .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
                .margin(LayoutEdges::all(LayoutLength::Px(1.0)))
                .size(LayoutDimension::Percent(0.5), LayoutDimension::Percent(0.5));
            let host = ui
                .append_render_layout(owner, nested, child_style.clone(), 0x0000ffff)
                .unwrap();
            let constraints = LayoutConstraints::viewport(120.0, 80.0);
            let (_, packet) = ui
                .edit_frame(
                    constraints,
                    [240.0, 180.0],
                    |_| Ok(()),
                    |(), edit| {
                        edit.allocation_builder(host, |input, edit| {
                            let area: [f32; 2] = [width / 3.0, 80.0 / 3.0];
                            // Track boundary subtraction and direct division
                            // can differ by one f32 ULP. The complete kernel
                            // reference below still compares exact layouts.
                            for (actual, expected) in [
                                (input.percentage_basis.width, area[0]),
                                (input.percentage_basis.height, area[1]),
                                (input.known_size.width, (area[0] * 0.5).max(4.0)),
                                (input.known_size.height, (area[1] * 0.5).max(4.0)),
                            ] {
                                assert!((actual.unwrap() - expected).abs() < 0.001);
                            }
                            assert!(
                                (input.constraints.width.resolve_definite().unwrap()
                                    - (area[0] - 2.0))
                                    .abs()
                                    < 0.001
                            );
                            edit.append_layout(owner, host, fixed(100.0, 100.0), 0xff0000ff)
                        })
                    },
                )
                .unwrap();
            let mut full = UiTree::new_layout(style, 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let nested_ref = full
                .append_layout(
                    full.root(),
                    root(direction)
                        .size(LayoutDimension::Auto, LayoutDimension::Auto)
                        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0)),
                    0,
                )
                .unwrap();
            for _ in 0..2 {
                full.append_layout(full.root(), item(), 0).unwrap();
            }
            let child = full
                .append_layout(nested_ref, child_style, 0x0000ffff)
                .unwrap();
            full.append_layout(child, fixed(100.0, 100.0), 0xff0000ff)
                .unwrap();
            let reference = full.frame_in_viewport(constraints, [240.0, 180.0]).unwrap();
            assert_eq!(
                ui.layout_result(host).unwrap(),
                full.layout_result(child).unwrap()
            );
            assert_eq!(pixels(packet), pixels(reference));
        }
    }
}
