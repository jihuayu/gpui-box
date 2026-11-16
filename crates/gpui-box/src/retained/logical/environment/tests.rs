use super::*;
use crate::retained::{responsive::*, software};
use std::{cell::RefCell, rc::Rc};

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn column() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn constraints() -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(180.0),
        LayoutAvailableSpace::Unbounded,
    )
}
fn rules(a: LayoutStyle, breakpoint: f32, b: LayoutStyle) -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(a, vec![(breakpoint, b)]).unwrap()
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
fn fixture() -> (
    LogicalUi,
    NodeId,
    NodeId,
    Signal<LayoutStyle>,
    Signal<LayoutStyle>,
) {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let host_style = LayoutStyle::column().fixed_inline_size(70.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), host_style.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let host_source = ui.state(scope, 1, || host_style.clone()).unwrap();
    let source = ui.state(scope, 2, || fixed(20.0, 20.0)).unwrap();
    ui.bind(host_source, Property::layout_style(host)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                1,
                source,
                leaf,
                LayoutQuery::ContainerInline(host),
                rules(fixed(20.0, 20.0), 100.0, fixed(40.0, 30.0)),
            )
        },
    )
    .unwrap();
    (ui, host, leaf, host_source, source)
}
fn reference(host_width: f32, wide: bool, direction: LayoutDirection) -> FramePacket {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column().fixed_inline_size(host_width),
            0,
        )
        .unwrap();
    tree.append_layout(
        host,
        fixed(
            if wide { 40.0 } else { 20.0 },
            if wide { 30.0 } else { 20.0 },
        )
        .direction(direction),
        0xff0000ff,
    )
    .unwrap();
    tree.frame_in_viewport(constraints(), [200.0; 2]).unwrap()
}

#[test]
fn automatic_source_and_worker_transactions_publish_final_pixels_effects_and_noops() {
    let (mut ui, host, leaf, host_source, source) = fixture();
    let old = ui
        .local_frame_with_environment(
            &LayoutEnvironment::new([500.0, 300.0]),
            constraints(),
            [200.0; 2],
        )
        .unwrap();
    let old_pixels = pixels(old.resolve());
    let events = Rc::new(RefCell::new(vec![]));
    let out = events.clone();
    ui.effect(ui.root_scope(), 1, &[source.dependency()], move |input| {
        out.borrow_mut().push(input.read(source).unwrap().clone());
        None
    })
    .unwrap();
    let mut receiver = ui.open_updates(1).unwrap();
    let sender = ui.update_sender(&receiver, host_source).unwrap();
    for (index, width) in [130.0, 70.0, 130.0, 130.0].into_iter().enumerate() {
        let wide = width >= 100.0;
        let before = ui.stats();
        let commit = if index % 2 == 0 {
            ui.transaction(|tx| {
                tx.set(host_source, LayoutStyle::column().fixed_inline_size(width))?;
                Ok(())
            })
            .unwrap()
        } else {
            let worker = sender.clone();
            std::thread::spawn(move || {
                worker
                    .send(LayoutStyle::column().fixed_inline_size(90.0))
                    .unwrap();
                worker
                    .send(LayoutStyle::column().fixed_inline_size(width))
                    .unwrap();
            })
            .join()
            .unwrap();
            let batch = ui.apply_updates(&mut receiver).unwrap();
            assert_eq!((batch.applied, batch.retired), (1, 0));
            batch.commit.unwrap()
        };
        let frame = ui.published_local_frame().unwrap().clone();
        assert_eq!(ui.stats().transactions, before.transactions + 1);
        assert_eq!(ui.layout_result(host).unwrap().size[0], width);
        assert_eq!(
            ui.layout_result(leaf).unwrap().size,
            if wide { [40.0, 30.0] } else { [20.0, 20.0] }
        );
        assert_eq!(
            ui.read(source).unwrap(),
            &fixed(
                if wide { 40.0 } else { 20.0 },
                if wide { 30.0 } else { 20.0 }
            )
        );
        assert_eq!(frame.hit_test(2.0, 2.0), Some(leaf));
        assert_eq!(
            pixels(frame.resolve()),
            pixels(reference(width, wide, LayoutDirection::Ltr))
        );
        if index == 3 {
            assert!(!commit.commit.request_frame);
            assert_eq!((commit.changed_sources, commit.effect_runs), (0, 0));
        } else {
            assert_eq!(commit.effect_runs, 1);
            assert_eq!(events.borrow().last(), Some(ui.read(source).unwrap()));
        }
    }
    assert!(ui.apply_updates(&mut receiver).unwrap().commit.is_none());
    assert_eq!(receiver.stats().unwrap().coalesced, 2);
    drop(ui);
    assert_eq!(pixels(old.resolve()), old_pixels);
}

