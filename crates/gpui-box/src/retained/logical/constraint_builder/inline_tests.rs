use super::*;
use std::{cell::RefCell, rc::Rc};

fn boundary() -> LayoutStyle {
    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize)
}
fn column() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        layout::AvailableSpace::Definite(w),
        layout::AvailableSpace::Unbounded,
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
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}

#[test]
fn inline_branch_content_sets_auto_height_and_following_flex_sibling_geometry() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host = ui
        .append_render_layout(
            scope,
            root,
            boundary().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let mut old = None;
    for w in [80.0, 80.0, 140.0, 140.0, 80.0] {
        let events = events.clone();
        let (commit, frame) = ui
            .edit_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.min, w - 10.0);
                        assert_eq!(input.max, w - 10.0);
                        assert_eq!(input.percentage_basis, Some(w - 10.0));
                        let key = u64::from(input.max >= 100.0);
                        let branch = edit.reconcile_keys(scope, &[key])?[0];
                        edit.effect(branch, 0, &[], move |_| {
                            events.borrow_mut().push(("run", key));
                            let events = events.clone();
                            Some(Box::new(move || events.borrow_mut().push(("cleanup", key))))
                        })?;
                        let handles = edit.renders(branch)?;
                        if let Some(leaf) = handles.first() {
                            return Ok(*leaf);
                        }
                        edit.append_layout(
                            branch,
                            host,
                            fixed(400.0, if key == 0 { 20.0 } else { 60.0 }),
                            0xff0000ff,
                        )
                    })
                },
            )
            .unwrap();
        let leaf = commit.result.value;
        let height = if w < 100.0 { 20.0 } else { 60.0 };
        assert_eq!(ui.layout_result(leaf).unwrap().size, [w - 10.0, height]);
        assert_eq!(ui.layout_result(host).unwrap().size, [w, height + 10.0]);
        assert_eq!(
            ui.layout_result(footer).unwrap().location,
            [0.0, height + 10.0]
        );
        assert_eq!(ui.layout_result(root).unwrap().size, [w, height + 20.0]);
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.frame_build_policy = FrameBuildPolicy::Full;
        let full_host = full
            .append_layout(
                full.root(),
                boundary().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                0,
            )
            .unwrap();
        full.append_layout(full_host, fixed(400.0, height), 0xff0000ff)
            .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        assert_eq!(
            pixels(frame.clone()),
            pixels(full.frame_in_viewport(constraints(w), [180.0; 2]).unwrap())
        );
        assert_eq!(frame.hit_test(10.0, 10.0), Some(leaf));
        assert_eq!(frame.hit_test(10.0, height + 15.0), Some(footer));
        if old.is_none() {
            old = Some((frame, leaf));
        }
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
    let (old, old_leaf) = old.unwrap();
    assert_eq!(
        old.items
            .iter()
            .find(|item| item.node == old_leaf)
            .unwrap()
            .bounds
            .height,
        20.0
    );
    assert_eq!(old.hit_test(10.0, 10.0), Some(old_leaf));
}

#[test]
fn nested_inline_builders_and_tight_box_ancestors_publish_content_height_upward() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let outer = ui.append_render_layout(scope, root, boundary(), 0).unwrap();
    let (commit, _) = ui
        .edit_frame(
            constraints(120.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_constraint_builder(outer, |input, edit| {
                    assert_eq!(input.max, 120.0);
                    let inner = edit.append_layout(
                        scope,
                        outer,
                        boundary().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                        0,
                    )?;
                    edit.inline_constraint_builder(inner, |input, edit| {
                        assert_eq!(input.max, 110.0);
                        let row = edit.append_layout(
                            scope,
                            inner,
                            LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap),
                            0,
                        )?;
                        for _ in 0..3 {
                            edit.append_layout(scope, row, fixed(50.0, 20.0), 0xff0000ff)?;
                        }
                        Ok((inner, row))
                    })
                })
            },
        )
        .unwrap();
    let (inner, row) = commit.result.value;
    assert_eq!(ui.layout_result(row).unwrap().size, [110.0, 40.0]);
    assert_eq!(ui.layout_result(inner).unwrap().size, [120.0, 50.0]);
    assert_eq!(ui.layout_result(outer).unwrap().size, [120.0, 50.0]);
    assert_eq!(ui.layout_result(root).unwrap().size, [120.0, 50.0]);
}

