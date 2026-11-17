use super::*;

fn root_style() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn host_style() -> LayoutStyle {
    LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(5.0)))
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
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
fn reference(w: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(tree.root(), host_style(), 0x0000ffff)
        .unwrap();
    tree.append_layout(
        host,
        fixed(400.0, if w < 100.0 { 20.0 } else { 60.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}

#[test]
fn ordinary_flex_inline_allocation_builds_content_height_without_containment() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, host_style(), 0x0000ffff)
            .unwrap();
        let footer = ui
            .append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let mut old = None;
        for width in [80.0, 80.0, 140.0, 140.0, 80.0] {
            let build = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                edit.inline_allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_width, Some(width));
                    assert!(input.known_width_is_definite);
                    assert_eq!(input.percentage_basis, Some(width));
                    // Ordinary Flex passes the child's final allocation using
                    // Content mode after resolving its own preferred styles.
                    assert_eq!(input.sizing_mode, LayoutSizingMode::Content);
                    assert_eq!(input.min, 0.0);
                    assert_eq!(input.max, f32::INFINITY);
                    let branch = edit
                        .reconcile_keys(scope, &[u64::from(input.known_width.unwrap() >= 100.0)])?
                        [0];
                    let renders = edit.renders(branch)?;
                    if let Some(node) = renders.first() {
                        return Ok(*node);
                    }
                    edit.append_layout(
                        branch,
                        host,
                        fixed(
                            400.0,
                            if input.known_width.unwrap() < 100.0 {
                                20.0
                            } else {
                                60.0
                            },
                        ),
                        0xff0000ff,
                    )
                })
            };
            let (node, frame) = if local {
                let (commit, frame) = ui
                    .edit_local_frame(constraints(width), [180.0; 2], |_| Ok(()), build)
                    .unwrap();
                (commit.result.value, frame.resolve())
            } else {
                let (commit, frame) = ui
                    .edit_frame(constraints(width), [180.0; 2], |_| Ok(()), build)
                    .unwrap();
                (commit.result.value, frame)
            };
            let height = if width < 100.0 { 20.0 } else { 60.0 };
            assert_eq!(ui.layout_result(node).unwrap().size, [400.0, height]);
            assert_eq!(ui.layout_result(host).unwrap().size, [width, height + 10.0]);
            assert_eq!(
                ui.layout_result(footer).unwrap().location,
                [0.0, height + 10.0]
            );
            assert_eq!(pixels(frame.clone()), pixels(reference(width)));
            let tree = ui.reactive().tree();
            assert_eq!(
                tree.layout
                    .tracked_layout_input(tree.node(host).unwrap().layout)
                    .unwrap(),
                None,
                "successful transactions retire temporary observations"
            );
            assert_eq!(
                ui.frame_in_viewport(constraints(width), [180.0; 2])
                    .unwrap()
                    .layout_runs,
                0
            );
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn nested_ordinary_inline_builders_observe_current_draft_content_and_tight_ancestors() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let wrapper = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .padding(LayoutEdges::all(LayoutLength::Px(3.0))),
            0,
        )
        .unwrap();
    let host = ui
        .append_render_layout(scope, wrapper, host_style(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    for width in [80.0, 140.0, 80.0] {
        let (commit, _) = ui
            .edit_local_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        assert_eq!(input.known_width, Some(width - 6.0));
                        let branch = edit.reconcile_keys(scope, &[0])?[0];
                        let nodes = edit.renders(branch)?;
                        let inner = if let Some(node) = nodes.first() {
                            *node
                        } else {
                            edit.append_layout(branch, host, LayoutStyle::column(), 0)?
                        };
                        edit.inline_allocation_builder(inner, |input, edit| {
                            assert_eq!(input.known_width, Some(width - 16.0));
                            let key = u64::from(input.known_width.unwrap() > 100.0);
                            let child = edit.reconcile_keys(branch, &[key])?[0];
                            let nodes = edit.renders(child)?;
                            if let Some(node) = nodes.first() {
                                Ok(*node)
                            } else {
                                edit.append_layout(
                                    child,
                                    inner,
                                    fixed(400.0, if key == 0 { 20.0 } else { 60.0 }),
                                    0xff0000ff,
                                )
                            }
                        })
                    })
                },
            )
            .unwrap();
        let node = commit.result.value;
        let height = if width < 100.0 { 20.0 } else { 60.0 };
        assert_eq!(ui.layout_result(node).unwrap().size, [400.0, height]);
        assert_eq!(
            ui.layout_result(wrapper).unwrap().size,
            [width, height + 16.0]
        );
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, height + 16.0]
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct WidthPhase;
impl ParentInlineLayout for WidthPhase {
    fn allocate(
        &self,
        input: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        let width = input.known_width.unwrap_or(100.0) * 0.5;
        Ok(vec![
            ChildInlineAllocation {
                width,
                percentage_basis: Some(width)
            };
            count
        ])
    }
}
impl ParentLayout for WidthPhase {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut y = 0.0;
        let mut width: f32 = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            let slot = ChildLayoutInput::default();
            let result = children.measure_baselines(child, slot)?;
            width = width.max(result.size[0]);
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, y],
            });
            y += result.size[1];
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(width),
                input.known_size.height.unwrap_or(y),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}

