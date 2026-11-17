use super::*;

fn root() -> LayoutStyle {
    LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn host() -> LayoutStyle {
    LayoutStyle::column()
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
        .overflow(LayoutOverflow::Visible, LayoutOverflow::Scroll)
        .scrollbar_width(6.0)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn rules() -> LayoutBreakpointStyles {
    LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(100.0, fixed(400.0, 60.0))]).unwrap()
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
    let mut tree = UiTree::new_layout(root(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let node = tree.append_layout(tree.root(), host(), 0x0000ffff).unwrap();
    tree.append_layout(
        node,
        fixed(400.0, if w - 12.0 < 100.0 { 20.0 } else { 60.0 }),
        0xff0000ff,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}

#[test]
fn ordinary_query_uses_real_content_width_with_auto_height_and_zero_idle_layout() {
    for local in [false, true] {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let node = ui
            .append_render_layout(scope, ui.root_render(), host(), 0x0000ffff)
            .unwrap();
        let target = ui
            .append_render_layout(scope, node, fixed(400.0, 20.0), 0xff0000ff)
            .unwrap();
        let footer = ui
            .append_render_layout(scope, ui.root_render(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
        let mut bindings = LayoutResponsiveBindings::new();
        bindings
            .bind_container(&mut ui, source, target, node, rules())
            .unwrap();
        let mut old = None;
        for width in [80.0, 111.0, 112.0, 140.0, 80.0] {
            let environment = LayoutEnvironment::new([500.0, 180.0]);
            let frame = if local {
                bindings
                    .local_frame_in_viewport(&mut ui, &environment, constraints(width), [180.0; 2])
                    .unwrap()
                    .frame
                    .resolve()
            } else {
                bindings
                    .frame_in_viewport(&mut ui, &environment, constraints(width), [180.0; 2])
                    .unwrap()
                    .frame
            };
            let height = if width < 112.0 { 20.0 } else { 60.0 };
            assert_eq!(ui.layout_result(node).unwrap().size, [width, height + 6.0]);
            assert_eq!(
                ui.layout_result(node).unwrap().content_size,
                [width - 12.0, height]
            );
            assert_eq!(
                ui.layout_result(footer).unwrap().location,
                [0.0, height + 6.0]
            );
            assert_eq!(pixels(frame.clone()), pixels(reference(width)));
            let idle = bindings
                .local_frame_in_viewport(&mut ui, &environment, constraints(width), [180.0; 2])
                .unwrap();
            assert_eq!((idle.layout_runs, idle.changed_styles), (0, 0));
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn ordinary_nested_query_waits_for_ancestor_producer_registered_later() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let outer = ui
        .append_render_layout(scope, ui.root_render(), LayoutStyle::column(), 0)
        .unwrap();
    let inner_style =
        LayoutStyle::column().size(LayoutDimension::Percent(0.5), LayoutDimension::Auto);
    let inner = ui
        .append_render_layout(scope, outer, inner_style.clone(), 0)
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, inner, fixed(400.0, 20.0), 1)
        .unwrap();
    let leaf_source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let inner_source = ui.state(scope, 1, || inner_style.clone()).unwrap();
    let mut bindings = LayoutResponsiveBindings::new();
    bindings
        .bind_container(
            &mut ui,
            leaf_source,
            leaf,
            inner,
            LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                .unwrap(),
        )
        .unwrap();
    bindings
        .bind_container(
            &mut ui,
            inner_source,
            inner,
            outer,
            LayoutBreakpointStyles::new(inner_style, vec![(120.0, LayoutStyle::column())]).unwrap(),
        )
        .unwrap();
    for (w, iw, h) in [
        (100.0, 50.0, 20.0),
        (120.0, 120.0, 60.0),
        (100.0, 50.0, 20.0),
    ] {
        bindings
            .frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([180.0; 2]),
                constraints(w),
                [180.0; 2],
            )
            .unwrap();
        assert_eq!(ui.layout_result(inner).unwrap().size, [iw, h]);
        assert_eq!(ui.layout_result(outer).unwrap().size, [w, h]);
    }
}

#[test]
fn ordinary_query_requires_live_external_anchor_and_safe_ancestor_alternatives() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root_node = ui.root_render();
    let node = ui
        .append_render_layout(scope, root_node, host(), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, node, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let mut bindings = LayoutResponsiveBindings::new();
    bindings
        .bind_container(&mut ui, source, target, node, rules())
        .unwrap();
    bindings
        .frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([180.0; 2]),
            constraints(80.0),
            [180.0; 2],
        )
        .unwrap();
    let before = ui.read(source).unwrap().clone();
    let stats = ui.stats();
    let revision = ui.published_frame().unwrap().revision;
    for available in [
        LayoutAvailableSpace::Unbounded,
        LayoutAvailableSpace::MinContent,
        LayoutAvailableSpace::MaxContent,
    ] {
        assert!(matches!(
            bindings.local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([180.0; 2]),
                LayoutConstraints::new(available, LayoutAvailableSpace::Unbounded),
                [180.0; 2]
            ),
            Err(Error::UnsafeContainerQuery)
        ));
        assert_eq!(ui.read(source).unwrap(), &before);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, revision);
    }
    let root_source = ui.state(scope, 1, root).unwrap();
    assert!(matches!(
        bindings.bind(
            &mut ui,
            root_source,
            root_node,
            LayoutBreakpointStyles::new(
                root(),
                vec![(100.0, root().flex_wrap(LayoutFlexWrap::Wrap))]
            )
            .unwrap()
        ),
        Err(Error::UnsafeContainerQuery)
    ));
    assert_eq!(ui.reactive().binding_count(), 1);
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().layout_style(
                root_node,
                root().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
            );
            Ok(())
        },
    )
    .unwrap();
    bindings
        .frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([180.0; 2]),
            LayoutConstraints::new(
                LayoutAvailableSpace::Unbounded,
                LayoutAvailableSpace::Unbounded,
            ),
            [180.0; 2],
        )
        .unwrap();
    assert_eq!(ui.layout_result(node).unwrap().size[0], 80.0);
}

