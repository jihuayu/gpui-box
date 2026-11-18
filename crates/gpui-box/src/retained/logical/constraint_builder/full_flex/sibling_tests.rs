use super::*;
use std::{cell::Cell, rc::Rc};

fn root(direction: LayoutFlexDirection) -> LayoutStyle {
    LayoutStyle::column()
        .flex_direction(direction)
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Percent(1.0))
}
fn body() -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden)
}
fn content() -> LayoutStyle {
    LayoutStyle::column()
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
        .flex_shrink(0.0)
}
fn fixed(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(width), LayoutDimension::Px(height))
        .flex_shrink(0.0)
}
fn metrics(width: f32, height: f32) -> Option<MeasureSnapshot> {
    Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
        width,
        height,
        baseline: Some(height * 0.5),
    }))
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
fn fixture(direction: LayoutFlexDirection) -> (LogicalUi, NodeId, NodeId, NodeId) {
    let mut ui = LogicalUi::new_layout(root(direction), 0, BackendCapabilities::default()).unwrap();
    let owner = ui.root_scope();
    let header = ui
        .append_render_layout(owner, ui.root_render(), content(), 0x00ff00ff)
        .unwrap();
    let leaf = ui
        .append_render_layout(owner, header, content(), 0x0000ffff)
        .unwrap();
    let body = ui
        .append_render_layout(owner, ui.root_render(), body(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().measure(leaf, metrics(20.0, 15.0));
            Ok(())
        },
    )
    .unwrap();
    (ui, header, leaf, body)
}

#[test]
fn sibling_content_full_flex_matches_numeric_reference_and_late_paint_is_allowed() {
    run(false);
}
#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires WGPU adapter; run explicitly"]
fn hardware_sibling_content_full_flex_matches_numeric_reference() {
    run(true);
}
fn run(hardware: bool) {
    #[cfg(feature = "retained-wgpu")]
    let mut gpu = hardware.then(|| wgpu::WgpuFrameRenderer::new(2).unwrap());
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &gpu {
        eprintln!("measured siblings adapter: {:?}", gpu.adapter_info());
    }
    #[cfg(not(feature = "retained-wgpu"))]
    assert!(!hardware);
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        let row = matches!(
            direction,
            LayoutFlexDirection::Row | LayoutFlexDirection::RowReverse
        );
        let (mut ui, _, leaf, host) = fixture(direction);
        let owner = ui.root_scope();
        let mut old = None;
        for (size, intrinsic) in [
            ([120.0, 90.0], [20.0, 15.0]),
            ([150.0, 120.0], [35.0, 30.0]),
            ([120.0, 90.0], [20.0, 15.0]),
        ] {
            let expected = if row {
                [size[0] - intrinsic[0], size[1]]
            } else {
                [size[0], size[1] - intrinsic[1]]
            };
            let (_, frame) = ui
                .edit_local_frame(
                    LayoutConstraints::viewport(size[0], size[1]),
                    [180.0; 2],
                    |_| Ok(()),
                    |(), edit| {
                        edit.properties()
                            .measure(leaf, metrics(intrinsic[0], intrinsic[1]));
                        edit.allocation_builder(host, |input, edit| {
                            assert_eq!(
                                input.known_size,
                                LayoutSize::new(Some(expected[0]), Some(expected[1]))
                            );
                            let branch =
                                edit.reconcile_keys(owner, &[u64::from(size[0] > 120.0)])?[0];
                            if edit.renders(branch)?.is_empty() {
                                edit.append_layout(branch, host, fixed(400.0, 300.0), 0xff0000ff)?;
                            }
                            // Fill is not an input to any sibling content measurement.
                            edit.properties().color(leaf, 0xffffffff);
                            Ok(())
                        })
                    },
                )
                .unwrap();
            let mut full = UiTree::new_layout(root(direction), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let header_size = if row {
                [intrinsic[0], size[1]]
            } else {
                [size[0], intrinsic[1]]
            };
            let header = full
                .append_layout(
                    full.root(),
                    fixed(header_size[0], header_size[1]),
                    0x00ff00ff,
                )
                .unwrap();
            full.append_layout(header, fixed(header_size[0], intrinsic[1]), 0xffffffff)
                .unwrap();
            let body = full
                .append_layout(
                    full.root(),
                    fixed(expected[0], expected[1])
                        .overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                    0,
                )
                .unwrap();
            full.append_layout(body, fixed(400.0, 300.0), 0xff0000ff)
                .unwrap();
            let reference = full
                .frame_in_viewport(LayoutConstraints::viewport(size[0], size[1]), [180.0; 2])
                .unwrap();
            assert_eq!(pixels(frame.resolve()), pixels(reference.clone()));
            #[cfg(feature = "retained-wgpu")]
            if let Some(gpu) = &mut gpu {
                gpu.submit_local(&frame, [180, 180], 1.0).unwrap();
                assert_eq!(
                    gpu.image().unwrap().as_raw(),
                    gpu.reference_image(&reference, [180, 180], 1.0)
                        .unwrap()
                        .as_raw()
                );
            }
            old.get_or_insert_with(|| (frame.clone(), pixels(frame.resolve())));
        }
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame.resolve()), expected);
    }
    #[cfg(feature = "retained-wgpu")]
    if let Some(gpu) = &mut gpu {
        gpu.finish().unwrap();
    }
}

