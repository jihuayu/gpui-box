use super::*;

#[derive(Debug)]
struct LooseSlots;
impl ParentAllocationLayout for LooseSlots {
    fn allocate(
        &self,
        input: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        // Width derives only from incoming soft space. Measured host height is
        // unavailable. Width remains a loose interval; no used width is claimed.
        let width = match input.constraints.width {
            LayoutAvailableSpace::Definite(w) => w,
            _ => 80.0,
        };
        Ok((0..count)
            .map(|_| ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(width),
                    LayoutAvailableSpace::Unbounded,
                )
                .with_bounds(BoxConstraints::new([0.0; 2], [width, f32::INFINITY])),
                percentage_basis: LayoutSize::new(Some(width), None),
            })
            .collect())
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
        let mut height = 0.0;
        let mut width: f32 = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            assert!(children.allocation(child)?.is_some());
            // Deliberately conflict with the independent assignment. Both
            // measurement and final placement must use the allocated interval.
            let request = ChildLayoutInput::new(
                LayoutConstraints::viewport(999.0, 999.0)
                    .with_bounds(BoxConstraints::tight([999.0; 2])),
            );
            let result = children.measure(child, request)?;
            width = width.max(result.size[0]);
            placements.push(ParentPlacement {
                child,
                input: request,
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
    ParentLayoutSnapshot::new(Stack).with_allocation_layout(LooseSlots)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn column() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixture() -> (LogicalUi, NodeId, NodeId) {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui.append_render_layout(scope, root, column(), 0).unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 7.0), 0x00ff00ff)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(root, Some(snapshot()));
            Ok(())
        },
    )
    .unwrap();
    (ui, host, footer)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 100],
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

