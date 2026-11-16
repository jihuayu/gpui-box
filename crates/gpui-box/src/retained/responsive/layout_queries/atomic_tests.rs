use super::*;
use std::{cell::RefCell, rc::Rc};

fn setup() -> LogicalUi {
    LogicalUi::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
        BackendCapabilities::default(),
    )
    .unwrap()
}

fn rules(base: LayoutStyle, threshold: f32, wider: LayoutStyle) -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(base, vec![(threshold, wider)]).unwrap()
}

#[test]
fn later_computation_failure_rolls_back_viewport_query_sources_frames_stats_and_effects() {
    for local in [false, true] {
        let mut ui = setup();
        let scope = ui.root_scope();
        let narrow = LayoutStyle::column().fixed_inline_size(50.0);
        let wide = LayoutStyle::column().fixed_inline_size(80.0);
        let host = ui
            .append_render_layout(scope, ui.root_render(), narrow.clone(), 0)
            .unwrap();
        let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
        let leaf = ui
            .append_render_layout(scope, host, base.clone(), 0xff0000ff)
            .unwrap();
        let width = ui.state(scope, 0, || narrow.clone()).unwrap();
        let style = ui.state(scope, 1, || base.clone()).unwrap();
        let color = ui
            .computed(scope, 2, &[style.dependency()], move |input| {
                if input.read(style)?.flex_direction == LayoutFlexDirection::Column {
                    return Err(Error::InvalidPrimitive);
                }
                Ok(0xff0000ffu32)
            })
            .unwrap();
        ui.bind(color, Property::color(leaf)).unwrap();
        let observations = Rc::new(RefCell::new(vec![]));
        let observed = observations.clone();
        ui.effect(
            scope,
            3,
            &[width.dependency(), style.dependency()],
            move |input| {
                observed
                    .borrow_mut()
                    .push(input.read(width).unwrap().size.width);
                None
            },
        )
        .unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind(&mut ui, width, host, rules(narrow.clone(), 150.0, wide))
            .unwrap();
        responsive
            .bind_container(
                &mut ui,
                style,
                leaf,
                host,
                rules(
                    base.clone(),
                    70.0,
                    base.clone().flex_direction(LayoutFlexDirection::Column),
                ),
            )
            .unwrap();
        let environment = LayoutEnvironment::new([100.0, 100.0]);
        let before = responsive.local_frame(&mut ui, &environment).unwrap();
        let published = ui.published_frame().unwrap().clone();
        let stats = ui.stats();
        let effects = observations.borrow().clone();
        let environment = LayoutEnvironment::new([200.0, 100.0]);
        let error = if local {
            responsive.local_frame(&mut ui, &environment).err()
        } else {
            responsive.frame(&mut ui, &environment).err()
        };
        assert_eq!(error, Some(Error::InvalidPrimitive));
        assert_eq!(ui.read(width).unwrap(), &narrow);
        assert_eq!(ui.read(style).unwrap(), &base);
        assert_eq!(ui.stats(), stats);
        assert_eq!(*observations.borrow(), effects);
        let after = ui.published_frame().unwrap();
        assert_eq!(after.revision, published.revision);
        assert!(Arc::ptr_eq(&after.items, &published.items));
        assert_eq!(ui.layout_result(host).unwrap().size[0], 50.0);
        assert_eq!(
            before.frame.resolve_geometry(leaf).unwrap().bounds.width,
            10.0
        );
        let recovered = responsive
            .local_frame(&mut ui, &LayoutEnvironment::new([100.0, 100.0]))
            .unwrap();
        assert_eq!(recovered.layout_runs, 0);
        assert_eq!(recovered.frame.revision(), before.frame.revision());
    }
}