#[test]
fn sibling_content_full_flex_late_equal_metrics_structure_and_algorithm_changes_reject() {
    for mutation in 0..5 {
        let (mut ui, header, leaf, host) = fixture(LayoutFlexDirection::Column);
        let owner = ui.root_scope();
        let source = ui.state(owner, 0, || 0u32).unwrap();
        let constraints = LayoutConstraints::viewport(120.0, 90.0);
        let before = ui.frame_in_viewport(constraints, [180.0; 2]).unwrap();
        let stats = ui.stats();
        assert!(
            matches!(
                ui.edit_local_frame(
                    constraints,
                    [180.0; 2],
                    |tx| {
                        tx.set(source, 1)?;
                        Ok(())
                    },
                    |(), edit| {
                        edit.allocation_builder(host, |_, edit| {
                            edit.append_layout(owner, host, fixed(20.0, 10.0), 0xff0000ff)?;
                            match mutation {
                                // Height remains15: equality of actual inputs is not the
                                // dependency contract. Baselines/resource kinds can matter.
                                0 => edit.properties().measure(leaf, metrics(21.0, 15.0)),
                                1 => {
                                    edit.append_layout(owner, header, fixed(0.0, 0.0), 0)?;
                                }
                                2 => edit
                                    .properties()
                                    .parent_data(leaf, Some(ParentData::new(1u32))),
                                3 => edit.properties().layout_style(
                                    leaf,
                                    content().max_size(
                                        LayoutDimension::Px(500.0),
                                        LayoutDimension::Auto,
                                    ),
                                ),
                                _ => edit.properties().layout_style(leaf, content().order(-1)),
                            }
                            Ok(())
                        })
                    }
                ),
                Err(Error::UnsafeConstraintBuilder)
            ),
            "mutation {mutation}"
        );
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(ui.stats(), stats);
        assert_eq!(
            pixels(before),
            pixels(ui.frame_in_viewport(constraints, [180.0; 2]).unwrap())
        );
    }
}

#[test]
fn sibling_content_full_flex_producers_observe_settled_measurements_and_explicit_order() {
    let (mut ui, _, leaf, host) = fixture(LayoutFlexDirection::Column);
    let owner = ui.root_scope();
    let height = ui.state(owner, 0, || 15u32).unwrap();
    let source = ui.state(owner, 1, || 20u32).unwrap();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    // A reusable producer for the measured leaf's host runs before the body.
    // It uses the proven inline builder, then the full builder reads the result.
    let header = ui.reactive().tree().node(leaf).unwrap().parent.unwrap();
    let header_producer = LayoutProducer::new(move |_, edit| {
        let h = *edit.read(height)? as f32;
        edit.properties().measure(leaf, metrics(20.0, h));
        Ok(())
    });
    let body_producer = LayoutProducer::new(move |input, edit| {
        let LayoutBuildInput::Full(input) = input else {
            panic!()
        };
        count.set(count.get() + 1);
        let expected = 90.0 - *edit.read(height)? as f32;
        assert_eq!(input.known_size.height, Some(expected));
        let value = *edit.read(source)?;
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
            let before = edit.layout_producer(
                owner,
                0,
                header,
                LayoutBuildKind::InlineAllocation,
                &[],
                header_producer,
            )?;
            edit.layout_producer(
                owner,
                1,
                host,
                LayoutBuildKind::Allocation,
                &[before],
                body_producer,
            )?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 75.0]);
    ui.transaction(|tx| tx.set(height, 30)).unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 60.0]);
    ui.transaction(|tx| tx.set(source, 40)).unwrap();
    assert_eq!(calls.get(), 3);
    ui.transaction(|tx| tx.set(source, 40)).unwrap();
    assert_eq!(calls.get(), 3);
}