#[test]
fn loose_incoming_allocation_builds_content_height_and_matches_ordinary_full_flex() {
    for local in [false, true] {
        let (mut ui, host, footer) = fixture();
        let scope = ui.root_scope();
        let mut old = None;
        for w in [60.0, 140.0, 60.0, 140.0] {
            let mut calls = 0;
            let build = |(): (), edit: &mut LogicalEdit<'_, '_, '_>| {
                edit.allocation_builder(host, |input, edit| {
                    calls += 1;
                    assert_eq!(
                        input.constraints.bounds,
                        BoxConstraints::new([0.0; 2], [w, f32::INFINITY])
                    );
                    assert_eq!(input.constraints.height, LayoutAvailableSpace::Unbounded);
                    assert_eq!(input.percentage_basis, LayoutSize::new(Some(w), None));
                    let branch = edit.reconcile_keys(scope, &[u64::from(w >= 100.0)])?[0];
                    let outputs = edit.renders(branch)?;
                    if let Some(&node) = outputs.first() {
                        return Ok(node);
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
            };
            let frame = if local {
                ui.edit_local_frame(constraints(w), [180.0, 100.0], |_| Ok(()), build)
                    .unwrap()
                    .1
                    .resolve()
            } else {
                ui.edit_frame(constraints(w), [180.0, 100.0], |_| Ok(()), build)
                    .unwrap()
                    .1
            };
            assert_eq!(calls, 1);
            let h = if w < 100.0 { 30.0 } else { 10.0 };
            assert_eq!(ui.layout_result(host).unwrap().size, [w, h]);
            assert_eq!(ui.layout_result(footer).unwrap().location[1], h);
            assert_eq!(
                ui.layout_result(ui.root_render()).unwrap().size,
                [w, h + 7.0]
            );
            let mut full = UiTree::new_layout(column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let h = full.append_layout(full.root(), column(), 0).unwrap();
            let row = full
                .append_layout(
                    h,
                    LayoutStyle::row()
                        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                        .flex_wrap(LayoutFlexWrap::Wrap),
                    0,
                )
                .unwrap();
            for _ in 0..3 {
                full.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
                    .unwrap();
            }
            full.append_layout(full.root(), fixed(20.0, 7.0), 0x00ff00ff)
                .unwrap();
            assert_eq!(
                pixels(frame.clone()),
                pixels(
                    full.frame_in_viewport(constraints(w), [180.0, 100.0])
                        .unwrap()
                )
            );
            assert_eq!(
                frame.hit_test(1.0, ui.layout_result(footer).unwrap().location[1] + 1.0),
                Some(footer)
            );
            old.get_or_insert_with(|| (frame.clone(), pixels(frame)));
        }
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
}

#[test]
fn nested_full_assignment_builders_can_change_both_content_dimensions() {
    let (mut ui, host, _) = fixture();
    let scope = ui.root_scope();
    for width in [80.0, 130.0, 80.0] {
        let root = ui.root_render();
        let (commit, _) = ui
            .edit_frame(
                constraints(width),
                [180.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    // Install before observing; the ancestor's own list stays stable.
                    let current = edit.renders(scope)?;
                    let inner = if current.len() > 2 {
                        current[2]
                    } else {
                        edit.append_layout(scope, host, LayoutStyle::column(), 0)?
                    };
                    edit.properties().parent_layout(host, Some(snapshot()));
                    edit.properties().layout_style(host, LayoutStyle::column());
                    edit.allocation_builder(host, |_, edit| {
                        edit.allocation_builder(inner, |input, edit| {
                            assert_eq!(input.constraints.bounds.max.width, width);
                            let branch =
                                edit.reconcile_keys(scope, &[u64::from(width > 100.0)])?[0];
                            if edit.renders(branch)?.is_empty() {
                                edit.append_layout(
                                    branch,
                                    inner,
                                    fixed(
                                        if width > 100.0 { 70.0 } else { 30.0 },
                                        if width > 100.0 { 20.0 } else { 10.0 },
                                    ),
                                    0xff0000ff,
                                )?;
                            }
                            Ok(())
                        })
                    })
                },
            )
            .unwrap();
        let _ = commit;
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            if width > 100.0 {
                [70.0, 20.0]
            } else {
                [30.0, 10.0]
            }
        );
        assert_eq!(
            ui.layout_result(root).unwrap().size[1],
            if width > 100.0 { 27.0 } else { 17.0 }
        );
    }
}

#[test]
fn equal_numeric_algorithm_replacement_and_child_list_changes_reject_atomically() {
    for mutation in 0..3 {
        let (mut ui, host, footer) = fixture();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let value = ui.state(scope, 42, || 0u32).unwrap();
        let old = ui
            .frame_in_viewport(constraints(80.0), [180.0, 100.0])
            .unwrap();
        let stats = ui.stats();
        let result = ui.edit_frame(
            constraints(80.0),
            [180.0, 100.0],
            |tx| {
                tx.set(value, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.allocation_builder(host, |_, edit| {
                    edit.append_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)?;
                    match mutation {
                        0 => edit.properties().parent_layout(root, Some(snapshot())),
                        1 => {
                            edit.append_layout(scope, root, fixed(1.0, 1.0), 0)?;
                        }
                        _ => edit
                            .properties()
                            .layout_style(footer, fixed(20.0, 7.0).order(-1)),
                    }
                    Ok(())
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
        assert_eq!(*ui.read(value).unwrap(), 0);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        assert_eq!(
            pixels(
                ui.frame_in_viewport(constraints(80.0), [180.0, 100.0])
                    .unwrap()
            ),
            pixels(old)
        );
    }
}

#[test]
fn arbitrary_and_flex_feedback_and_intrinsic_style_resolution_reject_before_callback() {
    for case in 0..3 {
        let (mut ui, host, _) = fixture();
        let root = ui.root_render();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                match case {
                    0 => edit.properties().parent_layout(root, None),
                    1 => edit
                        .properties()
                        .parent_layout(root, Some(ParentLayoutSnapshot::new(Stack))),
                    _ => edit.properties().layout_style(
                        host,
                        column().size(LayoutDimension::MaxContent, LayoutDimension::Auto),
                    ),
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(matches!(
            ui.edit_frame(
                constraints(80.0),
                [180.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |_, _| -> Result<(), Error> {
                        panic!("unsafe builder callback ran")
                    })
                }
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
    }
}

#[derive(Debug)]
struct InvalidSlots(u8);
impl ParentAllocationLayout for InvalidSlots {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        let mut slot = ChildLayoutInput::new(constraints(80.0));
        match self.0 {
            0 => return Ok(vec![]),
            1 => slot.constraints.bounds.min.height = f32::NAN,
            2 => slot.constraints.bounds.max.width = -1.0,
            _ => slot.percentage_basis.height = Some(f32::NAN),
        }
        Ok(vec![slot; count])
    }
}
#[test]
fn invalid_allocations_roll_back_layout_and_sources() {
    for case in 0..4 {
        let (mut ui, _, _) = fixture();
        let root = ui.root_render();
        let old = ui
            .frame_in_viewport(constraints(80.0), [180.0, 100.0])
            .unwrap();
        let stats = ui.stats();
        let result = ui.edit_local_frame(
            constraints(80.0),
            [180.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(
                    root,
                    Some(
                        ParentLayoutSnapshot::new(Stack).with_allocation_layout(InvalidSlots(case)),
                    ),
                );
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(stats, ui.stats());
        assert_eq!(old.revision, ui.published_frame().unwrap().revision);
    }
}

#[derive(Debug)]
struct FixedInput(ChildLayoutInput);
impl ParentAllocationLayout for FixedInput {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![self.0; count])
    }
}

#[test]
fn incoming_builder_preserves_intrinsic_unbounded_bounds_and_independent_bases() {
    for space in [
        LayoutAvailableSpace::MinContent,
        LayoutAvailableSpace::MaxContent,
        LayoutAvailableSpace::Unbounded,
    ] {
        let (mut ui, host, _) = fixture();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let leaf = ui
            .append_render_layout(scope, host, fixed(999.0, 999.0), 0xff0000ff)
            .unwrap();
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::new(space, LayoutAvailableSpace::Unbounded)
                .with_bounds(BoxConstraints::new([25.0, 15.0], [70.0, 40.0])),
            percentage_basis: LayoutSize::new(Some(200.0), Some(300.0)),
        };
        ui.edit_frame(
            constraints(80.0),
            [180.0, 100.0],
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(
                    root,
                    Some(ParentLayoutSnapshot::new(Stack).with_allocation_layout(FixedInput(slot))),
                );
                edit.properties().layout_style(
                    host,
                    LayoutStyle::column().size(
                        LayoutDimension::Percent(0.25),
                        LayoutDimension::Percent(0.1),
                    ),
                );
                edit.allocation_builder(host, |input, _| {
                    assert_eq!(input.constraints, slot.constraints);
                    assert_eq!(input.percentage_basis, slot.percentage_basis);
                    Ok(())
                })
            },
        )
        .unwrap();
        assert_eq!(ui.layout_result(host).unwrap().size, [50.0, 30.0]);
        // A slot is an incoming interval, not the descendant's chosen size.
        assert_eq!(ui.layout_result(leaf).unwrap().size, [999.0; 2]);
    }
}

#[derive(Debug)]
struct OuterSoftSpace(f32);
impl ParentAllocationLayout for OuterSoftSpace {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![
            ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    LayoutAvailableSpace::Definite(self.0),
                    LayoutAvailableSpace::Unbounded
                )
                .with_bounds(BoxConstraints::new([80.0, 0.0], [80.0, f32::INFINITY])),
                percentage_basis: LayoutSize::new(Some(80.0), None),
            };
            count
        ])
    }
}
#[test]
fn equal_known_size_with_changed_soft_space_recomputes_nested_allocations() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let parent = tree.append_layout(tree.root(), column(), 0).unwrap();
    let leaf = tree
        .append_layout(parent, fixed(999.0, 10.0), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.parent_layout(parent, Some(snapshot()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for soft in [40.0, 70.0, 40.0] {
        let mut tx = Transaction::default();
        tx.parent_layout(
            tree.root(),
            Some(ParentLayoutSnapshot::new(Stack).with_allocation_layout(OuterSoftSpace(soft))),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let actual = tree
            .frame_in_viewport(constraints(80.0), [180.0, 100.0])
            .unwrap();
        assert_eq!(tree.layout_result(parent).unwrap().size[0], 80.0);
        assert_eq!(tree.layout_result(leaf).unwrap().size, [soft, 10.0]);
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        let host = full
            .append_layout(full.root(), fixed(80.0, 10.0), 0)
            .unwrap();
        full.append_layout(host, fixed(soft, 10.0), 0xff0000ff)
            .unwrap();
        assert_eq!(
            pixels(actual),
            pixels(
                full.frame_in_viewport(constraints(80.0), [180.0, 100.0])
                    .unwrap()
            )
        );
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_full_allocations_and_content_builders_match_fresh_flex() {
    let (mut ui, host, footer) = fixture();
    let scope = ui.root_scope();
    let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("full allocation adapter: {:?}", renderer.adapter_info());
    for width in [60.0, 140.0, 60.0, 140.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [180.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    edit.allocation_builder(host, |input, edit| {
                        assert_eq!(input.constraints.bounds.max.width, width);
                        let branch = edit.reconcile_keys(scope, &[u64::from(width >= 100.0)])?[0];
                        if edit.renders(branch)?.is_empty() {
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
                        }
                        Ok(())
                    })
                },
            )
            .unwrap();
        renderer
            .submit_local(&frame, [180, 100], 1.0)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full.append_layout(full.root(), column(), 0).unwrap();
        let row = full
            .append_layout(
                host,
                LayoutStyle::row()
                    .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                    .flex_wrap(LayoutFlexWrap::Wrap),
                0,
            )
            .unwrap();
        for _ in 0..3 {
            full.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
                .unwrap();
        }
        full.append_layout(full.root(), fixed(20.0, 7.0), 0x00ff00ff)
            .unwrap();
        let reference = full
            .frame_in_viewport(constraints(width), [180.0, 100.0])
            .unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&reference, [180, 100], 1.0)
                .unwrap()
                .as_raw()
        );
        let h = ui.layout_result(footer).unwrap().location[1] as usize;
        assert_eq!(
            &actual.as_raw()[(h * 180 + 1) * 4..(h * 180 + 1) * 4 + 4],
            &[0, 255, 0, 255]
        );
    }
    renderer.finish().unwrap();
}
