use super::*;
use std::{cell::RefCell, rc::Rc};
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
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [200, 200],
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
fn rules(base: LayoutStyle, threshold: f32, wide: LayoutStyle) -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(base, vec![(threshold, wide)]).unwrap()
}

#[test]
fn combined_viewport_container_and_keyed_inline_builders_publish_one_final_lifecycle() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let narrow = LayoutStyle::column().fixed_inline_size(70.0);
        let wide = LayoutStyle::column().fixed_inline_size(130.0);
        let host = ui
            .append_render_layout(scope, root, narrow.clone(), 0)
            .unwrap();
        let box_style = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
            .container_type(LayoutContainerType::InlineSize);
        let boundary = ui
            .append_render_layout(scope, host, box_style.clone(), 0)
            .unwrap();
        let footer = ui
            .append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let host_source = ui.state(scope, 1, || narrow.clone()).unwrap();
        let box_source = ui.state(scope, 2, || box_style.clone()).unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind_container(
                &mut ui,
                box_source,
                boundary,
                host,
                rules(
                    box_style.clone(),
                    100.0,
                    box_style
                        .clone()
                        .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                ),
            )
            .unwrap();
        responsive
            .bind(
                &mut ui,
                host_source,
                host,
                rules(narrow.clone(), 150.0, wide.clone()),
            )
            .unwrap();
        let log = Rc::new(RefCell::new(vec![]));
        let seen = log.clone();
        ui.effect(
            scope,
            3,
            &[host_source.dependency(), box_source.dependency()],
            move |input| {
                seen.borrow_mut()
                    .push(input.read(host_source).unwrap().size.width);
                None
            },
        )
        .unwrap();
        let mut old = None;
        for viewport_width in [100.0, 100.0, 200.0, 200.0, 100.0] {
            let before = ui.stats();
            let env = LayoutEnvironment::new([viewport_width, 200.0]);
            let render = |(): (), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                let stats = edit.responsive(&responsive, &env)?;
                assert_eq!(stats.rules_examined, 2);
                let wide = viewport_width >= 150.0;
                assert_eq!(
                    edit.read(host_source)?.size.width,
                    if wide {
                        LayoutDimension::Px(130.0)
                    } else {
                        LayoutDimension::Px(70.0)
                    }
                );
                edit.inline_constraint_builder(boundary, |input, edit| {
                    assert_eq!(input.max, if wide { 120.0 } else { 70.0 });
                    let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                    let state = edit.state(branch, 1, || wide)?;
                    let leaf = match edit.renders(branch)?.first() {
                        Some(leaf) => *leaf,
                        None => edit.append_layout(
                            branch,
                            boundary,
                            fixed(400.0, if wide { 40.0 } else { 20.0 }),
                            0xff0000ff,
                        )?,
                    };
                    Ok((state, leaf))
                })
            };
            let (commit, frame) = if local {
                let (commit, frame) = ui
                    .edit_local_frame(constraints(180.0), [200.0; 2], |_| Ok(()), render)
                    .unwrap();
                (commit, frame.resolve())
            } else {
                ui.edit_frame(constraints(180.0), [200.0; 2], |_| Ok(()), render)
                    .unwrap()
            };
            let wide = viewport_width >= 150.0;
            assert_eq!(*ui.read(commit.result.value.0).unwrap(), wide);
            assert_eq!(ui.stats().transactions, before.transactions + 1);
            assert_eq!(
                ui.layout_result(footer).unwrap().location[1],
                if wide { 50.0 } else { 20.0 }
            );
            assert_eq!(frame.hit_test(6.0, 6.0), Some(commit.result.value.1));
            let mut full = UiTree::new_layout(column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let h = full
                .append_layout(
                    full.root(),
                    if wide {
                        LayoutStyle::column().fixed_inline_size(130.0)
                    } else {
                        narrow.clone()
                    },
                    0,
                )
                .unwrap();
            let b = full
                .append_layout(
                    h,
                    if wide {
                        box_style
                            .clone()
                            .padding(LayoutEdges::all(LayoutLength::Px(5.0)))
                    } else {
                        box_style.clone()
                    },
                    0,
                )
                .unwrap();
            full.append_layout(b, fixed(400.0, if wide { 40.0 } else { 20.0 }), 0xff0000ff)
                .unwrap();
            full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
                .unwrap();
            assert_eq!(
                pixels(frame.clone()),
                pixels(
                    full.frame_in_viewport(constraints(180.0), [200.0; 2])
                        .unwrap()
                )
            );
            if old.is_none() {
                old = Some((frame.clone(), pixels(frame)));
            }
        }
        assert_eq!(
            log.borrow().len(),
            3,
            "effects observe each final transition once"
        );
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
}