#[test]
fn scope_owned_ordinary_query_and_inline_builder_share_one_atomic_session() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let root_node = ui.root_render();
    let scope = ui.root_scope();
    let node = ui
        .append_render_layout(scope, root_node, host(), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, node, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let env = LayoutEnvironment::new([180.0; 2]);
    ui.edit_frame(
        constraints(140.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive_rule(
                scope,
                0,
                source,
                target,
                LayoutQuery::ContainerInline(node),
                rules(),
            )?;
            edit.layout_environment(&env)?;
            edit.inline_allocation_builder(node, |input, edit| {
                assert_eq!(input.known_width, Some(140.0));
                assert_eq!(edit.read(source)?.size.height, LayoutDimension::Px(60.0));
                Ok(())
            })
        },
    )
    .unwrap();
    let width_source = ui.state(scope, 1, || host()).unwrap();
    ui.bind(width_source, Property::layout_style(node)).unwrap();
    ui.transaction(|tx| {
        tx.set(
            width_source,
            host().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(ui.read(source).unwrap(), &fixed(400.0, 20.0));
    assert_eq!(ui.layout_result(node).unwrap().size, [80.0, 26.0]);
    let stats = ui.stats();
    let revision = ui.published_frame().unwrap().revision;
    let bad = ui.edit_local_frame(
        constraints(140.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.responsive_registered(&env)?;
            edit.properties()
                .layout_style(root_node, root().flex_direction(LayoutFlexDirection::Row));
            Ok(())
        },
    );
    assert!(matches!(bad, Err(Error::UnsafeContainerQuery)));
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.published_frame().unwrap().revision, revision);
}

#[derive(Debug)]
struct WidthPhase;
impl ParentInlineLayout for WidthPhase {
    fn allocate(
        &self,
        input: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        let width = input.known_width.unwrap_or(match input.available {
            LayoutAvailableSpace::Definite(width) => width,
            _ => 40.0,
        }) * 0.5;
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
        let mut placements = vec![];
        for child in 0..children.len() {
            let slot = ChildLayoutInput::default();
            let measured = children.measure_baselines(child, slot)?;
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, y],
            });
            y += measured.size[1];
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(0.0),
                input.known_size.height.unwrap_or(y),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}

#[test]
fn ordinary_query_crosses_tight_boxes_and_enforced_custom_widths_without_containment() {
    for custom in [false, true] {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root_node = ui.root_render();
        let parent = ui
            .append_render_layout(
                scope,
                root_node,
                if custom {
                    LayoutStyle::column()
                } else {
                    LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                },
                0,
            )
            .unwrap();
        if custom {
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties().parent_layout(
                        parent,
                        Some(ParentLayoutSnapshot::new(WidthPhase).with_inline_layout(WidthPhase)),
                    );
                    Ok(())
                },
            )
            .unwrap();
        }
        let host = ui
            .append_render_layout(scope, parent, LayoutStyle::column(), 0)
            .unwrap();
        let target = ui
            .append_render_layout(scope, host, fixed(400.0, 20.0), 1)
            .unwrap();
        let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
        let mut bindings = LayoutResponsiveBindings::new();
        bindings
            .bind_container(
                &mut ui,
                source,
                target,
                host,
                LayoutBreakpointStyles::new(fixed(400.0, 20.0), vec![(90.0, fixed(400.0, 60.0))])
                    .unwrap(),
            )
            .unwrap();
        for w in [100.0, 200.0, 100.0] {
            bindings
                .local_frame_in_viewport(
                    &mut ui,
                    &LayoutEnvironment::new([180.0; 2]),
                    constraints(w),
                    [180.0; 2],
                )
                .unwrap();
            let width = if custom { w * 0.5 } else { w };
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [width, if width < 90.0 { 20.0 } else { 60.0 }]
            );
        }
    }
}