#[test]
fn environment_resize_entrypoints_keep_layout_query_and_paint_inputs_independent() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let base = LayoutStyle::column().fixed_inline_size(70.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), base.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let host_source = ui
        .state(scope, 9, || LayoutStyle::column().fixed_inline_size(70.0))
        .unwrap();
    let source = ui.state(scope, 10, || fixed(20.0, 20.0)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            // Declare consumer first: stage order comes from the dependency DAG.
            edit.responsive_rule(
                scope,
                1,
                source,
                leaf,
                LayoutQuery::ContainerInline(host),
                rules(fixed(20.0, 20.0), 100.0, fixed(40.0, 30.0)),
            )?;
            edit.responsive_rule(
                scope,
                2,
                host_source,
                host,
                LayoutQuery::ViewportInline,
                rules(base.clone(), 150.0, base.fixed_inline_size(130.0)),
            )
        },
    )
    .unwrap();
    let mut env = LayoutEnvironment::new([100.0, 300.0]);
    env.direction = LayoutDirection::Rtl;
    env.device_scale = 1.5;
    ui.frame_with_environment(&env, constraints(), [200.0; 2])
        .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size[0], 70.0);
    // Paint-only viewport changes must not silently change the query viewport.
    ui.local_frame_in_viewport(constraints(), [180.0; 2])
        .unwrap();
    assert_eq!(ui.layout_environment(), Some(&env));
    assert_eq!(ui.layout_result(host).unwrap().size[0], 70.0);
    ui.frame([200.0; 2]).unwrap();
    env.layout_viewport = [200.0; 2];
    assert_eq!(ui.layout_environment(), Some(&env));
    assert_eq!(ui.layout_result(host).unwrap().size[0], 130.0);
    assert_eq!(ui.layout_result(leaf).unwrap().size[0], 40.0);
    assert_eq!(ui.read(source).unwrap().direction, LayoutDirection::Rtl);
    ui.local_frame([100.0, 200.0]).unwrap();
    assert_eq!(
        ui.layout_environment().unwrap().layout_viewport,
        [100.0, 200.0]
    );
    assert_eq!(ui.layout_result(host).unwrap().size[0], 70.0);
    ui.frame_with_constraints(LayoutConstraints::viewport(200.0, 200.0))
        .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size[0], 130.0);
    let remembered = ui.layout_environment().unwrap().clone();
    ui.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
    assert_eq!(ui.layout_environment(), Some(&remembered));
}

#[test]
fn failed_environment_sources_and_final_geometry_preserve_the_whole_session() {
    let (mut ui, host, leaf, host_source, source) = fixture();
    let env = LayoutEnvironment::new([500.0, 300.0]);
    ui.local_frame_with_environment(&env, constraints(), [200.0; 2])
        .unwrap();
    let before = ui.stats();
    let revision = ui.published_local_frame().unwrap().revision();
    let mut invalid = env.clone();
    invalid.text_scale = 0.0;
    assert!(matches!(
        ui.frame_with_environment(&invalid, constraints(), [200.0; 2]),
        Err(Error::InvalidEnvironment)
    ));
    assert!(matches!(
        ui.frame_with_environment(&env, constraints(), [f32::NAN, 200.0]),
        Err(Error::InvalidViewport)
    ));
    let invalid_style = LayoutStyle::row().fixed_inline_size(f32::NAN);
    assert!(
        ui.transaction(|tx| {
            tx.set(host_source, invalid_style)?;
            Ok(())
        })
        .is_err()
    );
    let mut next = env.clone();
    next.direction = LayoutDirection::Rtl;
    let result = ui.edit_local_frame(
        constraints(),
        [200.0; 2],
        |tx| {
            tx.set(host_source, LayoutStyle::column().fixed_inline_size(130.0))?;
            Ok(())
        },
        |(), edit| {
            edit.layout_environment(&next)?;
            edit.properties().translation(host, [f32::MAX; 2]);
            edit.properties().translation(leaf, [f32::MAX; 2]);
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::InvalidFrame)));
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.layout_environment(), Some(&env));
    assert_eq!(ui.published_local_frame().unwrap().revision(), revision);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 70.0);
    assert_eq!(ui.read(source).unwrap(), &fixed(20.0, 20.0));
    let mut receiver = ui.open_updates(1).unwrap();
    let sender = ui.update_sender(&receiver, host_source).unwrap();
    sender
        .send(LayoutStyle::row().fixed_inline_size(f32::NAN))
        .unwrap();
    assert!(ui.apply_updates(&mut receiver).is_err());
    assert_eq!(
        (
            receiver.stats().unwrap().rejected,
            receiver.stats().unwrap().pending
        ),
        (1, 0)
    );
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.published_local_frame().unwrap().revision(), revision);
}