#[test]
fn sibling_content_full_flex_wrapped_lines_and_content_measurers_keep_final_dependency_checks() {
    #[derive(Debug)]
    struct Natural([f32; 2]);
    impl ContentMeasure for Natural {
        fn measure(&self, _: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
            Ok(ContentMeasureOutput {
                size: self.0,
                baseline: Some(5.0),
                last_baseline: Some(5.0),
            })
        }
    }
    for wrap in [LayoutFlexWrap::Wrap, LayoutFlexWrap::WrapReverse] {
        let mut ui = LogicalUi::new_layout(
            root(LayoutFlexDirection::Row)
                .flex_wrap(wrap)
                .align_content(LayoutAlignContent::Start),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let owner = ui.root_scope();
        let header = ui
            .append_render_layout(owner, ui.root_render(), content(), 0x00ff00ff)
            .unwrap();
        let leaf = ui
            .append_render_layout(owner, header, content(), 0)
            .unwrap();
        let host = ui
            .append_render_layout(
                owner,
                ui.root_render(),
                body()
                    .flex_basis(LayoutDimension::Px(50.0))
                    .flex_grow(0.0)
                    .size(LayoutDimension::Auto, LayoutDimension::Px(20.0)),
                0x0000ffff,
            )
            .unwrap();
        let measure = ContentMeasureSnapshot::new(Natural([80.0, 30.0]));
        let constraints = LayoutConstraints::viewport(120.0, 90.0);
        let (_, frame) = ui
            .edit_frame(
                constraints,
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.properties()
                        .measure(leaf, Some(MeasureSnapshot::Content(measure.clone())));
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(input.known_size, LayoutSize::new(Some(50.0), Some(20.0)));
                        edit.append_layout(owner, host, fixed(400.0, 300.0), 0xff0000ff)
                    })
                },
            )
            .unwrap();
        let mut reference = UiTree::new_layout(
            root(LayoutFlexDirection::Row)
                .flex_wrap(wrap)
                .align_content(LayoutAlignContent::Start),
            0,
        )
        .unwrap();
        reference.set_frame_build_policy(FrameBuildPolicy::Full);
        reference
            .append_layout(reference.root(), fixed(80.0, 30.0), 0x00ff00ff)
            .unwrap();
        let reference_host = reference
            .append_layout(
                reference.root(),
                fixed(50.0, 20.0).overflow(LayoutOverflow::Hidden, LayoutOverflow::Hidden),
                0x0000ffff,
            )
            .unwrap();
        reference
            .append_layout(reference_host, fixed(400.0, 300.0), 0xff0000ff)
            .unwrap();
        assert_eq!(
            pixels(frame),
            pixels(
                reference
                    .frame_in_viewport(constraints, [180.0; 2])
                    .unwrap()
            )
        );
        // A new pure handle with identical answers is still a changed declared
        // dependency; it cannot be installed after construction consumed it.
        assert!(matches!(
            ui.edit_frame(
                constraints,
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |_, edit| {
                        edit.properties().measure(
                            leaf,
                            Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
                                Natural([80.0, 30.0]),
                            ))),
                        );
                        Ok(())
                    })
                }
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
        // Reusing the original immutable handle is an equal declaration.
        ui.edit_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |_, edit| {
                    edit.properties()
                        .measure(leaf, Some(MeasureSnapshot::Content(measure)));
                    Ok(())
                })
            },
        )
        .unwrap();
    }
}

#[test]
fn sibling_content_full_flex_late_layout_task_cannot_retroactively_change_header_measurement() {
    let (mut ui, header, leaf, host) = fixture(LayoutFlexDirection::Column);
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let before = ui.frame_in_viewport(constraints, [180.0; 2]).unwrap();
    let result = ui.edit_local_frame(
        constraints,
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            let mut plan = LayoutBuildPlan::new();
            plan.add(
                LayoutBuildTaskId(1),
                host,
                LayoutBuildKind::Allocation,
                &[],
                |input, _, _| {
                    let LayoutBuildInput::Full(input) = input else {
                        panic!()
                    };
                    assert_eq!(input.known_size.height, Some(75.0));
                    Ok(())
                },
            )?;
            plan.add(
                LayoutBuildTaskId(2),
                header,
                LayoutBuildKind::InlineAllocation,
                &[],
                |_, edit, _| {
                    edit.properties().measure(leaf, metrics(20.0, 30.0));
                    Ok(())
                },
            )?;
            edit.build_layout_plan(plan)
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(
        pixels(before),
        pixels(ui.frame_in_viewport(constraints, [180.0; 2]).unwrap())
    );
}

#[test]
fn sibling_content_full_flex_image_pixels_can_change_without_changing_intrinsic_inputs() {
    let (mut ui, _, leaf, host) = fixture(LayoutFlexDirection::Column);
    let image = |size: [u32; 2], color: [u8; 4]| {
        ImageResource::rgba8(size, color.repeat((size[0] * size[1]) as usize)).unwrap()
    };
    let constraints = LayoutConstraints::viewport(120.0, 90.0);
    let (_, frame) = ui
        .edit_local_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.properties()
                    .image(leaf, Some(image([20, 15], [255, 0, 0, 255])));
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_size.height, Some(0.0));
                    // The stretched image is120x90 from its8:6 natural ratio.
                    edit.properties()
                        .image(leaf, Some(image([20, 15], [0, 255, 0, 255])));
                    Ok(())
                })
            },
        )
        .unwrap();
    let before = pixels(frame.resolve());
    assert!(matches!(
        ui.edit_local_frame(
            constraints,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |_, edit| {
                    edit.properties()
                        .image(leaf, Some(image([20, 10], [0, 255, 0, 255])));
                    Ok(())
                })
            }
        ),
        Err(Error::UnsafeConstraintBuilder)
    ));
    assert_eq!(
        before,
        pixels(ui.published_local_frame().unwrap().resolve())
    );
}