#[test]
fn ordinary_query_rejects_computed_host_feedback_even_when_output_width_is_equal() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let node = ui
        .append_render_layout(scope, ui.root_render(), host(), 0)
        .unwrap();
    let target = ui
        .append_render_layout(scope, node, fixed(400.0, 20.0), 1)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let mut bindings = LayoutResponsiveBindings::new();
    bindings
        .bind_container(&mut ui, source, target, node, rules())
        .unwrap();
    let feedback = ui
        .computed(scope, 1, &[source.dependency()], move |input| {
            let _ = input.read(source)?;
            Ok(host())
        })
        .unwrap();
    ui.bind(feedback, Property::layout_style(node)).unwrap();
    let before = ui.frame_in_viewport(constraints(80.0), [180.0; 2]).unwrap();
    let stats = ui.stats();
    assert!(matches!(
        bindings.frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([180.0; 2]),
            constraints(140.0),
            [180.0; 2]
        ),
        Err(Error::UnsafeContainerQuery)
    ));
    assert_eq!(ui.stats(), stats);
    assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    assert_eq!(ui.read(source).unwrap(), &fixed(400.0, 20.0));
}

#[test]
fn standalone_final_query_observations_reject_changed_width_or_equal_width_algorithm() {
    for custom in [false, true] {
        let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root_node = ui.root_render();
        let node = ui
            .append_render_layout(scope, root_node, LayoutStyle::column(), 0)
            .unwrap();
        ui.append_render_layout(scope, node, fixed(400.0, 20.0), 1)
            .unwrap();
        let writer = ui.state(scope, 77, LayoutStyle::column).unwrap();
        if custom {
            let algorithm = ui
                .computed(scope, 78, &[writer.dependency()], move |input| {
                    let _ = input.read(writer)?;
                    Ok(Some(
                        ParentLayoutSnapshot::new(WidthPhase).with_inline_layout(WidthPhase),
                    ))
                })
                .unwrap();
            ui.bind(algorithm, Property::parent_layout(root_node))
                .unwrap();
        } else {
            ui.bind(writer, Property::layout_style(node)).unwrap();
        }
        let before = ui
            .frame_in_viewport(constraints(100.0), [180.0; 2])
            .unwrap();
        let stats = ui.stats();
        let error = ui.reactive_mut().responsive_output(
            constraints(100.0),
            [180.0; 2],
            false,
            true,
            |draft| {
                assert_eq!(draft.inline_size(node)?, if custom { 50.0 } else { 100.0 });
                draft.apply(vec![(
                    writer,
                    LayoutStyle::row().size(LayoutDimension::Px(80.0), LayoutDimension::Auto),
                )])?;
                let _ = draft.inline_size(node)?;
                Ok(0)
            },
        );
        assert!(matches!(error, Err(Error::UnsafeContainerQuery)));
        assert_eq!(ui.read(writer).unwrap(), &LayoutStyle::column());
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.published_frame().unwrap().revision, before.revision);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_ordinary_container_query_matches_complete_flex_reference() {
    let mut ui = LogicalUi::new_layout(root(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let node = ui
        .append_render_layout(scope, ui.root_render(), host(), 0x0000ffff)
        .unwrap();
    let target = ui
        .append_render_layout(scope, node, fixed(400.0, 20.0), 0xff0000ff)
        .unwrap();
    ui.append_render_layout(scope, ui.root_render(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let source = ui.state(scope, 0, || fixed(400.0, 20.0)).unwrap();
    let mut bindings = LayoutResponsiveBindings::new();
    bindings
        .bind_container(&mut ui, source, target, node, rules())
        .unwrap();
    let mut gpu = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("ordinary query adapter: {:?}", gpu.adapter_info());
    for w in [80.0, 112.0, 140.0, 80.0] {
        let frame = bindings
            .local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([180.0; 2]),
                constraints(w),
                [180.0; 2],
            )
            .unwrap()
            .frame;
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert!(
            gpu.image().unwrap().as_raw()
                == gpu
                    .reference_image(&reference(w), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
        );
    }
    gpu.finish().unwrap();
}
