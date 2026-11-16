use super::*;
use crate::retained::{responsive::*, software};

fn column() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(180.0),
        LayoutAvailableSpace::Unbounded,
    )
}
fn rules(base: LayoutStyle, width: f32, wide: LayoutStyle) -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(base, vec![(width, wide)]).unwrap()
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

#[test]
fn new_scope_sources_rules_and_inline_content_publish_and_retire_atomically() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let mut old = None;
        for env in [100.0, 100.0, 200.0, 200.0, 100.0] {
            let wide = env >= 150.0;
            let render = |(): (), edit: &mut LogicalEdit<'_, '_, '_>| {
                let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                let base = LayoutStyle::column().fixed_inline_size(70.0);
                let box_style =
                    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                        .container_type(LayoutContainerType::InlineSize);
                let current = edit.renders(branch)?;
                let (host, boundary, footer) = if current.is_empty() {
                    let host = edit.append_layout(branch, root, base.clone(), 0)?;
                    let boundary = edit.append_layout(branch, host, box_style.clone(), 0)?;
                    let footer = edit.append_layout(branch, root, fixed(20.0, 10.0), 0x00ff00ff)?;
                    (host, boundary, footer)
                } else {
                    (current[0], current[1], current[2])
                };
                let source = edit.state(branch, 1, || base.clone())?;
                let slot = edit.state(branch, 2, || box_style.clone())?;
                // Register the dependent first, including provisional handles.
                edit.responsive_rule(
                    branch,
                    1,
                    slot,
                    boundary,
                    LayoutQuery::ContainerInline(host),
                    rules(
                        box_style.clone(),
                        100.0,
                        box_style
                            .clone()
                            .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                    ),
                )?;
                edit.responsive_rule(
                    branch,
                    2,
                    source,
                    host,
                    LayoutQuery::ViewportInline,
                    rules(base.clone(), 150.0, base.clone().fixed_inline_size(130.0)),
                )?;
                let stats = edit.responsive_registered(&LayoutEnvironment::new([env, 200.0]))?;
                assert_eq!(stats.rules_examined, 2);
                let leaf = edit.inline_constraint_builder(boundary, |input, edit| {
                    assert_eq!(input.max, if wide { 120.0 } else { 70.0 });
                    let child = edit.reconcile_keys(branch, &[0])?[0];
                    Ok(match edit.renders(child)?.first() {
                        Some(node) => *node,
                        None => edit.append_layout(
                            child,
                            boundary,
                            fixed(400.0, if wide { 40.0 } else { 20.0 }),
                            0xff0000ff,
                        )?,
                    })
                })?;
                Ok((source, leaf, footer))
            };
            let (commit, frame) = if local {
                let (c, f) = ui
                    .edit_local_frame(constraints(), [200.0; 2], |_| Ok(()), render)
                    .unwrap();
                (c, f.resolve())
            } else {
                ui.edit_frame(constraints(), [200.0; 2], |_| Ok(()), render)
                    .unwrap()
            };
            assert_eq!(ui.responsive_rules.len(), 2);
            assert_eq!(
                ui.layout_result(commit.result.value.2).unwrap().location[1],
                if wide { 50.0 } else { 20.0 }
            );
            assert_eq!(frame.hit_test(6.0, 6.0), Some(commit.result.value.1));
            let mut full = UiTree::new_layout(column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let h = full
                .append_layout(
                    full.root(),
                    LayoutStyle::column().fixed_inline_size(if wide { 130.0 } else { 70.0 }),
                    0,
                )
                .unwrap();
            let mut b = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                .container_type(LayoutContainerType::InlineSize);
            if wide {
                b = b.padding(LayoutEdges::all(LayoutLength::Px(5.0)));
            }
            let b = full.append_layout(h, b, 0).unwrap();
            full.append_layout(b, fixed(400.0, if wide { 40.0 } else { 20.0 }), 0xff0000ff)
                .unwrap();
            full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
                .unwrap();
            let actual = pixels(frame.clone());
            assert_eq!(
                actual,
                pixels(full.frame_in_viewport(constraints(), [200.0; 2]).unwrap())
            );
            if old.is_none() {
                old = Some((frame, actual, commit.result.value.0));
            }
        }
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(scope, &[])?;
                Ok(())
            },
        )
        .unwrap();
        assert!(ui.responsive_rules.is_empty());
        let (frame, expected, source) = old.unwrap();
        assert!(ui.read(source).is_err());
        drop(ui);
        assert_eq!(pixels(frame), expected);
    }
}