#[test]
fn nested_rules_publish_final_values_once_and_effects_never_see_intermediate_sources() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let narrow = LayoutStyle::column().fixed_inline_size(50.0);
    let wide = LayoutStyle::column().fixed_inline_size(80.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), narrow.clone(), 0)
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
    let leaf = ui
        .append_render_layout(scope, host, base.clone(), 0xff0000ff)
        .unwrap();
    let width = ui.state(scope, 0, || narrow.clone()).unwrap();
    let style = ui.state(scope, 1, || base.clone()).unwrap();
    let observed = Rc::new(RefCell::new(vec![]));
    let log = observed.clone();
    ui.effect(
        scope,
        2,
        &[width.dependency(), style.dependency()],
        move |input| {
            log.borrow_mut().push((
                input.read(width).unwrap().size.width,
                input.read(style).unwrap().flex_direction,
            ));
            None
        },
    )
    .unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            style,
            leaf,
            host,
            rules(
                base.clone(),
                70.0,
                base.flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    responsive
        .bind(&mut ui, width, host, rules(narrow, 150.0, wide))
        .unwrap();
    let environment = LayoutEnvironment::new([100.0, 100.0]);
    responsive.frame(&mut ui, &environment).unwrap();
    let before = ui.published_frame().unwrap().revision;
    let stats = ui.stats();
    let frame = responsive
        .local_frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
        .unwrap();
    assert_eq!(frame.changed_styles, 2);
    assert_eq!(ui.stats().transactions - stats.transactions, 1);
    assert_eq!(ui.stats().frame_requests - stats.frame_requests, 1);
    assert_eq!(ui.stats().effect_runs - stats.effect_runs, 1);
    assert_eq!(
        &*observed.borrow(),
        &[
            (LayoutDimension::Px(50.0), LayoutFlexDirection::Row),
            (LayoutDimension::Px(80.0), LayoutFlexDirection::Column),
        ]
    );
    // One property transaction plus one changed layout/paint environment.
    assert_eq!(frame.frame.revision(), before + 2);
    assert_eq!(ui.published_frame().unwrap().base_revision, Some(before));
    assert_eq!(
        frame.frame.resolve().base_revision,
        None,
        "local compatibility is a full snapshot"
    );
}

#[test]
fn public_local_queries_keep_sparse_property_output_and_separate_layout_from_viewport() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let style = LayoutStyle::column().fixed_inline_size(40.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), style, 0)
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(20.0));
    let leaf = ui
        .append_render_layout(scope, host, base.clone(), 0xff0000ff)
        .unwrap();
    let source = ui.state(scope, 0, || base.clone()).unwrap();
    let translation = ui.state(scope, 1, || [0.0f32; 2]).unwrap();
    ui.bind(translation, Property::translation(leaf)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            source,
            leaf,
            host,
            rules(
                base.clone(),
                100.0,
                base.flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Definite(100.0),
        LayoutAvailableSpace::Unbounded,
    );
    let env = LayoutEnvironment::new([500.0, 400.0]);
    let before = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [10.0, 50.0])
        .unwrap();
    assert_eq!(before.frame.hit_test(5.0, 5.0), Some(leaf));
    assert_eq!(before.frame.hit_test(15.0, 5.0), None);
    ui.transaction(|tx| {
        tx.set(translation, [2.0, 0.0])?;
        Ok(())
    })
    .unwrap();
    let moved = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [10.0, 50.0])
        .unwrap();
    assert_eq!(moved.layout_runs, 0);
    assert!(!moved.frame.stats().full_publication);
    assert_eq!(moved.frame.stats().changed_nodes, 1);
    assert_eq!(before.frame.resolve_geometry(leaf).unwrap().bounds.x, 0.0);
    assert_eq!(moved.frame.resolve_geometry(leaf).unwrap().bounds.x, 2.0);
    let idle = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [10.0, 50.0])
        .unwrap();
    assert_eq!(idle.layout_runs, 0);
    assert_eq!(idle.layout_stats, LayoutStats::default());
    let resized = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [30.0, 50.0])
        .unwrap();
    assert_eq!(resized.layout_runs, 0);
    assert_eq!(resized.frame.hit_test(15.0, 5.0), Some(leaf));
    assert_eq!(ui.layout_result(ui.root_render()).unwrap().size[0], 100.0);
}