#[test]
fn ordinary_host_under_independent_inline_phase_keeps_slot_and_snapshot_dependencies() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, host_style(), 0)
        .unwrap();
    let snapshot = ParentLayoutSnapshot::new(WidthPhase).with_inline_layout(WidthPhase);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(root, Some(snapshot));
            Ok(())
        },
    )
    .unwrap();
    let (commit, _) = ui
        .edit_frame(
            constraints(140.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_width, Some(70.0));
                    assert_eq!(input.min, 70.0);
                    assert_eq!(input.max, 70.0);
                    edit.append_layout(scope, host, fixed(400.0, 30.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    let node = commit.result.value;
    assert_eq!(ui.layout_result(host).unwrap().size, [70.0, 40.0]);
    assert_eq!(ui.layout_result(node).unwrap().size, [400.0, 30.0]);
    let old = ui.published_frame().unwrap().revision;
    let result = ui.edit_frame(
        constraints(140.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.inline_allocation_builder(host, |_, edit| {
                edit.properties().parent_layout(
                    root,
                    Some(ParentLayoutSnapshot::new(WidthPhase).with_inline_layout(WidthPhase)),
                );
                Ok(())
            })
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.published_frame().unwrap().revision, old);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_ordinary_inline_allocation_builder_matches_complete_flex_reference() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, host_style(), 0x0000ffff)
        .unwrap();
    ui.append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("inline-allocation adapter: {:?}", gpu.adapter_info());
    for width in [80.0, 140.0, 80.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        let key = u64::from(input.known_width.unwrap() > 100.0);
                        let branch = edit.reconcile_keys(scope, &[key])?[0];
                        if edit.renders(branch)?.is_empty() {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if key == 0 { 20.0 } else { 60.0 }),
                                0xff0000ff,
                            )?;
                        }
                        Ok(())
                    })
                },
            )
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(width), [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}

#[test]
fn ordinary_inline_builder_rejects_feedback_and_rolls_back_final_input_changes() {
    for invalid in 0..4 {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, host_style(), 0)
            .unwrap();
        let source = ui.state(scope, 123, || root_style()).unwrap();
        ui.bind(source, reactive::Property::layout_style(root))
            .unwrap();
        let old = ui
            .frame_in_viewport(constraints(100.0), [180.0; 2])
            .unwrap();
        let stats = ui.stats();
        let result = ui.edit_frame(
            constraints(100.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |_, edit| {
                    let bad = match invalid {
                        0 => root_style().size(LayoutDimension::Px(120.0), LayoutDimension::Auto),
                        1 => root_style().flex_direction(LayoutFlexDirection::Row),
                        2 => root_style().flex_wrap(LayoutFlexWrap::Wrap),
                        _ => {
                            let mut style = root_style();
                            style.aspect_ratio = Some(2.0);
                            style
                        }
                    };
                    edit.properties().layout_style(root, bad);
                    Ok(())
                })
            },
        );
        assert!(
            matches!(result, Err(Error::UnsafeConstraintBuilder)),
            "{invalid}"
        );
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        assert_eq!(ui.layout_result(host).unwrap().size, [100.0, 10.0]);
        assert!(
            !ui.reactive()
                .tree()
                .layout
                .tracked_layout_input(ui.reactive().tree().node(host).unwrap().layout)
                .unwrap()
                .is_some()
        );
    }
    let mut ratio = host_style();
    ratio.aspect_ratio = Some(2.0);
    for style in [
        LayoutStyle::column().align_self(LayoutAlignSelf::Start),
        ratio,
        host_style().size(LayoutDimension::MinContent, LayoutDimension::Auto),
    ] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui.append_render_layout(scope, root, style, 0).unwrap();
        assert!(matches!(
            ui.edit_frame(
                constraints(100.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| edit.inline_allocation_builder(host, |_, _| Ok(()))
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
    }
    // Locking the host's used width does not establish independence of its
    // incoming soft space and percentage basis from a Row/wrap/aspect parent.
    for mut parent in [
        root_style().flex_direction(LayoutFlexDirection::Row),
        root_style().flex_wrap(LayoutFlexWrap::Wrap),
        root_style(),
    ] {
        if parent == root_style() {
            parent.aspect_ratio = Some(2.0);
        }
        let mut ui = LogicalUi::new_layout(parent, 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(
                scope,
                root,
                host_style()
                    .size(LayoutDimension::Px(40.0), LayoutDimension::Auto)
                    .min_size(LayoutDimension::Px(40.0), LayoutDimension::Auto)
                    .max_size(LayoutDimension::Px(40.0), LayoutDimension::Auto),
                0,
            )
            .unwrap();
        assert!(matches!(
            ui.edit_frame(
                constraints(100.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| edit.inline_allocation_builder(host, |_, _| Ok(()))
            ),
            Err(Error::UnsafeConstraintBuilder)
        ));
    }
}

#[test]
fn ordinary_inline_builder_requires_an_independent_external_inline_anchor() {
    for available in [
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::MinContent,
        LayoutAvailableSpace::MaxContent,
    ] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, host_style(), 0)
            .unwrap();
        let mut called = false;
        let result = ui.edit_frame(
            LayoutConstraints::new(available, LayoutAvailableSpace::Unbounded),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |_, _| {
                    called = true;
                    Ok(())
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
        assert!(
            !called,
            "unresolved percentage root rejects before construction"
        );
    }
    for tight in [false, true] {
        let style = if tight {
            root_style()
        } else {
            root_style().size(LayoutDimension::Px(80.0), LayoutDimension::Auto)
        };
        let mut ui = LogicalUi::new_layout(style, 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, host_style(), 0)
            .unwrap();
        let mut input = LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        );
        if tight {
            input.bounds.min.width = 80.0;
            input.bounds.max.width = 80.0;
        }
        ui.edit_frame(
            input,
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_width, Some(80.0));
                    edit.append_layout(scope, host, fixed(400.0, 20.0), 0)?;
                    Ok(())
                })
            },
        )
        .unwrap();
        assert_eq!(ui.layout_result(host).unwrap().size, [80.0, 30.0]);
    }
}

