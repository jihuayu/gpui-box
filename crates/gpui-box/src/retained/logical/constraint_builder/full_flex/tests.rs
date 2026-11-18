use super::*;

fn root(direction: LayoutFlexDirection) -> LayoutStyle {
    LayoutStyle::row()
        .flex_direction(direction)
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn item(grow: f32) -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(grow)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
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
fn row(direction: LayoutFlexDirection) -> bool {
    matches!(
        direction,
        LayoutFlexDirection::Row | LayoutFlexDirection::RowReverse
    )
}

#[test]
fn full_flex_two_axis_builders_match_complete_numeric_flex_and_retain_old_frames() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_full_flex_two_axis_builders_match_complete_numeric_flex() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("two-axis Flex adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        for local in [false, true] {
            let mut ui =
                LogicalUi::new_layout(root(direction), 0, BackendCapabilities::default()).unwrap();
            let scope = ui.root_scope();
            let host = ui
                .append_render_layout(scope, ui.root_render(), item(1.0), 0x0000ffff)
                .unwrap();
            ui.append_render_layout(scope, ui.root_render(), item(2.0), 0x00ff00ff)
                .unwrap();
            let mut old = None;
            for size in [[120.0, 90.0], [150.0, 120.0], [120.0, 90.0]] {
                let expected = if row(direction) {
                    [size[0] / 3.0, size[1]]
                } else {
                    [size[0], size[1] / 3.0]
                };
                let build = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(input.sizing_mode, LayoutSizingMode::Content);
                        assert_eq!(
                            input.known_size,
                            LayoutSize::new(Some(expected[0]), Some(expected[1]))
                        );
                        assert_eq!(input.known_size_is_definite, LayoutSize::new(true, true));
                        assert_eq!(
                            input.percentage_basis,
                            LayoutSize::new(Some(size[0]), Some(size[1]))
                        );
                        assert_eq!(input.constraints.bounds, BoxConstraints::UNBOUNDED);
                        let branch = edit.reconcile_keys(scope, &[u64::from(size[0] > 120.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                        }
                        Ok(())
                    })
                };
                let packet = if local {
                    ui.edit_local_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [180.0; 2],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap()
                    .1
                    .resolve()
                } else {
                    ui.edit_frame(
                        LayoutConstraints::viewport(size[0], size[1]),
                        [180.0; 2],
                        |_| Ok(()),
                        build,
                    )
                    .unwrap()
                    .1
                };
                assert_eq!(ui.layout_result(host).unwrap().size, expected);
                let mut full = UiTree::new_layout(root(direction), 0).unwrap();
                full.set_frame_build_policy(FrameBuildPolicy::Full);
                let reference_host = full
                    .append_layout(full.root(), fixed(expected[0], expected[1]), 0x0000ffff)
                    .unwrap();
                full.append_layout(reference_host, fixed(400.0, 300.0), 0xff0000ff)
                    .unwrap();
                let other = if row(direction) {
                    [size[0] - expected[0], size[1]]
                } else {
                    [size[0], size[1] - expected[1]]
                };
                full.append_layout(full.root(), fixed(other[0], other[1]), 0x00ff00ff)
                    .unwrap();
                let reference = full
                    .frame_in_viewport(LayoutConstraints::viewport(size[0], size[1]), [180.0; 2])
                    .unwrap();
                assert_eq!(pixels(packet.clone()), pixels(reference.clone()));
                #[cfg(feature = "retained-wgpu")]
                if let Some(gpu) = &mut gpu {
                    gpu.submit(&packet, [180, 180], 1.0).unwrap();
                    assert_eq!(
                        gpu.image().unwrap().as_raw(),
                        gpu.reference_image(&reference, [180, 180], 1.0)
                            .unwrap()
                            .as_raw()
                    );
                }
                let tree = ui.reactive().tree();
                assert_eq!(
                    tree.layout
                        .tracked_layout_input(tree.node(host).unwrap().layout)
                        .unwrap(),
                    None
                );
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
fn full_flex_nested_row_column_plans_and_automatic_producers_share_known_both_axes() {
    use std::{cell::Cell, rc::Rc};
    let mut ui = LogicalUi::new_layout(
        root(LayoutFlexDirection::Row),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let owner = ui.root_scope();
    let outer = ui
        .append_render_layout(owner, ui.root_render(), item(1.0), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(owner, ui.root_render(), item(1.0), 0x00ff00ff)
        .unwrap();
    let host = ui.append_render_layout(owner, outer, item(1.0), 0).unwrap();
    ui.append_render_layout(owner, outer, item(2.0), 0).unwrap();
    let model = ui.state(owner, 0, || 10u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let producer = LayoutProducer::new(move |input, edit| {
        let LayoutBuildInput::Full(input) = input else {
            panic!()
        };
        count.set(count.get() + 1);
        assert_eq!(input.known_size_is_definite, LayoutSize::new(true, true));
        assert_eq!(
            input.known_size.width.unwrap() * 0.5,
            input.known_size.height.unwrap()
        );
        let value = *edit.read(model)?;
        let branch = edit.reconcile_keys(owner, &[value as u64])?[0];
        if edit.renders(branch)?.is_empty() {
            edit.append_layout(branch, host, fixed(20.0, value as f32), 0xff0000ff)?;
        }
        Ok(())
    });
    ui.edit_local_frame(
        LayoutConstraints::viewport(120.0, 90.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.layout_producer(owner, 0, host, LayoutBuildKind::Allocation, &[], producer)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(ui.layout_result(host).unwrap().size, [60.0, 30.0]);
    let first = ui.published_local_frame().unwrap().clone();
    ui.transaction(|tx| tx.set(model, 10)).unwrap();
    assert_eq!(calls.get(), 1);
    assert!(first.same(ui.published_local_frame().unwrap()));
    ui.transaction(|tx| tx.set(model, 40)).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size, [60.0, 30.0]);
    ui.local_frame_in_viewport(LayoutConstraints::viewport(160.0, 120.0), [180.0; 2])
        .unwrap();
    assert_eq!(calls.get(), 3);
    assert_eq!(ui.layout_result(host).unwrap().size, [80.0, 40.0]);
    let mut reference = UiTree::new_layout(root(LayoutFlexDirection::Row), 0).unwrap();
    reference.set_frame_build_policy(FrameBuildPolicy::Full);
    let column = reference
        .append_layout(
            reference.root(),
            fixed(80.0, 120.0).flex_direction(LayoutFlexDirection::Column),
            0x0000ffff,
        )
        .unwrap();
    reference
        .append_layout(reference.root(), fixed(80.0, 120.0), 0x00ff00ff)
        .unwrap();
    let leaf = reference
        .append_layout(column, fixed(80.0, 40.0), 0)
        .unwrap();
    reference
        .append_layout(leaf, fixed(20.0, 40.0), 0xff0000ff)
        .unwrap();
    reference
        .append_layout(column, fixed(80.0, 80.0), 0)
        .unwrap();
    assert_eq!(
        pixels(ui.published_local_frame().unwrap().resolve()),
        pixels(
            reference
                .frame_in_viewport(LayoutConstraints::viewport(160.0, 120.0), [180.0; 2])
                .unwrap()
        )
    );
}

#[test]
fn full_flex_wrapping_percentages_insets_and_gutters_preserve_real_inputs() {
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        for wrap in [LayoutFlexWrap::Wrap, LayoutFlexWrap::WrapReverse] {
            for rtl in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
                let style = root(direction)
                    .flex_wrap(wrap)
                    .direction(rtl)
                    .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
                    .gap(LayoutLength::Px(4.0), LayoutLength::Px(4.0));
                let child = item(0.0)
                    .flex_basis(LayoutDimension::Percent(0.6))
                    .size(LayoutDimension::Percent(0.3), LayoutDimension::Percent(0.3))
                    .padding(LayoutEdges::all(LayoutLength::Percent(0.01)))
                    .overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll)
                    .scrollbar_width(2.0);
                let mut ui =
                    LogicalUi::new_layout(style.clone(), 0, BackendCapabilities::default())
                        .unwrap();
                let scope = ui.root_scope();
                let host = ui
                    .append_render_layout(scope, ui.root_render(), child.clone(), 0x0000ffff)
                    .unwrap();
                ui.append_render_layout(scope, ui.root_render(), child.clone(), 0x00ff00ff)
                    .unwrap();
                let constraints = LayoutConstraints::viewport(150.0, 120.0);
                let (_, packet) = ui
                    .edit_frame(
                        constraints,
                        [180.0; 2],
                        |_| Ok(()),
                        |(), edit| {
                            edit.allocation_builder(host, |input, edit| {
                                let inner = [144.0, 114.0];
                                let expected = if row(direction) {
                                    [86.4, 34.2]
                                } else {
                                    [43.2, 68.4]
                                };
                                for (actual, expected) in [
                                    input.known_size.width.unwrap(),
                                    input.known_size.height.unwrap(),
                                ]
                                .into_iter()
                                .zip(expected)
                                {
                                    assert!((actual - expected).abs() < 0.001);
                                }
                                assert_eq!(
                                    input.percentage_basis,
                                    LayoutSize::new(Some(inner[0]), Some(inner[1]))
                                );
                                edit.append_layout(scope, host, fixed(400.0, 300.0), 0xff0000ff)
                            })
                        },
                    )
                    .unwrap();
                let mut full = UiTree::new_layout(style, 0).unwrap();
                full.set_frame_build_policy(FrameBuildPolicy::Full);
                let expected_host = full
                    .append_layout(full.root(), child.clone(), 0x0000ffff)
                    .unwrap();
                full.append_layout(full.root(), child, 0x00ff00ff).unwrap();
                full.append_layout(expected_host, fixed(400.0, 300.0), 0xff0000ff)
                    .unwrap();
                let reference = full.frame_in_viewport(constraints, [180.0; 2]).unwrap();
                assert_eq!(
                    ui.layout_result(host).unwrap(),
                    full.layout_result(expected_host).unwrap()
                );
                assert_eq!(pixels(packet), pixels(reference));
            }
        }
    }
}

#[test]
fn full_flex_root_external_assignment_and_late_order_changes_keep_atomicity() {
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let mut ui = LogicalUi::new_layout(
        root(LayoutFlexDirection::Row),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let host = ui.root_render();
    ui.edit_frame(
        constraints,
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(host, |input, edit| {
                assert_eq!(
                    input.constraints.width,
                    LayoutAvailableSpace::Definite(120.0)
                );
                assert_eq!(
                    input.constraints.height,
                    LayoutAvailableSpace::Definite(90.0)
                );
                edit.append_layout(scope, host, item(1.0), 0)
            })
        },
    )
    .unwrap();
    let before = ui.published_frame().unwrap().revision;
    let root = ui.root_render();
    let host = ui.renders(scope).unwrap()[0];
    assert!(matches!(
        ui.edit_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |_, edit| {
                    edit.append_layout(scope, root, fixed(1.0, 1.0), 0)?;
                    Ok(())
                })
            }
        ),
        Err(Error::UnsafeConstraintBuilder)
    ));
    assert_eq!(ui.published_frame().unwrap().revision, before);
}