#[test]
fn later_structure_changes_reject_query_observations_and_keep_sources_effects_and_handles() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host_style = LayoutStyle::column().fixed_inline_size(80.0);
    let host = ui
        .append_render_layout(scope, root, host_style.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let app = ui.state(scope, 2, || 0u32).unwrap();
    let mut ruleset = LayoutResponsiveBindings::new();
    ruleset
        .bind_container(
            &mut ui,
            source,
            leaf,
            host,
            rules(fixed(20.0, 10.0), 70.0, fixed(20.0, 30.0)),
        )
        .unwrap();
    let (_, old) = ui
        .edit_local_frame(constraints(180.0), [200.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let before = ui.stats();
    let mut escaped = None;
    let error = ui.edit_local_frame(
        constraints(180.0),
        [200.0; 2],
        |tx| {
            tx.set(app, 1)?;
            Ok(())
        },
        |(), edit| {
            edit.responsive(&ruleset, &LayoutEnvironment::new([180.0, 200.0]))?;
            let branch = edit.reconcile_keys(scope, &[99])?[0];
            let node = edit.append_layout(branch, root, fixed(10.0, 10.0), 1)?;
            escaped = Some(node);
            edit.properties()
                .layout_style(host, LayoutStyle::column().fixed_inline_size(60.0));
            Ok(())
        },
    );
    assert!(matches!(error, Err(Error::UnsafeContainerQuery)));
    assert_eq!(*ui.read(app).unwrap(), 0);
    assert_eq!(ui.read(source).unwrap(), &fixed(20.0, 10.0));
    assert_eq!(ui.stats(), before);
    assert!(ui.children(scope).unwrap().is_empty());
    assert!(ui.layout_result(escaped.unwrap()).is_err());
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        old.revision()
    );
    assert_eq!(old.hit_test(1.0, 1.0), Some(leaf));
}

#[test]
fn derived_sinks_and_computations_created_before_responsive_stage_see_settled_values() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let base = fixed(30.0, 20.0);
    let host = ui
        .append_render_layout(scope, root, base.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(10.0, 10.0), 0)
        .unwrap();
    let source = ui.state(scope, 1, || base.clone()).unwrap();
    let mut ruleset = LayoutResponsiveBindings::new();
    ruleset
        .bind(
            &mut ui,
            source,
            host,
            rules(base.clone(), 100.0, fixed(80.0, 20.0)),
        )
        .unwrap();
    let observed = Rc::new(RefCell::new(vec![]));
    let seen = observed.clone();
    let (commit, frame) = ui
        .edit_frame(
            constraints(180.0),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                let wide = edit.computed(scope, 2, &[source.dependency()], move |input| {
                    Ok(input.read(source)?.size.width == LayoutDimension::Px(80.0))
                })?;
                let color = edit.computed(scope, 3, &[wide.dependency()], move |input| {
                    Ok(if *input.read(wide)? {
                        0x00ff00ffu32
                    } else {
                        0xff0000ffu32
                    })
                })?;
                edit.bind(color, Property::color(leaf))?;
                edit.effect(scope, 4, &[color.dependency()], move |input| {
                    seen.borrow_mut().push(*input.read(color).unwrap());
                    None
                })?;
                edit.responsive(&ruleset, &LayoutEnvironment::new([180.0, 200.0]))?;
                assert_eq!(*edit.read(color)?, 0x00ff00ff);
                Ok(color)
            },
        )
        .unwrap();
    assert_eq!(*ui.read(commit.result.value).unwrap(), 0x00ff00ff);
    assert_eq!(
        frame
            .items
            .iter()
            .find(|p| p.node == leaf)
            .unwrap()
            .fill_color,
        0x00ff00ff
    );
    assert_eq!(*observed.borrow(), [0x00ff00ff]);
    // The newly published reverse dependencies must also work on the next
    // ordinary source transaction, without a structural responsive stage.
    let color = commit.result.value;
    let commit = ui
        .transaction(|tx| {
            tx.set(source, base.clone())?;
            Ok(())
        })
        .unwrap();
    assert_eq!(commit.effect_runs, 1);
    assert_eq!(*ui.read(color).unwrap(), 0xff0000ff);
    assert_eq!(*observed.borrow(), [0x00ff00ff, 0xff0000ff]);
}