#[test]
fn source_driven_ordinary_inline_building_retains_keyed_state_and_rolls_back_changed_dependency() {
    use std::{cell::RefCell, rc::Rc};
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, host_style(), 0)
        .unwrap();
    let source = ui
        .state(scope, 702, || {
            root_style().size(LayoutDimension::Px(80.0), LayoutDimension::Auto)
        })
        .unwrap();
    let (binding, _) = ui
        .bind(source, reactive::Property::layout_style(root))
        .unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let mut previous = None;
    for width in [80.0, 80.0, 140.0, 80.0] {
        let events = events.clone();
        let (commit, _) = ui
            .edit_frame(
                constraints(180.0),
                [180.0; 2],
                |tx| {
                    tx.set(
                        source,
                        root_style().size(LayoutDimension::Px(width), LayoutDimension::Auto),
                    )?;
                    Ok(())
                },
                |(), edit| {
                    edit.inline_allocation_builder(host, |input, edit| {
                        assert_eq!(input.known_width, Some(width));
                        let key = u64::from(width > 100.0);
                        let branch = edit.reconcile_keys(scope, &[key])?[0];
                        let state = edit.state(branch, 0, || 7u32)?;
                        assert_eq!(*edit.read(state)?, 7);
                        edit.effect(branch, 0, &[], move |_| {
                            events.borrow_mut().push(("run", key));
                            let events = events.clone();
                            Some(Box::new(move || events.borrow_mut().push(("cleanup", key))))
                        })?;
                        let nodes = edit.renders(branch)?;
                        let node = if let Some(node) = nodes.first() {
                            *node
                        } else {
                            edit.append_layout(
                                branch,
                                host,
                                fixed(400.0, if key == 0 { 20.0 } else { 60.0 }),
                                1,
                            )?
                        };
                        Ok((node, state))
                    })
                },
            )
            .unwrap();
        let (node, state) = commit.result.value;
        if let Some((w, old_node, old_state)) = previous {
            if width == w {
                assert_eq!(node, old_node);
                assert_eq!(state, old_state);
            }
        }
        previous = Some((width, node, state));
    }
    assert_eq!(
        *events.borrow(),
        [
            ("run", 0),
            ("cleanup", 0),
            ("run", 1),
            ("cleanup", 1),
            ("run", 0)
        ]
    );
    let before = ui.read(source).unwrap().clone();
    let stats = ui.stats();
    let revision = ui.published_frame().unwrap().revision;
    let effect_count = events.borrow().len();
    let result = ui.edit_frame(
        constraints(180.0),
        [180.0; 2],
        |tx| {
            tx.set(
                source,
                root_style().size(LayoutDimension::Px(140.0), LayoutDimension::Auto),
            )?;
            Ok(())
        },
        |(), edit| {
            edit.inline_allocation_builder(host, |_, edit| {
                let branch = edit.reconcile_keys(scope, &[99])?[0];
                edit.append_layout(branch, host, fixed(50.0, 90.0), 1)?;
                // A changed source's live binding otherwise writes 140px at
                // final settlement. Unbind before changing the dependency so
                // the final assignment actually differs from the observation.
                edit.unbind(binding)?;
                edit.properties().layout_style(
                    root,
                    root_style().size(LayoutDimension::Px(120.0), LayoutDimension::Auto),
                );
                Ok(())
            })
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.read(source).unwrap(), &before);
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.published_frame().unwrap().revision, revision);
    assert_eq!(events.borrow().len(), effect_count);
    ui.edit_frame(
        constraints(180.0),
        [180.0; 2],
        |tx| {
            tx.set(
                source,
                root_style().size(LayoutDimension::Px(100.0), LayoutDimension::Auto),
            )?;
            Ok(())
        },
        |(), _| Ok(()),
    )
    .unwrap();
    assert_eq!(
        ui.layout_result(host).unwrap().size[0],
        100.0,
        "failed edits preserve the previously live source binding"
    );
}