#[test]
fn full_axis_builder_remains_rejected_on_inline_host_and_unsafe_chains_do_not_build() {
    for parent in [
        LayoutStyle::row(),
        column().flex_wrap(LayoutFlexWrap::Wrap),
        {
            let mut s = column();
            s.aspect_ratio = Some(2.0);
            s
        },
        column().min_size(LayoutDimension::MinContent, LayoutDimension::Auto),
    ] {
        let mut ui = LogicalUi::new_layout(parent, 0, BackendCapabilities::default()).unwrap();
        let host = ui
            .append_render_layout(ui.root_scope(), ui.root_render(), boundary(), 0)
            .unwrap();
        let result = ui.edit_frame(
            constraints(120.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_constraint_builder(host, |_, _| -> Result<(), Error> {
                    panic!("unsafe chain invoked builder")
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    }
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let host = ui
        .append_render_layout(ui.root_scope(), ui.root_render(), boundary(), 0)
        .unwrap();
    assert!(matches!(
        ui.edit_frame(
            constraints(120.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| edit.constraint_builder(host, |_, _| Ok(()))
        ),
        Err(Error::UnsafeConstraintBuilder)
    ));
}

#[test]
fn final_ancestor_proof_changes_reject_even_if_preview_and_final_widths_match() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui.append_render_layout(scope, root, boundary(), 0).unwrap();
    let before = ui
        .frame_in_viewport(constraints(120.0), [180.0; 2])
        .unwrap();
    let result = ui.edit_frame(
        constraints(120.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.inline_constraint_builder(host, |input, edit| {
                assert_eq!(input.max, 120.0);
                edit.append_layout(scope, host, fixed(20.0, 30.0), 0xff0000ff)?;
                Ok(())
            })?;
            edit.properties()
                .layout_style(root, column().flex_wrap(LayoutFlexWrap::Wrap));
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 0.0]);
}

#[test]
fn inline_input_change_rolls_back_branch_state_and_effect_after_source_settlement() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui.append_render_layout(scope, root, boundary(), 0).unwrap();
    let value = ui.state(scope, 7, || 0u32).unwrap();
    let before = ui
        .frame_in_viewport(constraints(120.0), [180.0; 2])
        .unwrap();
    let events = Rc::new(RefCell::new(0));
    let log = events.clone();
    let result = ui.edit_frame(
        constraints(120.0),
        [180.0; 2],
        |tx| {
            tx.set(value, 1)?;
            Ok(())
        },
        |(), edit| {
            edit.inline_constraint_builder(host, |_, edit| {
                let branch = edit.reconcile_keys(scope, &[1])?[0];
                edit.effect(branch, 0, &[], move |_| {
                    *log.borrow_mut() += 1;
                    None
                })?;
                edit.append_layout(branch, host, fixed(20.0, 30.0), 0xff0000ff)?;
                Ok(())
            })?;
            edit.properties().layout_style(
                root,
                LayoutStyle::column().size(LayoutDimension::Px(90.0), LayoutDimension::Auto),
            );
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(*ui.read(value).unwrap(), 0);
    assert!(ui.children(scope).unwrap().is_empty());
    assert_eq!(*events.borrow(), 0);
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
}

#[test]
fn loose_slot_observation_is_a_maximum_and_child_height_does_not_become_dependency() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
                .container_type(LayoutContainerType::InlineSize),
            0,
        )
        .unwrap();
    let (commit, _) = ui
        .edit_frame(
            constraints(120.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_constraint_builder(host, |input, edit| {
                    assert_eq!((input.min, input.max), (0.0, 120.0));
                    edit.append_layout(scope, host, fixed(20.0, 50.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    assert_eq!(
        ui.layout_result(commit.result.value).unwrap().size,
        [20.0, 50.0]
    );
    assert_eq!(ui.layout_result(host).unwrap().size, [120.0, 50.0]);
}

#[test]
fn fixed_inline_boundary_in_wrapping_row_keeps_content_height_and_does_not_require_column_ancestors()
 {
    let mut parent = LayoutStyle::row()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .flex_wrap(LayoutFlexWrap::Wrap)
        .align_items(LayoutAlignItems::Start);
    parent.aspect_ratio = Some(2.0);
    let mut ui = LogicalUi::new_layout(parent, 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            boundary()
                .fixed_inline_size(80.0)
                .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    for width in [100.0, 140.0, 100.0] {
        let (commit, _) = ui
            .edit_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, 70.0);
                        let handles = edit.renders(scope)?;
                        let leaf = if handles.len() > 1 {
                            handles[1]
                        } else {
                            edit.append_layout(scope, host, fixed(30.0, 45.0), 0xff0000ff)?
                        };
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        // The parent's ratio bounds the block space at narrow widths. This is
        // allowed to change child height, never the observed inline allocation.
        let height = 55.0f32.min(width / 2.0);
        assert_eq!(ui.layout_result(host).unwrap().size, [80.0, height]);
        assert_eq!(
            ui.layout_result(commit.result.value).unwrap().size,
            [70.0, height - 10.0]
        );
    }
}

#[test]
fn fixed_width_proof_rejects_percentage_insets_and_hard_constraint_parent_feedback() {
    // Border-box width alone cannot freeze content width if padding depends on
    // an unproven parent width. A hard parent edge may override authored min=max.
    for hard_parent in [false, true] {
        let root_style = if hard_parent {
            LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .size(LayoutDimension::Auto, LayoutDimension::Px(80.0))
        } else {
            LayoutStyle::row()
        };
        let mut ui = LogicalUi::new_layout(root_style, 0, BackendCapabilities::default()).unwrap();
        let host_style = if hard_parent {
            boundary().fixed_inline_size(80.0)
        } else {
            boundary()
                .fixed_inline_size(80.0)
                .padding(LayoutEdges::all(LayoutLength::Percent(0.1)))
        };
        let host = ui
            .append_render_layout(ui.root_scope(), ui.root_render(), host_style, 0)
            .unwrap();
        let result = ui.edit_frame(
            constraints(120.0),
            [180.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.inline_constraint_builder(host, |_, _| -> Result<(), Error> {
                    panic!("feedback chain built")
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    }
}

#[test]
fn responsive_container_query_uses_shared_tight_box_proof_and_rejects_loose_edge() {
    use crate::retained::responsive::*;
    for fit in [ConstraintFit::Tight, ConstraintFit::Loose] {
        let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(
                scope,
                ui.root_render(),
                LayoutStyle::constraint_box(fit, ConstraintFit::Loose)
                    .container_type(LayoutContainerType::InlineSize),
                0,
            )
            .unwrap();
        let query_host = ui
            .append_render_layout(
                scope,
                host,
                LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
                0,
            )
            .unwrap();
        let leaf = ui
            .append_render_layout(scope, query_host, fixed(20.0, 10.0), 0xff0000ff)
            .unwrap();
        let style = ui.state(scope, 0, || fixed(20.0, 10.0)).unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        let registered = rules.bind_container(
            &mut ui,
            style,
            leaf,
            query_host,
            LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![(100.0, fixed(20.0, 30.0))])
                .unwrap(),
        );
        if fit == ConstraintFit::Loose {
            assert_eq!(registered.err(), Some(Error::UnsafeContainerQuery));
            continue;
        }
        registered.unwrap();
        for width in [80.0, 140.0, 80.0] {
            rules
                .frame_in_viewport(
                    &mut ui,
                    &LayoutEnvironment::new([width, 180.0]),
                    constraints(width),
                    [180.0; 2],
                )
                .unwrap();
            assert_eq!(
                ui.layout_result(query_host).unwrap().size,
                [width, if width < 100.0 { 10.0 } else { 30.0 }]
            );
        }
        let before = ui.published_frame().unwrap().revision;
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().layout_style(
                    host,
                    LayoutStyle::constraint_box(ConstraintFit::Loose, ConstraintFit::Loose)
                        .container_type(LayoutContainerType::InlineSize),
                );
                Ok(())
            },
        )
        .unwrap();
        // A direct layout edit is independently committed; query publication
        // must still reject its now-invalid host proof before applying styles.
        let result = rules.frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([140.0, 180.0]),
            constraints(140.0),
            [180.0; 2],
        );
        assert_eq!(result.err(), Some(Error::UnsafeContainerQuery));
        assert_eq!(ui.published_frame().unwrap().revision, before);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_inline_branches_match_fresh_full_pixels_through_resize_and_retirement() {
    use crate::retained::wgpu::WgpuFrameRenderer;
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            boundary().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    ui.append_render_layout(scope, ui.root_render(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let mut renderer = WgpuFrameRenderer::new(2).unwrap();
    eprintln!("inline builder adapter: {:?}", renderer.adapter_info());
    for width in [80.0, 140.0, 80.0, 140.0] {
        let height = if width < 100.0 { 20.0 } else { 60.0 };
        let (_, frame) = ui
            .edit_frame(
                constraints(width),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        let branch =
                            edit.reconcile_keys(scope, &[u64::from(input.max >= 100.0)])?[0];
                        edit.append_layout(branch, host, fixed(400.0, height), 0xff0000ff)
                    })
                },
            )
            .unwrap();
        assert!(renderer.submit(&frame, [180, 180], 1.0).unwrap().is_some());
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full
            .append_layout(
                full.root(),
                boundary().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                0,
            )
            .unwrap();
        full.append_layout(host, fixed(400.0, height), 0xff0000ff)
            .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let reference = full
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
        assert_eq!(
            &actual.as_raw()[(10 * 180 + 10) * 4..(10 * 180 + 10) * 4 + 4],
            &[255, 0, 0, 255]
        );
        let footer_y = height as usize + 15;
        assert_eq!(
            &actual.as_raw()[(footer_y * 180 + 10) * 4..(footer_y * 180 + 10) * 4 + 4],
            &[0, 255, 0, 255]
        );
    }
    renderer.finish().unwrap();
}