#[test]
fn automatic_rules_precede_builders_and_reject_later_stale_observations() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let mut first = None;
    for width in [70.0, 130.0, 70.0] {
        let env = LayoutEnvironment::new([width, 200.0]);
        let (commit, _) = ui
            .edit_local_frame(
                constraints(),
                [200.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.layout_environment(&env)?;
                    let branch = edit.reconcile_keys(scope, &[u64::from(width > 100.0)])?[0];
                    let base = LayoutStyle::column().fixed_inline_size(70.0);
                    let host = edit.append_layout(branch, root, base.clone(), 0)?;
                    let source = edit.state(branch, 1, || base.clone())?;
                    edit.responsive_rule(
                        branch,
                        1,
                        source,
                        host,
                        LayoutQuery::ViewportInline,
                        rules(base.clone(), 100.0, base.fixed_inline_size(130.0)),
                    )?;
                    let boundary =
                        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                            .container_type(LayoutContainerType::InlineSize);
                    let slot = edit.append_layout(branch, host, boundary.clone(), 0)?;
                    let slot_source = edit.state(branch, 2, || boundary.clone())?;
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
                    let leaf = edit.inline_constraint_builder(slot, |input, edit| {
                        assert_eq!(input.max, if width > 100.0 { 120.0 } else { 70.0 });
                        edit.append_layout(branch, slot, fixed(400.0, 20.0), 0xff0000ff)
                    })?;
                    Ok((host, source, leaf))
                },
            )
            .unwrap();
        if first.is_none() {
            first = Some(commit.result.value.1);
        }
        assert_eq!(ui.responsive_rules.len(), 2);
        assert_eq!(
            ui.layout_result(commit.result.value.2).unwrap().size[0],
            if width > 100.0 { 120.0 } else { 70.0 }
        );
    }
    assert!(ui.read(first.unwrap()).is_err());
    let before = ui.stats();
    let previous = ui.layout_environment().unwrap().clone();
    let result = ui.edit(
        |_| Ok(()),
        |(), edit| {
            let branch = edit.children(scope)?[0];
            let slot = edit.renders(branch)?[1];
            edit.inline_constraint_builder(slot, |input, _| {
                assert_eq!(input.max, 70.0);
                Ok(())
            })?;
            edit.layout_environment(&LayoutEnvironment::new([130.0, 200.0]))
        },
    );
    assert!(matches!(
        result,
        Err(Error::UnsafeContainerQuery) | Err(Error::UnsafeConstraintBuilder)
    ));
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.layout_environment(), Some(&previous));
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.responsive_rules.is_empty());
}

#[test]
fn explicit_sessions_remain_opt_in_and_can_be_disabled_atomically() {
    let (mut ui, host, leaf, host_source, _) = fixture();
    ui.transaction(|tx| {
        tx.set(host_source, LayoutStyle::column().fixed_inline_size(130.0))?;
        Ok(())
    })
    .unwrap();
    ui.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
    assert_eq!(ui.layout_result(leaf).unwrap().size[0], 20.0);
    assert!(ui.layout_environment().is_none());
    ui.frame_with_environment(
        &LayoutEnvironment::new([80.0; 2]),
        constraints(),
        [200.0; 2],
    )
    .unwrap();
    assert_eq!(ui.layout_result(leaf).unwrap().size[0], 40.0);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.clear_layout_environment();
            Ok(())
        },
    )
    .unwrap();
    assert!(ui.layout_environment().is_none());
    ui.transaction(|tx| {
        tx.set(host_source, LayoutStyle::column().fixed_inline_size(70.0))?;
        Ok(())
    })
    .unwrap();
    ui.frame_in_viewport(constraints(), [200.0; 2]).unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size[0], 70.0);
    assert_eq!(ui.layout_result(leaf).unwrap().size[0], 40.0);
}

#[test]
fn full_constraint_builder_reads_automatic_viewport_selection_before_building_once() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let mut calls = 0;
    let (commit, _) = ui
        .edit_frame(
            constraints(),
            [200.0; 2],
            |_| Ok(()),
            |(), edit| {
                edit.layout_environment(&LayoutEnvironment::new([200.0; 2]))?;
                let base = LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Tight)
                    .container_type(LayoutContainerType::Size)
                    .size(LayoutDimension::Px(60.0), LayoutDimension::Px(40.0));
                let host = edit.append_layout(scope, root, base.clone(), 0)?;
                let source = edit.state(scope, 1, || base.clone())?;
                edit.responsive_rule(
                    scope,
                    1,
                    source,
                    host,
                    LayoutQuery::ViewportInline,
                    rules(
                        base.clone(),
                        100.0,
                        base.size(LayoutDimension::Px(120.0), LayoutDimension::Px(80.0)),
                    ),
                )?;
                edit.constraint_builder(host, |input, edit| {
                    calls += 1;
                    assert_eq!(
                        input.constraints.bounds,
                        BoxConstraints::tight([120.0, 80.0])
                    );
                    edit.append_layout(scope, host, fixed(10.0, 10.0), 0xff0000ff)
                })
            },
        )
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(
        ui.layout_result(commit.result.value).unwrap().size,
        [120.0, 80.0]
    );
}

