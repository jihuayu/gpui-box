use super::*;

#[derive(Debug)]
struct Loose;
impl ParentAllocationLayout for Loose {
    fn allocate(
        &self,
        input: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let width = input.known_size.width.unwrap_or(0.0);
        Ok(vec![
            ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(width),
                    LayoutAvailableSpace::Unbounded
                )
                .with_bounds(BoxConstraints::loose([width, f32::INFINITY])),
                percentage_basis: LayoutSize::new(Some(width), None),
                ..ChildLayoutInput::default()
            };
            count
        ])
    }
}
#[derive(Debug)]
struct Stack;
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut width: f32 = 0.0;
        let mut height = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            let slot = children
                .allocation(child)?
                .ok_or(Error::InvalidParentLayout)?;
            let result = children.measure(child, slot)?;
            width = width.max(result.size[0]);
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, height],
            });
            height += result.size[1];
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(width),
                input.known_size.height.unwrap_or(height),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}
fn snapshot() -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Stack).with_allocation_layout(Loose)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn root() -> LayoutStyle {
    LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Px(80.0))
}
fn boundary() -> LayoutStyle {
    LayoutStyle::column()
        .container_type(LayoutContainerType::Size)
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(1.0)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0))
}
fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::viewport(width, 100.0)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [200, 120],
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
fn reference(width: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(root(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    tree.append_layout(tree.root(), fixed(20.0, 80.0), 0x0000ffff)
        .unwrap();
    let cut = tree.append_layout(tree.root(), boundary(), 0).unwrap();
    // The independent parent gives a loose maximum. Ordinary Flex below is
    // positioned with the actual min(available, intrinsic120) content width.
    let host = tree
        .append_layout(
            cut,
            LayoutStyle::column().fixed_inline_size((width - 20.0).min(120.0)),
            0,
        )
        .unwrap();
    let row = tree
        .append_layout(
            host,
            LayoutStyle::row()
                .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                .flex_wrap(LayoutFlexWrap::Wrap),
            0,
        )
        .unwrap();
    for _ in 0..3 {
        tree.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
            .unwrap();
    }
    tree.frame_in_viewport(constraints(width), [200.0, 120.0])
        .unwrap()
}

#[test]
fn ordinary_flex_cut_composes_with_independent_parent_and_content_sized_keyed_builder() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        ui.append_render_layout(scope, ui.root_render(), fixed(20.0, 80.0), 0x0000ffff)
            .unwrap();
        let cut = ui
            .append_render_layout(scope, ui.root_render(), boundary(), 0)
            .unwrap();
        let host = ui
            .append_render_layout(scope, cut, LayoutStyle::column(), 0)
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(cut, Some(snapshot()));
                Ok(())
            },
        )
        .unwrap();
        let mut old = None;
        for width in [80.0, 160.0, 80.0, 160.0] {
            let mut called = 0;
            let build = |(): (), edit: &mut LogicalEdit<'_, '_, '_>| {
                edit.allocation_builder(cut, |input, edit| {
                    called += 1;
                    assert_eq!(
                        input.known_size,
                        LayoutSize::new(Some(width - 20.0), Some(80.0))
                    );
                    assert_eq!(input.known_size_is_definite, LayoutSize::new(true, true));
                    assert_eq!(input.constraints.bounds, BoxConstraints::UNBOUNDED);
                    edit.allocation_builder(host, |input, edit| {
                        called += 1;
                        assert_eq!(input.known_size, LayoutSize::new(None, None));
                        assert_eq!(
                            input.constraints.bounds,
                            BoxConstraints::loose([width - 20.0, f32::INFINITY])
                        );
                        assert_eq!(
                            input.percentage_basis,
                            LayoutSize::new(Some(width - 20.0), None)
                        );
                        let branch = edit.reconcile_keys(scope, &[u64::from(width > 100.0)])?[0];
                        let outputs = edit.renders(branch)?;
                        if let Some(&row) = outputs.first() {
                            return Ok(row);
                        }
                        let row = edit.append_layout(
                            branch,
                            host,
                            LayoutStyle::row()
                                .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                                .flex_wrap(LayoutFlexWrap::Wrap),
                            0,
                        )?;
                        for _ in 0..3 {
                            edit.append_layout(branch, row, fixed(40.0, 10.0), 0xff0000ff)?;
                        }
                        Ok(row)
                    })
                })
            };
            let (commit, frame) = if local {
                let (c, f) = ui
                    .edit_local_frame(constraints(width), [200.0, 120.0], |_| Ok(()), build)
                    .unwrap();
                (c, f.resolve())
            } else {
                ui.edit_frame(constraints(width), [200.0, 120.0], |_| Ok(()), build)
                    .unwrap()
            };
            assert_eq!(called, 2);
            assert_eq!(ui.layout_result(cut).unwrap().size, [width - 20.0, 80.0]);
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [
                    (width - 20.0).min(120.0),
                    if width > 100.0 { 10.0 } else { 30.0 }
                ]
            );
            assert_eq!(
                frame.hit_test(21.0, 1.0),
                Some(
                    ui.children(scope)
                        .unwrap()
                        .iter()
                        .flat_map(|s| ui.renders(*s).unwrap().iter())
                        .copied()
                        .find(|n| *n != commit.result.value)
                        .unwrap()
                )
            );
            let actual = pixels(frame.clone());
            assert_eq!(actual, pixels(reference(width)));
            old.get_or_insert((frame, actual));
        }
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
}