#[test]
fn readonly_sources_and_replaced_rule_writers_are_rejected() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let base = LayoutStyle::row();
    let target = ui
        .append_render_layout(scope, ui.root_render(), base.clone(), 0)
        .unwrap();
    let source = ui.state(scope, 0, || base.clone()).unwrap();
    let computed = ui
        .computed(scope, 1, &[source.dependency()], move |input| {
            Ok(input.read(source)?.clone())
        })
        .unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    assert_eq!(
        responsive.bind(
            &mut ui,
            computed,
            target,
            rules(base.clone(), 10.0, LayoutStyle::column())
        ),
        Err(Error::ReadonlySignal)
    );
    responsive
        .bind(
            &mut ui,
            source,
            target,
            rules(base.clone(), 10.0, LayoutStyle::column()),
        )
        .unwrap();
    // BindingId is an opaque public handle; locate this known sole binding by
    // reusing the handle issued to the rule rather than inspecting internals.
    let id = responsive.bindings[0].binding;
    ui.edit(|_| Ok(()), |(), draft| draft.unbind(id)).unwrap();
    let other = ui.state(scope, 2, || base).unwrap();
    ui.bind(other, Property::layout_style(target)).unwrap();
    let stats = ui.stats();
    assert_eq!(
        responsive
            .frame(&mut ui, &LayoutEnvironment::new([20.0, 20.0]))
            .err(),
        Some(Error::InvalidBinding)
    );
    assert_eq!(ui.stats(), stats);
}

#[test]
fn computed_layout_sinks_join_query_dependency_order_even_when_registered_late() {
    let mut ui = LogicalUi::new_layout(
        LayoutStyle::row()
            .fixed_inline_size(300.0)
            .size(LayoutDimension::Px(300.0), LayoutDimension::Px(100.0)),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let small = LayoutStyle::row()
        .size(LayoutDimension::Px(20.0), LayoutDimension::Px(10.0))
        .flex_shrink(0.0);
    let sibling = ui
        .append_render_layout(scope, root, small.clone(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::column()
                .container_type(LayoutContainerType::Size)
                .flex_grow(1.0),
            0,
        )
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
    let target = ui
        .append_render_layout(scope, host, base.clone(), 1)
        .unwrap();
    let marker = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::row().size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0)),
            0,
        )
        .unwrap();
    let marker_style = LayoutStyle::row().size(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0));
    let producer = ui.state(scope, 0, || marker_style.clone()).unwrap();
    let consumer = ui.state(scope, 1, || base.clone()).unwrap();
    let computed = ui
        .computed(scope, 2, &[producer.dependency()], move |input| {
            Ok(
                if input.read(producer)?.flex_direction == LayoutFlexDirection::Column {
                    LayoutStyle::row()
                        .size(LayoutDimension::Px(100.0), LayoutDimension::Px(10.0))
                        .flex_shrink(0.0)
                } else {
                    small.clone()
                },
            )
        })
        .unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind_container(
            &mut ui,
            consumer,
            target,
            host,
            rules(
                base.clone(),
                250.0,
                base.flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            producer,
            marker,
            root,
            rules(
                marker_style.clone(),
                250.0,
                marker_style.flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    // This dependency did not exist when the rules were registered.
    ui.bind(computed, Property::layout_style(sibling)).unwrap();
    let frame = responsive
        .frame(&mut ui, &LayoutEnvironment::new([300.0, 100.0]))
        .unwrap();
    assert_eq!(frame.changed_styles, 1);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 200.0);
    assert_eq!(
        ui.read(consumer).unwrap().flex_direction,
        LayoutFlexDirection::Row
    );
    assert_eq!(ui.published_frame().unwrap().revision, frame.frame.revision);
}

#[test]
fn derived_self_feedback_and_unsafe_ancestor_changes_reject_before_publication() {
    for unsafe_style in [false, true] {
        let mut ui = setup();
        let scope = ui.root_scope();
        let host_style = LayoutStyle::column().fixed_inline_size(80.0);
        let host = ui
            .append_render_layout(scope, ui.root_render(), host_style.clone(), 0)
            .unwrap();
        let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
        let leaf = ui
            .append_render_layout(scope, host, base.clone(), 1)
            .unwrap();
        let source = ui.state(scope, 0, || base.clone()).unwrap();
        let computed = ui
            .computed(scope, 1, &[source.dependency()], move |input| {
                let mut style = host_style.clone();
                if unsafe_style && input.read(source)?.flex_direction == LayoutFlexDirection::Column
                {
                    style = LayoutStyle::column();
                }
                Ok(style)
            })
            .unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind_container(
                &mut ui,
                source,
                leaf,
                host,
                rules(
                    base.clone(),
                    50.0,
                    base.flex_direction(LayoutFlexDirection::Column),
                ),
            )
            .unwrap();
        ui.bind(computed, Property::layout_style(host)).unwrap();
        let before = ui.frame([100.0, 100.0]).unwrap();
        let stats = ui.stats();
        assert_eq!(
            responsive
                .frame(&mut ui, &LayoutEnvironment::new([100.0, 100.0]))
                .err(),
            Some(Error::UnsafeContainerQuery)
        );
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);
        assert_eq!(
            ui.read(source).unwrap().flex_direction,
            LayoutFlexDirection::Row
        );
    }
}