#[test]
fn active_legacy_reorder_reparent_and_scope_retirement_use_the_atomic_rule_boundary() {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_scope();
    let branches = ui.reconcile_keys(root, &[1, 2]).unwrap().order;
    let h = ui
        .append_render_layout(
            branches[0],
            ui.root_render(),
            LayoutStyle::column().fixed_inline_size(70.0),
            0,
        )
        .unwrap();
    let n = ui
        .append_render_layout(branches[0], h, fixed(20.0, 20.0), 0xff0000ff)
        .unwrap();
    let other = ui
        .append_render_layout(branches[1], ui.root_render(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let source = ui.state(branches[0], 1, || fixed(20.0, 20.0)).unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                branches[0],
                1,
                source,
                n,
                LayoutQuery::ContainerInline(h),
                rules(fixed(20.0, 20.0), 50.0, fixed(40.0, 30.0)),
            )
        },
    )
    .unwrap();
    ui.local_frame_with_environment(
        &LayoutEnvironment::new([200.0; 2]),
        constraints(),
        [200.0; 2],
    )
    .unwrap();
    let before = ui.stats();
    assert!(ui.reparent_render(n, ui.root_render()).is_err());
    assert_eq!(ui.stats(), before);
    assert_eq!(ui.layout_result(n).unwrap().size, [40.0, 30.0]);
    let result = ui.reconcile_keys(root, &[2, 1]).unwrap();
    assert_eq!(result.order, [branches[1], branches[0]]);
    assert_eq!(ui.layout_result(other).unwrap().location[1], 0.0);
    assert_eq!(ui.layout_result(h).unwrap().location[1], 10.0);
    assert!(!ui.sync_render_order().unwrap());
    let mut inbox = ui.open_updates(1).unwrap();
    let sender = ui.update_sender(&inbox, source).unwrap();
    sender.send(fixed(10.0, 10.0)).unwrap();
    ui.reconcile_keys(root, &[2]).unwrap();
    assert!(ui.responsive_rules.is_empty());
    assert!(ui.read(source).is_err());
    assert_eq!(ui.layout_result(other).unwrap().location[1], 0.0);
    let batch = ui.apply_updates(&mut inbox).unwrap();
    assert_eq!((batch.applied, batch.retired), (0, 1));
    assert!(batch.commit.is_none());
    // Ordinary construction/binding also publishes a complete local frame in
    // the captured session, rather than leaving reactive sinks ahead of layout.
    let appended = ui
        .append_render_layout(branches[1], ui.root_render(), fixed(12.0, 8.0), 0xff0000ff)
        .unwrap();
    assert_eq!(ui.layout_result(appended).unwrap().location[1], 10.0);
    let style = ui.state(root, 99, || fixed(30.0, 16.0)).unwrap();
    ui.bind(style, Property::layout_style(appended)).unwrap();
    assert_eq!(ui.layout_result(appended).unwrap().size, [30.0, 16.0]);
    assert_eq!(
        ui.published_local_frame().unwrap().hit_test(1.0, 11.0),
        Some(appended)
    );
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_automatic_source_and_inbox_updates_match_fresh_full_pixels() {
    let (mut ui, _, _, host_source, _) = fixture();
    ui.local_frame_with_environment(
        &LayoutEnvironment::new([500.0, 300.0]),
        constraints(),
        [200.0; 2],
    )
    .unwrap();
    let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!(
        "automatic environment adapter: {:?}",
        renderer.adapter_info()
    );
    let mut receiver = ui.open_updates(1).unwrap();
    let sender = ui.update_sender(&receiver, host_source).unwrap();
    for (i, width) in [70.0, 130.0, 70.0, 130.0].into_iter().enumerate() {
        if i % 2 == 0 {
            ui.transaction(|tx| {
                tx.set(host_source, LayoutStyle::column().fixed_inline_size(width))?;
                Ok(())
            })
            .unwrap();
        } else {
            sender
                .send(LayoutStyle::column().fixed_inline_size(width))
                .unwrap();
            ui.apply_updates(&mut receiver).unwrap();
        }
        renderer
            .submit_local(ui.published_local_frame().unwrap(), [200, 200], 1.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            renderer.image().unwrap().as_raw(),
            renderer
                .reference_image(
                    &reference(width, width >= 100.0, LayoutDirection::Ltr),
                    [200, 200],
                    1.0
                )
                .unwrap()
                .as_raw()
        );
    }
    renderer.finish().unwrap();
}