#[test]
fn full_contained_constraint_box_assigns_loose_intervals_to_content_sized_host() {
    let style = LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
        .size(LayoutDimension::Px(100.0), LayoutDimension::Px(60.0))
        .container_type(LayoutContainerType::Size)
        .padding(LayoutEdges::all(LayoutLength::Px(5.0)));
    let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, ui.root_render(), LayoutStyle::column(), 0)
        .unwrap();
    ui.edit_frame(
        constraints(160.0),
        [200.0, 120.0],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(host, |input, edit| {
                assert_eq!(input.known_size, LayoutSize::new(None, None));
                assert_eq!(
                    input.constraints.bounds,
                    BoxConstraints::loose([90.0, 50.0])
                );
                assert_eq!(
                    input.percentage_basis,
                    LayoutSize::new(Some(90.0), Some(50.0))
                );
                edit.append_layout(scope, host, fixed(40.0, 20.0), 0xff0000ff)
            })
        },
    )
    .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [40.0, 20.0]);
}

#[test]
fn inline_containment_arbitrary_parent_and_late_proof_changes_reject_atomically() {
    for mutation in 0..5 {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let cut = ui.append_render_layout(scope, root, boundary(), 0).unwrap();
        let host = ui
            .append_render_layout(scope, cut, LayoutStyle::column(), 0)
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(cut, Some(snapshot()));
                Ok(())
            },
        )
        .unwrap();
        let value = ui.state(scope, 1, || 0u32).unwrap();
        let old = ui
            .frame_in_viewport(constraints(160.0), [200.0, 120.0])
            .unwrap();
        let before = ui.stats();
        let result = ui.edit_frame(
            constraints(160.0),
            [200.0, 120.0],
            |tx| {
                tx.set(value, 1)?;
                Ok(())
            },
            |(), edit| {
                if mutation == 0 {
                    edit.properties().layout_style(
                        cut,
                        boundary().container_type(LayoutContainerType::InlineSize),
                    );
                }
                if mutation == 1 {
                    edit.properties()
                        .parent_layout(cut, Some(ParentLayoutSnapshot::new(Stack)));
                }
                edit.allocation_builder(host, |_, edit| {
                    assert!(mutation >= 2, "unsupported chain reached builder");
                    edit.append_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)?;
                    match mutation {
                        2 => edit.properties().layout_style(
                            cut,
                            boundary().container_type(LayoutContainerType::Normal),
                        ),
                        3 => edit.properties().parent_layout(cut, Some(snapshot())),
                        _ => {
                            edit.append_layout(scope, root, fixed(0.0, 0.0), 0)?;
                        }
                    }
                    Ok(())
                })
            },
        );
        assert!(result.is_err(), "mutation {mutation}");
        assert_eq!(ui.read(value).unwrap(), &0);
        assert_eq!(ui.stats(), before);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

#[test]
fn contained_external_root_supports_unknown_sizes_without_turning_soft_space_into_bounds() {
    let mut ui = LogicalUi::new_layout(boundary(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    ui.edit_local_frame(
        LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        ),
        [200.0, 120.0],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(root, |input, edit| {
                assert_eq!(input.known_size, LayoutSize::new(None, None));
                assert_eq!(input.constraints.bounds, BoxConstraints::UNBOUNDED);
                assert_eq!(input.constraints.width, LayoutAvailableSpace::Unbounded);
                edit.append_layout(scope, root, fixed(40.0, 20.0), 0xff0000ff)
            })
        },
    )
    .unwrap();
    assert_eq!(ui.layout_result(root).unwrap().size, [0.0, 0.0]);
}