#[test]
fn rule_updates_and_removal_preserve_writer_identity_and_allow_endpoint_reuse() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let node = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let mut original_writer = None;
    for width in [40.0, 40.0, 60.0] {
        ui.edit_local_frame(
            constraints(),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.responsive_rule(
                    scope,
                    10,
                    source,
                    node,
                    LayoutQuery::ViewportInline,
                    rules(fixed(20.0, 10.0), 100.0, fixed(width, 10.0)),
                )?;
                edit.responsive_registered(&LayoutEnvironment::new([200.0; 2]))?;
                Ok(())
            },
        )
        .unwrap();
        let writer = ui.responsive_rules[&(scope, 10)].binding;
        assert_eq!(*original_writer.get_or_insert(writer), writer);
        assert_eq!(ui.layout_result(node).unwrap().size[0], width);
        if width == 40.0 {
            let binding = ui.responsive_rules[&(scope, 10)].binding;
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.responsive_rule(
                        scope,
                        10,
                        source,
                        node,
                        LayoutQuery::ViewportInline,
                        rules(fixed(20.0, 10.0), 100.0, fixed(width, 10.0)),
                    )
                },
            )
            .unwrap();
            assert_eq!(binding, ui.responsive_rules[&(scope, 10)].binding);
        }
    }
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.remove_responsive_rule(scope, 10)?;
            edit.bind(source, Property::layout_style(node))?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.responsive_rules.is_empty());
    ui.transaction(|tx| {
        tx.set(source, fixed(30.0, 10.0))?;
        Ok(())
    })
    .unwrap();
    ui.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
    assert_eq!(ui.layout_result(node).unwrap().size[0], 30.0);
}

#[test]
fn failed_final_observation_preserves_registry_sources_and_old_frame() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let node = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    ui.edit_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                10,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )?;
            edit.responsive_registered(&LayoutEnvironment::new([50.0; 2]))
        },
    )
    .unwrap();
    let before = ui.stats();
    let revision = ui.published_frame().unwrap().revision;
    let binding = ui.responsive_rules[&(scope, 10)].binding;
    let result = ui.edit_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                10,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(80.0, 10.0)),
            )?;
            edit.responsive_registered(&LayoutEnvironment::new([200.0; 2]))?;
            edit.properties().layout_style(node, fixed(90.0, 10.0));
            // Final source sink wins an ordinary property write. Remove the observed
            // writer to make the edit truly invalidate its observed ownership.
            edit.remove_responsive_rule(scope, 10)
        },
    );
    assert!(matches!(result, Err(Error::InvalidBinding)));
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.published_frame().unwrap().revision, revision);
    assert_eq!(ui.read(source).unwrap(), &fixed(20.0, 10.0));
    assert_eq!(ui.responsive_rules[&(scope, 10)].binding, binding);
    // This failure happens in final frame validation, after rule/source
    // settlement and registry preflight, without an error from the closure.
    let result = ui.edit_local_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                10,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(80.0, 10.0)),
            )?;
            edit.responsive_registered(&LayoutEnvironment::new([200.0; 2]))?;
            let child = edit.append_layout(scope, node, fixed(10.0, 10.0), 1)?;
            edit.properties().translation(node, [f32::MAX; 2]);
            edit.properties().translation(child, [f32::MAX; 2]);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidFrame)));
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.published_frame().unwrap().revision, revision);
    assert_eq!(ui.read(source).unwrap(), &fixed(20.0, 10.0));
    assert_eq!(ui.responsive_rules[&(scope, 10)].binding, binding);
    ui.edit_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| edit.responsive_registered(&LayoutEnvironment::new([200.0; 2])),
    )
    .unwrap();
    assert_eq!(ui.read(source).unwrap(), &fixed(40.0, 10.0));
}