#[test]
fn invalid_computed_sink_and_final_revision_exhaustion_leave_the_owner_unchanged() {
    for exhausted in [false, true] {
        let mut ui = setup();
        let scope = ui.root_scope();
        let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
        let target = ui
            .append_render_layout(scope, ui.root_render(), base.clone(), 1)
            .unwrap();
        let source = ui.state(scope, 0, || base.clone()).unwrap();
        if !exhausted {
            let opacity = ui
                .computed(scope, 1, &[source.dependency()], move |input| {
                    Ok(
                        if input.read(source)?.flex_direction == LayoutFlexDirection::Column {
                            f32::NAN
                        } else {
                            1.0f32
                        },
                    )
                })
                .unwrap();
            ui.bind(opacity, Property::opacity(target)).unwrap();
        }
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind(
                &mut ui,
                source,
                target,
                rules(
                    base.clone(),
                    150.0,
                    base.clone().flex_direction(LayoutFlexDirection::Column),
                ),
            )
            .unwrap();
        let before = responsive
            .frame(&mut ui, &LayoutEnvironment::new([100.0, 100.0]))
            .unwrap()
            .frame;
        if exhausted {
            ui.reactive_mut().responsive_test_tree().revision = u64::MAX - 1;
        }
        let revision = ui.reactive().tree().revision;
        let stats = ui.stats();
        let error = responsive
            .frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
            .err();
        assert_eq!(
            error,
            Some(if exhausted {
                Error::RevisionExhausted
            } else {
                Error::InvalidOpacity
            })
        );
        assert_eq!(ui.reactive().tree().revision, revision);
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);
        assert!(Arc::ptr_eq(
            &ui.published_frame().unwrap().items,
            &before.items
        ));
        assert_eq!(ui.read(source).unwrap(), &base);
        assert_eq!(ui.stats(), stats);
    }
}