#[test]
fn ordinary_flex_contained_builders_preserve_reverse_wrap_direction_aspect_and_box_inputs() {
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        for rtl in [false, true] {
            for wrap in [
                LayoutFlexWrap::NoWrap,
                LayoutFlexWrap::Wrap,
                LayoutFlexWrap::WrapReverse,
            ] {
                for decorated in [false, true] {
                    let mut outer = fixed(120.0, 90.0)
                        .flex_direction(direction)
                        .flex_wrap(wrap)
                        .direction(if rtl {
                            LayoutDirection::Rtl
                        } else {
                            LayoutDirection::Ltr
                        })
                        .align_items(LayoutAlignItems::LastBaseline);
                    outer.aspect_ratio = Some(1.5);
                    let mut cut_style = boundary().flex_basis(LayoutDimension::Px(70.0));
                    cut_style.aspect_ratio = Some(2.0);
                    if decorated {
                        cut_style = cut_style
                            .padding(LayoutEdges::all(LayoutLength::Percent(0.05)))
                            .margin(LayoutEdges::all(LayoutLength::Px(2.0)));
                    }
                    let mut ui =
                        LogicalUi::new_layout(outer.clone(), 0, BackendCapabilities::default())
                            .unwrap();
                    let scope = ui.root_scope();
                    let cut = ui
                        .append_render_layout(
                            scope,
                            ui.root_render(),
                            cut_style.clone(),
                            0x111111ff,
                        )
                        .unwrap();
                    ui.append_render_layout(scope, ui.root_render(), fixed(60.0, 35.0), 0x00ff00ff)
                        .unwrap();
                    let mut observed = None;
                    let (commit, frame) = ui
                        .edit_local_frame(
                            constraints(120.0),
                            [200.0, 120.0],
                            |_| Ok(()),
                            |(), edit| {
                                edit.allocation_builder(cut, |input, edit| {
                                    observed = Some(input);
                                    let child = edit.append_layout(
                                        scope,
                                        cut,
                                        fixed(130.0, 100.0),
                                        0xff0000ff,
                                    )?;
                                    Ok(child)
                                })
                            },
                        )
                        .unwrap();
                    let mut full = UiTree::new_layout(outer, 0).unwrap();
                    full.set_frame_build_policy(FrameBuildPolicy::Full);
                    let fc = full
                        .append_layout(full.root(), cut_style, 0x111111ff)
                        .unwrap();
                    full.append_layout(full.root(), fixed(60.0, 35.0), 0x00ff00ff)
                        .unwrap();
                    let leaf = full
                        .append_layout(fc, fixed(130.0, 100.0), 0xff0000ff)
                        .unwrap();
                    let reference = full
                        .frame_in_viewport(constraints(120.0), [200.0, 120.0])
                        .unwrap();
                    assert_eq!(
                        ui.layout_result(cut).unwrap(),
                        full.layout_result(fc).unwrap()
                    );
                    assert_eq!(
                        ui.layout_result(commit.result.value).unwrap(),
                        full.layout_result(leaf).unwrap()
                    );
                    assert!(observed.unwrap().known_size.width.is_some());
                    assert_eq!(pixels(frame.resolve()), pixels(reference));
                }
            }
        }
    }
}

#[test]
fn content_derived_flex_known_block_size_keeps_indefinite_percentage_semantics() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Px(100.0), LayoutDimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let cut = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            boundary().flex_basis(LayoutDimension::Auto),
            0,
        )
        .unwrap();
    ui.edit_frame(
        constraints(100.0),
        [200.0, 120.0],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(cut, |input, edit| {
                assert_eq!(input.known_size.height, Some(0.0));
                assert!(!input.known_size_is_definite.height);
                // This is the parent's final containing-block basis, distinct
                // from whether this item's known block size is definite.
                assert_eq!(input.percentage_basis.height, Some(0.0));
                edit.append_layout(scope, cut, fixed(40.0, 20.0), 0xff0000ff)
            })
        },
    )
    .unwrap();
    assert_eq!(ui.layout_result(cut).unwrap().size[1], 0.0);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_mixed_flex_allocation_builders_match_independent_full_pixels() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    ui.append_render_layout(scope, ui.root_render(), fixed(20.0, 80.0), 0x0000ffff)
        .unwrap();
    let cut = ui
        .append_render_layout(scope, ui.root_render(), boundary(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(scope, cut, LayoutStyle::column(), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(cut, Some(snapshot()));
            Ok(())
        },
    )
    .unwrap();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("mixed allocation adapter: {:?}", renderer.adapter_info());
    for width in [80.0, 160.0, 80.0, 160.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [200.0, 120.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(input.constraints.bounds.max.width, width - 20.0);
                        let branch = edit.reconcile_keys(scope, &[u64::from(width > 100.0)])?[0];
                        let row = edit.append_layout(
                            branch,
                            host,
                            LayoutStyle::row()
                                .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                                .flex_wrap(LayoutFlexWrap::Wrap),
                            0,
                        )?;
                        for _ in 0..3 {
                            edit.append_layout(branch, row, fixed(40.0, 10.0), 0xff0000ff)?;
                        }
                        Ok(())
                    })
                },
            )
            .unwrap();
        renderer
            .submit_local(&frame, [200, 120], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            renderer.image().unwrap().as_raw(),
            renderer
                .reference_image(&reference(width), [200, 120], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();
}