#[test]
fn caught_invalid_registration_keeps_registry_and_endpoint_available() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let node = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            assert!(matches!(
                edit.responsive_rule(
                    scope,
                    10,
                    source,
                    node,
                    LayoutQuery::ContainerInline(node),
                    rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0))
                ),
                Err(Error::UnsafeContainerQuery)
            ));
            edit.responsive_rule(
                scope,
                10,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )?;
            assert!(matches!(
                edit.responsive_rule(
                    scope,
                    11,
                    source,
                    node,
                    LayoutQuery::ViewportInline,
                    rules(fixed(20.0, 10.0), 100.0, fixed(50.0, 10.0))
                ),
                Err(Error::BindingConflict)
            ));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ui.responsive_rules.len(), 1);
}

#[test]
fn scoped_retirement_detaches_rules_using_surviving_sources_in_legacy_reconciliation() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let child = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let node = ui
        .append_render_layout(child, root, fixed(20.0, 10.0), 1)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                child,
                1,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )
        },
    )
    .unwrap();
    ui.reconcile_keys(scope, &[]).unwrap();
    assert!(ui.responsive_rules.is_empty());
    assert!(ui.read(source).is_ok());
    let node = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    ui.bind(source, Property::layout_style(node)).unwrap();
}

#[test]
fn declarations_require_owned_live_target_and_cannot_survive_missing_dependencies() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let child = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let source = ui.state(child, 1, || fixed(20.0, 10.0)).unwrap();
    let node = ui
        .append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            assert!(matches!(
                edit.responsive_rule(
                    child,
                    1,
                    source,
                    node,
                    LayoutQuery::ViewportInline,
                    rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0))
                ),
                Err(Error::ScopeInUse)
            ));
            edit.responsive_rule(
                scope,
                1,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )
        },
    )
    .unwrap();
    assert!(matches!(
        ui.reconcile_keys(scope, &[]),
        Err(Error::UnsafeContainerQuery)
    ));
    assert!(ui.read(source).is_ok());
    assert!(
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.reconcile_keys(scope, &[])?;
                Ok(())
            }
        )
        .is_err()
    );
    assert!(ui.read(source).is_ok());
    assert_eq!(ui.responsive_rules.len(), 1);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.remove_responsive_rule(scope, 1)?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.responsive_rules.is_empty());
    assert!(ui.read(source).is_err());
}