#[test]
fn final_resolved_and_local_queries_match_full_geometry_pixels_and_owned_snapshots() {
    use crate::retained::{pacing, software};
    fn scene() -> (LogicalUi, LayoutResponsiveBindings, NodeId) {
        let mut ui = setup();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(
                scope,
                ui.root_render(),
                LayoutStyle::column().fixed_inline_size(40.0),
                0,
            )
            .unwrap();
        let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
        let target = ui
            .append_render_layout(scope, host, base.clone(), 0xff0000ff)
            .unwrap();
        let source = ui.state(scope, 0, || base.clone()).unwrap();
        let mut responsive = LayoutResponsiveBindings::new();
        responsive
            .bind_container(
                &mut ui,
                source,
                target,
                host,
                rules(
                    base,
                    30.0,
                    LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(20.0)),
                ),
            )
            .unwrap();
        (ui, responsive, target)
    }
    fn pixels(frame: FramePacket) -> Vec<u8> {
        let mut job = software::SoftwareJob::new(
            frame,
            software::SoftwareOptions {
                size: [100, 100],
                scale: 1.0,
                background: 0,
                effects: pacing::SoftwareEffects::Exact,
                max_bytes: 4 * 1024 * 1024,
            },
        )
        .unwrap();
        while !job.step(4096).unwrap() {}
        job.into_frame().unwrap().pixels().to_vec()
    }
    let (mut full, mut full_rules, a) = scene();
    let (mut local, mut local_rules, b) = scene();
    full.reactive_mut()
        .responsive_test_tree()
        .set_frame_build_policy(FrameBuildPolicy::Full);
    let env = LayoutEnvironment::new([100.0, 100.0]);
    let before = local.frame([100.0, 100.0]).unwrap();
    let resolved = full_rules.frame(&mut full, &env).unwrap();
    let placed = local_rules.local_frame(&mut local, &env).unwrap();
    assert_eq!(
        full.layout_result(a).unwrap(),
        local.layout_result(b).unwrap()
    );
    assert_eq!(pixels(resolved.frame), pixels(placed.frame.resolve()));
    let after_pixels = pixels(placed.frame.resolve());
    assert_eq!(
        &after_pixels[(15 * 100 + 15) * 4..(15 * 100 + 15) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(before.items[2].bounds.width, 10.0);
    assert_eq!(placed.frame.resolve_geometry(b).unwrap().bounds.width, 20.0);
    assert_eq!(placed.frame.hit_test(15.0, 15.0), Some(b));
}

#[test]
fn derived_values_restored_across_query_stages_do_not_publish_transient_materials_or_effects() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(
            scope,
            ui.root_render(),
            LayoutStyle::column().fixed_inline_size(80.0),
            0,
        )
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
    let a = ui
        .append_render_layout(scope, host, base.clone(), 0)
        .unwrap();
    let b = ui
        .append_render_layout(scope, host, base.clone(), 0)
        .unwrap();
    let first = ui.state(scope, 0, || base.clone()).unwrap();
    let second = ui.state(scope, 1, || base.clone()).unwrap();
    let material = ui
        .computed(
            scope,
            2,
            &[first.dependency(), second.dependency()],
            move |input| {
                Ok(
                    if input.read(first)?.flex_direction == input.read(second)?.flex_direction {
                        0xff0000ffu32
                    } else {
                        0x00ff00ffu32
                    },
                )
            },
        )
        .unwrap();
    ui.bind(material, Property::color(a)).unwrap();
    let observed = Rc::new(RefCell::new(vec![]));
    let log = observed.clone();
    ui.effect(scope, 3, &[material.dependency()], move |input| {
        log.borrow_mut().push(*input.read(material).unwrap());
        None
    })
    .unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            first,
            a,
            rules(
                base.clone(),
                150.0,
                base.clone().flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            second,
            b,
            host,
            rules(
                base.clone(),
                70.0,
                base.clone().flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    // Initial frame is prepared without the resolver so both sources are Row.
    let before = ui.frame([200.0, 100.0]).unwrap();
    let stats = ui.stats();
    let result = responsive
        .frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
        .unwrap();
    assert_eq!(result.changed_styles, 2);
    assert_eq!(ui.stats().changed_sources - stats.changed_sources, 2);
    assert_eq!(ui.stats().effect_runs - stats.effect_runs, 0);
    assert_eq!(*observed.borrow(), [0xff0000ff]);
    assert_eq!(*ui.read(material).unwrap(), 0xff0000ff);
    assert_eq!(result.frame.items[2].fill_color, 0xff0000ff);
    assert!(Arc::ptr_eq(
        &result.frame.items[2].commands,
        &before.items[2].commands
    ));
    assert_eq!(result.frame.base_revision, Some(before.revision));
}

#[test]
fn viewport_derived_invalid_host_proof_rolls_back_without_query_publication() {
    let mut ui = setup();
    let scope = ui.root_scope();
    let host_style = LayoutStyle::column().fixed_inline_size(80.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), host_style.clone(), 0)
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
    let leaf = ui
        .append_render_layout(scope, host, base.clone(), 1)
        .unwrap();
    let marker = ui
        .append_render_layout(scope, ui.root_render(), base.clone(), 0)
        .unwrap();
    let viewport = ui.state(scope, 0, || base.clone()).unwrap();
    let query = ui.state(scope, 1, || base.clone()).unwrap();
    let computed = ui
        .computed(scope, 2, &[viewport.dependency()], move |input| {
            Ok(
                if input.read(viewport)?.flex_direction == LayoutFlexDirection::Column {
                    LayoutStyle::column()
                } else {
                    host_style.clone()
                },
            )
        })
        .unwrap();
    ui.bind(computed, Property::layout_style(host)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(
            &mut ui,
            viewport,
            marker,
            rules(
                base.clone(),
                150.0,
                base.clone().flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            query,
            leaf,
            host,
            rules(
                base.clone(),
                50.0,
                base.clone().flex_direction(LayoutFlexDirection::Column),
            ),
        )
        .unwrap();
    let before = ui.frame([100.0, 100.0]).unwrap();
    let stats = ui.stats();
    assert_eq!(
        responsive
            .frame(&mut ui, &LayoutEnvironment::new([200.0, 100.0]))
            .err(),
        Some(Error::UnsafeContainerQuery)
    );
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.read(viewport).unwrap(), &base);
    assert_eq!(ui.read(query).unwrap(), &base);
    assert_eq!(ui.layout_result(host).unwrap().size[0], 80.0);
    assert!(Arc::ptr_eq(
        &ui.published_frame().unwrap().items,
        &before.items
    ));
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_local_responsive_updates_match_fresh_full_pixels_and_skip_idle_submission() {
    use crate::retained::wgpu::WgpuFrameRenderer;
    let mut ui = setup();
    let scope = ui.root_scope();
    let narrow = LayoutStyle::column().fixed_inline_size(20.0);
    let wide = LayoutStyle::column().fixed_inline_size(40.0);
    let host = ui
        .append_render_layout(scope, ui.root_render(), narrow.clone(), 0)
        .unwrap();
    let base = LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(10.0));
    let leaf = ui
        .append_render_layout(scope, host, base.clone(), 0xff0000ff)
        .unwrap();
    let width = ui.state(scope, 0, || narrow.clone()).unwrap();
    let style = ui.state(scope, 1, || base.clone()).unwrap();
    let translate = ui.state(scope, 2, || [0.0f32; 2]).unwrap();
    ui.bind(translate, Property::translation(leaf)).unwrap();
    let mut responsive = LayoutResponsiveBindings::new();
    responsive
        .bind(&mut ui, width, host, rules(narrow, 150.0, wide))
        .unwrap();
    responsive
        .bind_container(
            &mut ui,
            style,
            leaf,
            host,
            rules(
                base,
                30.0,
                LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Px(20.0)),
            ),
        )
        .unwrap();
    let mut renderer = WgpuFrameRenderer::new(2).unwrap();
    eprintln!("responsive WGPU adapter: {:?}", renderer.adapter_info());
    let constraints = LayoutConstraints::new(
        LayoutAvailableSpace::Definite(100.0),
        LayoutAvailableSpace::Unbounded,
    );
    let mut old = None;
    for width in [100.0, 200.0] {
        let frame = responsive
            .local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([width, 100.0]),
                constraints,
                [100.0, 100.0],
            )
            .unwrap()
            .frame;
        assert!(
            renderer
                .submit_local(&frame, [100, 100], 1.0)
                .unwrap()
                .is_some()
        );
        let actual = renderer.image().unwrap();
        let expected = renderer
            .reference_image(&frame.resolve(), [100, 100], 1.0)
            .unwrap();
        assert_eq!(actual.as_raw(), expected.as_raw());
        if let Some(old) = old {
            assert_eq!(old, 10.0);
            assert_eq!(frame.resolve_geometry(leaf).unwrap().bounds.width, 20.0);
            assert_eq!(
                &actual.as_raw()[(15 * 100 + 15) * 4..(15 * 100 + 15) * 4 + 4],
                &[255, 0, 0, 255]
            );
        }
        old = Some(frame.resolve_geometry(leaf).unwrap().bounds.width);
    }
    ui.transaction(|tx| {
        tx.set(translate, [5.0, 0.0])?;
        Ok(())
    })
    .unwrap();
    let env = LayoutEnvironment::new([200.0, 100.0]);
    let moved = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [100.0, 100.0])
        .unwrap();
    assert_eq!(moved.layout_runs, 0);
    assert!(!moved.frame.stats().full_publication);
    renderer
        .submit_local(&moved.frame, [100, 100], 1.0)
        .unwrap();
    assert_eq!(
        renderer.image().unwrap().as_raw(),
        renderer
            .reference_image(&moved.frame.resolve(), [100, 100], 1.0)
            .unwrap()
            .as_raw()
    );
    let idle = responsive
        .local_frame_in_viewport(&mut ui, &env, constraints, [100.0, 100.0])
        .unwrap();
    assert_eq!(idle.layout_runs, 0);
    assert!(
        renderer
            .submit_local(&idle.frame, [100, 100], 1.0)
            .unwrap()
            .is_none()
    );
    renderer.finish().unwrap();
}