#[test]
fn full_flex_feedback_rejects_before_callbacks_and_late_equal_style_changes_roll_back() {
    for case in 0..7 {
        let mut style = root(LayoutFlexDirection::Row);
        if case == 0 {
            style.size.height = LayoutDimension::Auto;
        }
        if case == 3 || case == 6 {
            style.flex_wrap = LayoutFlexWrap::Wrap;
        }
        let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let mut child = item(1.0);
        match case {
            1 => child.min_size.width = LayoutDimension::Auto,
            2 => child.flex_basis = LayoutDimension::Content,
            4 => child.aspect_ratio = Some(2.0),
            5 => child.align_self = LayoutAlignSelf::Start.into(),
            6 => {
                child.size.height = LayoutDimension::Px(20.0);
                child.align_self = LayoutAlignSelf::Baseline.into();
            }
            _ => {}
        }
        let host = ui
            .append_render_layout(scope, ui.root_render(), child, 0)
            .unwrap();
        assert!(
            matches!(
                ui.edit_frame(
                    LayoutConstraints::viewport(120.0, 90.0),
                    [180.0; 2],
                    |_| Ok(()),
                    |(), edit| {
                        edit.allocation_builder(host, |_, _| -> Result<(), Error> {
                            panic!("unproven callback ran in case {case}")
                        })
                    }
                ),
                Err(Error::UnsafeConstraintBuilder)
            ),
            "case {case}"
        );
    }
    let mut ui = LogicalUi::new_layout(
        root(LayoutFlexDirection::Row),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), item(1.0), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, ui.root_render(), item(1.0), 0)
        .unwrap();
    let source = ui.state(scope, 0, || 0u32).unwrap();
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let before = ui.frame_in_viewport(constraints, [180.0; 2]).unwrap();
    let stats = ui.stats();
    assert!(matches!(
        ui.edit_frame(
            constraints,
            [180.0; 2],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.allocation_builder(host, |_, edit| {
                    edit.append_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)?;
                    // Numerically equal in this scene, but changes a sibling input.
                    edit.properties().layout_style(
                        sibling,
                        item(1.0).max_size(LayoutDimension::Px(500.0), LayoutDimension::Auto),
                    );
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
        pixels(ui.frame_in_viewport(constraints, [180.0; 2]).unwrap())
    );
}