#[test]
fn aborted_new_rule_cannot_publish_handles_and_cancelled_construction_leaves_no_writer() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let mut escaped = None;
    let before = ui.stats();
    let result = ui.edit_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            let branch = edit.reconcile_keys(scope, &[1])?[0];
            let source = edit.state(branch, 1, || fixed(20.0, 10.0))?;
            let node = edit.append_layout(branch, root, fixed(20.0, 10.0), 1)?;
            edit.responsive_rule(
                branch,
                1,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )?;
            escaped = Some((source, node));
            edit.responsive_registered(&LayoutEnvironment::new([200.0; 2]))?;
            Err::<(), _>(Error::InvalidFrame)
        },
    );
    assert!(matches!(result, Err(Error::InvalidFrame)));
    assert!(ui.responsive_rules.is_empty());
    assert_eq!(ui.stats(), before);
    let (source, node) = escaped.unwrap();
    assert!(ui.read(source).is_err());
    assert!(ui.layout_result(node).is_err());
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            let branch = edit.reconcile_keys(scope, &[1])?[0];
            let source = edit.state(branch, 1, || fixed(20.0, 10.0))?;
            let node = edit.append_layout(branch, root, fixed(20.0, 10.0), 1)?;
            edit.responsive_rule(
                branch,
                1,
                source,
                node,
                LayoutQuery::ViewportInline,
                rules(fixed(20.0, 10.0), 100.0, fixed(40.0, 10.0)),
            )?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.responsive_rules.is_empty());
    assert!(ui.children(scope).unwrap().is_empty());
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_scoped_rule_branch_replacement_matches_fresh_full_pixels() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("scoped responsive adapter: {:?}", renderer.adapter_info());
    for env in [100.0, 200.0, 100.0, 200.0] {
        let wide = env >= 150.0;
        let (commit, frame) = ui
            .edit_local_frame(
                constraints(),
                [200.0; 2],
                |_| Ok(()),
                |(), edit| {
                    let branch = edit.reconcile_keys(scope, &[u64::from(wide)])?[0];
                    let base = LayoutStyle::column().fixed_inline_size(70.0);
                    let host = edit.append_layout(branch, root, base.clone(), 0)?;
                    let source = edit.state(branch, 1, || base.clone())?;
                    let boundary =
                        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                            .container_type(LayoutContainerType::InlineSize);
                    let slot = edit.append_layout(branch, host, boundary.clone(), 0)?;
                    let slot_source = edit.state(branch, 2, || boundary.clone())?;
                    edit.responsive_rule(
                        branch,
                        1,
                        source,
                        host,
                        LayoutQuery::ViewportInline,
                        rules(base.clone(), 150.0, base.fixed_inline_size(130.0)),
                    )?;
                    edit.responsive_rule(
                        branch,
                        2,
                        slot_source,
                        slot,
                        LayoutQuery::ContainerInline(host),
                        rules(
                            boundary.clone(),
                            100.0,
                            boundary.padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                        ),
                    )?;
                    edit.responsive_registered(&LayoutEnvironment::new([env, 200.0]))?;
                    edit.inline_constraint_builder(slot, |input, edit| {
                        assert_eq!(input.max, if wide { 120.0 } else { 70.0 });
                        edit.append_layout(
                            branch,
                            slot,
                            fixed(400.0, if wide { 40.0 } else { 20.0 }),
                            0xff0000ff,
                        )
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
                LayoutStyle::column().fixed_inline_size(if wide { 130.0 } else { 70.0 }),
                0,
            )
            .unwrap();
        let mut boundary = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
            .container_type(LayoutContainerType::InlineSize);
        if wide {
            boundary = boundary.padding(LayoutEdges::all(LayoutLength::Px(5.0)));
        }
        let b = full.append_layout(h, boundary, 0).unwrap();
        full.append_layout(b, fixed(400.0, if wide { 40.0 } else { 20.0 }), 0xff0000ff)
            .unwrap();
        let reference = full.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
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
    }
    renderer.finish().unwrap();
}

#[test]
fn surviving_portal_rule_cannot_keep_a_retired_query_host() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let child = ui.reconcile_keys(scope, &[1]).unwrap().order[0];
    let host = ui
        .append_render_layout(
            child,
            root,
            LayoutStyle::column().fixed_inline_size(80.0),
            0,
        )
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 1)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                1,
                source,
                leaf,
                LayoutQuery::ContainerInline(host),
                rules(fixed(20.0, 10.0), 50.0, fixed(40.0, 10.0)),
            )
        },
    )
    .unwrap();
    let old = ui.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
    assert!(matches!(
        ui.reconcile_keys(scope, &[]),
        Err(Error::UnsafeContainerQuery)
    ));
    let result = ui.edit_local_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.reparent(leaf, root)?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidNode)));
    assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    assert_eq!(ui.responsive_rules.len(), 1);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 80.0);
    ui.edit_local_frame(
        constraints(),
        [200.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.remove_responsive_rule(scope, 1)?;
            edit.reparent(leaf, root)?;
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.responsive_rules.is_empty());
    assert!(ui.layout_result(host).is_err());
    assert!(ui.layout_result(leaf).is_ok());
}