#[test]
fn failed_responsive_stage_can_be_caught_without_leaking_earlier_stage_values() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host_style = LayoutStyle::column().fixed_inline_size(50.0);
    let host = ui
        .append_render_layout(scope, root, host_style.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(10.0, 10.0), 0xff0000ff)
        .unwrap();
    let host_source = ui.state(scope, 1, || host_style.clone()).unwrap();
    let leaf_source = ui.state(scope, 2, || fixed(10.0, 10.0)).unwrap();
    let derived = ui
        .computed(scope, 3, &[leaf_source.dependency()], move |input| {
            if input.read(leaf_source)?.size.height == LayoutDimension::Px(30.0) {
                return Err(Error::InvalidPrimitive);
            }
            Ok(0xff0000ffu32)
        })
        .unwrap();
    ui.bind(derived, Property::color(leaf)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            host_source,
            host,
            rules(
                host_style.clone(),
                150.0,
                LayoutStyle::column().fixed_inline_size(80.0),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            leaf_source,
            leaf,
            host,
            rules(fixed(10.0, 10.0), 70.0, fixed(10.0, 30.0)),
        )
        .unwrap();
    let (_, frame) = ui
        .edit_frame(
            constraints(180.0),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                assert_eq!(
                    edit.responsive(&responsive, &LayoutEnvironment::new([200.0, 200.0])),
                    Err(Error::InvalidPrimitive)
                );
                assert_eq!(edit.read(host_source)?, &host_style);
                assert_eq!(edit.read(leaf_source)?, &fixed(10.0, 10.0));
                edit.properties().translation(host, [2.0, 0.0]);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(ui.read(host_source).unwrap(), &host_style);
    assert_eq!(frame.hit_test(3.0, 1.0), Some(leaf));
}

#[test]
fn transient_responsive_values_restore_without_paint_or_effect_and_idle_local_stays_sparse() {
    let mut ui = LogicalUi::new_layout(
        column(),
        0,
        BackendCapabilities {
            solid_color_patch: true,
        },
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let base = fixed(30.0, 20.0);
    let target = ui
        .append_render_layout(scope, root, base.clone(), 0xff0000ff)
        .unwrap();
    let source = ui.state(scope, 1, || base.clone()).unwrap();
    let observed = Rc::new(RefCell::new(vec![]));
    let log = observed.clone();
    ui.effect(scope, 2, &[source.dependency()], move |input| {
        log.borrow_mut()
            .push(input.read(source).unwrap().size.width);
        None
    })
    .unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            source,
            target,
            rules(base.clone(), 100.0, fixed(80.0, 20.0)),
        )
        .unwrap();
    let (_, old) = ui
        .edit_local_frame(constraints(180.0), [200.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap();
    let (commit, next) = ui
        .edit_local_frame(
            constraints(180.0),
            [200.0; 2],
            |tx| {
                tx.set(source, fixed(80.0, 20.0))?;
                Ok(())
            },
            |(), edit| {
                edit.responsive(&responsive, &LayoutEnvironment::new([50.0, 200.0]))?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(commit.result.changed_sources, 0);
    assert_eq!(commit.result.effect_runs, 0);
    assert!(!commit.result.commit.request_frame);
    assert_eq!(next.revision(), old.revision());
    assert_eq!(observed.borrow().len(), 1);
    let (_, patch) = ui
        .edit_local_frame(
            constraints(180.0),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.responsive(&responsive, &LayoutEnvironment::new([50.0, 200.0]))?;
                edit.properties().translation(target, [3.0, 0.0]);
                Ok(())
            },
        )
        .unwrap();
    assert!(!patch.stats().full_publication);
    assert_eq!(patch.stats().changed_nodes, 1);
    assert_eq!(patch.hit_test(4.0, 1.0), Some(target));
}

#[test]
fn final_writer_replacement_target_removal_or_dependency_proof_change_rejects_atomically() {
    for mode in 0..3 {
        let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host_style = LayoutStyle::column().fixed_inline_size(80.0);
        let host = ui.append_render_layout(scope, root, host_style, 0).unwrap();
        let leaf = ui
            .append_render_layout(scope, host, fixed(10.0, 10.0), 0xff0000ff)
            .unwrap();
        let source = ui.state(scope, 1, || fixed(10.0, 10.0)).unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind_container(
                &mut ui,
                source,
                leaf,
                host,
                rules(fixed(10.0, 10.0), 70.0, fixed(10.0, 30.0)),
            )
            .unwrap();
        let binding = responsive.bindings[0].binding;
        let (_, old) = ui
            .edit_local_frame(constraints(180.0), [200.0; 2], |_| Ok(()), |(), _| Ok(()))
            .unwrap();
        let before = ui.stats();
        let result = ui.edit_local_frame(
            constraints(180.0),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.responsive(&responsive, &LayoutEnvironment::new([180.0, 200.0]))?;
                match mode {
                    0 => {
                        edit.unbind(binding)?;
                        edit.bind(source, Property::layout_style(leaf))?;
                    }
                    1 => edit.remove(leaf)?,
                    _ => edit.properties().layout_style(
                        host,
                        LayoutStyle::column()
                            .size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
                    ),
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(ui.stats(), before);
        assert_eq!(ui.read(source).unwrap(), &fixed(10.0, 10.0));
        assert_eq!(
            ui.published_local_frame().unwrap().revision(),
            old.revision()
        );
        assert_eq!(old.hit_test(1.0, 1.0), Some(leaf));
    }
}

#[test]
fn new_computed_sibling_sink_that_introduces_query_feedback_is_rejected_before_publication() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Px(50.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host_style = LayoutStyle::column()
        .container_type(LayoutContainerType::Size)
        .flex_grow(1.0)
        .size(LayoutDimension::Auto, LayoutDimension::Px(40.0));
    let host = ui.append_render_layout(scope, root, host_style, 0).unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(10.0, 10.0), 0)
        .unwrap();
    let sibling = ui
        .append_render_layout(scope, root, fixed(10.0, 10.0), 0)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(10.0, 10.0)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            leaf,
            host,
            rules(fixed(10.0, 10.0), 70.0, fixed(10.0, 30.0)),
        )
        .unwrap();
    let before = ui.reactive().source_count();
    let result = ui.edit_frame(
        constraints(180.0),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive(&responsive, &LayoutEnvironment::new([180.0, 200.0]))?;
            let computed = edit.computed(scope, 2, &[source.dependency()], move |input| {
                input.read(source)?;
                Ok(fixed(10.0, 10.0))
            })?;
            edit.bind(computed, Property::layout_style(sibling))?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::UnsafeContainerQuery)));
    assert_eq!(ui.reactive().source_count(), before);
}

#[test]
fn nested_query_stages_observe_prior_reparent_and_settle_before_branch_construction() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let outer = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::column().fixed_inline_size(130.0),
            0,
        )
        .unwrap();
    let other = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::column().fixed_inline_size(50.0),
            0,
        )
        .unwrap();
    let inner_style = LayoutStyle::column().fixed_inline_size(50.0);
    let inner = ui
        .append_render_layout(scope, outer, inner_style.clone(), 0)
        .unwrap();
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let host = ui
        .append_render_layout(scope, inner, boundary.clone(), 0)
        .unwrap();
    let inner_source = ui.state(scope, 1, || inner_style.clone()).unwrap();
    let source = ui.state(scope, 2, || boundary.clone()).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            host,
            inner,
            rules(
                boundary.clone(),
                70.0,
                boundary
                    .clone()
                    .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            inner_source,
            inner,
            outer,
            rules(
                inner_style,
                100.0,
                LayoutStyle::column().fixed_inline_size(80.0),
            ),
        )
        .unwrap();
    // Reparenting the registered host temporarily before the responsive read is
    // allowed when final query ancestry and dependency proofs remain valid.
    let (commit, frame) = ui
        .edit_frame(
            constraints(180.0),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.reparent(inner, other)?;
                edit.reparent(inner, outer)?;
                let stats =
                    edit.responsive(&responsive, &LayoutEnvironment::new([500.0, 200.0]))?;
                assert_eq!(stats.changed_styles, 2);
                assert_eq!(stats.layout_runs, 2);
                edit.inline_constraint_builder(host, |input, edit| {
                    assert_eq!(input.max, 70.0);
                    edit.append_layout(scope, host, fixed(400.0, 20.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    assert_eq!(ui.layout_result(inner).unwrap().size, [80.0, 30.0]);
    assert_eq!(frame.hit_test(6.0, 6.0), Some(commit.result.value));
}

#[test]
fn responsive_without_frame_or_with_invalid_environment_cannot_publish_partial_source_edits() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let app = ui.state(scope, 1, || 0u32).unwrap();
    let responsive = LayoutResponsiveBindings::new();
    let before = ui.stats();
    let result = ui.edit(
        |tx| {
            tx.set(app, 1)?;
            Ok(())
        },
        |(), edit| edit.responsive(&responsive, &LayoutEnvironment::new([200.0; 2])),
    );
    assert!(matches!(result, Err(Error::UnsafeContainerQuery)));
    assert_eq!(*ui.read(app).unwrap(), 0);
    assert_eq!(ui.stats(), before);
    let mut environment = LayoutEnvironment::new([200.0; 2]);
    environment.text_scale = 0.0;
    let result = ui.edit_local_frame(
        constraints(180.0),
        [200.0; 2],
        |tx| {
            tx.set(app, 1)?;
            Ok(())
        },
        |(), edit| edit.responsive(&responsive, &environment),
    );
    assert!(matches!(result, Err(Error::InvalidEnvironment)));
    assert_eq!(*ui.read(app).unwrap(), 0);
    assert_eq!(ui.stats(), before);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_structural_responsive_frames_match_fresh_full_pixels() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let host_style = LayoutStyle::column().fixed_inline_size(70.0);
    let host = ui
        .append_render_layout(scope, root, host_style.clone(), 0)
        .unwrap();
    let boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
        .container_type(LayoutContainerType::InlineSize);
    let slot = ui
        .append_render_layout(scope, host, boundary.clone(), 0)
        .unwrap();
    let footer = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let source = ui.state(scope, 1, || host_style.clone()).unwrap();
    let slot_source = ui.state(scope, 2, || boundary.clone()).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            slot_source,
            slot,
            host,
            rules(
                boundary.clone(),
                100.0,
                boundary
                    .clone()
                    .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            ),
        )
        .unwrap();
    responsive
        .bind(
            &mut ui,
            source,
            host,
            rules(
                host_style.clone(),
                150.0,
                host_style.clone().fixed_inline_size(130.0),
            ),
        )
        .unwrap();
    let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!(
        "structural responsive adapter: {:?}",
        renderer.adapter_info()
    );
    for env_width in [100.0, 200.0, 100.0, 200.0] {
        let wide = env_width >= 150.0;
        let (commit, frame) = ui
            .edit_local_frame(
                constraints(180.0),
                [200.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.responsive(&responsive, &LayoutEnvironment::new([env_width, 200.0]))?;
                    edit.inline_constraint_builder(slot, |input, edit| {
                        assert_eq!(input.max, if wide { 120.0 } else { 70.0 });
                        let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                        let leaf = edit.append_layout(
                            branch,
                            slot,
                            fixed(400.0, if wide { 40.0 } else { 20.0 }),
                            0xff0000ff,
                        )?;
                        Ok(leaf)
                    })
                },
            )
            .unwrap();
        renderer
            .submit_local(&frame, [200, 200], 1.0)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let h = full
            .append_layout(
                full.root(),
                host_style
                    .clone()
                    .fixed_inline_size(if wide { 130.0 } else { 70.0 }),
                0,
            )
            .unwrap();
        let b = full
            .append_layout(
                h,
                if wide {
                    boundary
                        .clone()
                        .padding(LayoutEdges::all(LayoutLength::Px(5.0)))
                } else {
                    boundary.clone()
                },
                0,
            )
            .unwrap();
        full.append_layout(b, fixed(400.0, if wide { 40.0 } else { 20.0 }), 0xff0000ff)
            .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let reference = full
            .frame_in_viewport(constraints(180.0), [200.0; 2])
            .unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&reference, [200, 200], 1.0)
                .unwrap()
                .as_raw()
        );
        assert_eq!(frame.hit_test(6.0, 6.0), Some(commit.result.value));
        let red = (6 * 200 + 6) * 4;
        assert_eq!(&actual.as_raw()[red..red + 4], &[255, 0, 0, 255]);
        let footer_y = if wide { 50 } else { 20 };
        assert_eq!(
            ui.layout_result(footer).unwrap().location[1],
            footer_y as f32
        );
        let green = ((footer_y + 1) * 200 + 1) * 4;
        assert_eq!(&actual.as_raw()[green..green + 4], &[0, 255, 0, 255]);
    }
    renderer.finish().unwrap();
}
