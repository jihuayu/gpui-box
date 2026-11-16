use super::*;
use crate::retained::{logical::LogicalUi, responsive::*};

#[derive(Debug, Clone)]
struct Widths {
    fraction: f32,
}
impl ParentInlineLayout for Widths {
    fn allocate(
        &self,
        input: ParentInlineInput,
        children: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        let width = input.known_width.unwrap_or(match input.available {
            LayoutAvailableSpace::Definite(w) => w,
            _ => 40.0,
        }) * self.fraction;
        Ok(vec![
            ChildInlineAllocation {
                width,
                percentage_basis: Some(width)
            };
            children
        ])
    }
}

#[derive(Debug, Clone)]
struct Stack;
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut height: f32 = 0.0;
        let mut width: f32 = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            let slot = ChildLayoutInput::new(LayoutConstraints::new(
                LayoutAvailableSpace::Unbounded,
                LayoutAvailableSpace::Unbounded,
            ));
            let measured = children.measure(child, slot)?;
            if let Some(allocation) = children.inline_allocation(child)? {
                // These hard inputs apply to both measurements and placements.
                assert_eq!(measured.size[0], allocation.width);
            }
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, height],
            });
            width = width.max(measured.size[0]);
            height += measured.size[1];
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(width),
                input.known_size.height.unwrap_or(height),
            ],
            baseline: None,
            placements,
        })
    }
}
fn snapshot(fraction: f32) -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Stack).with_inline_layout(Widths { fraction })
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
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    tests::pixels(frame)
}

#[test]
fn independent_width_phase_is_enforced_for_measurement_and_final_placement() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let host = tree.append_layout(tree.root(), column(), 0).unwrap();
    let child = tree
        .append_layout(
            host,
            fixed(999.0, 10.0).fixed_inline_size(999.0),
            0xff0000ff,
        )
        .unwrap();
    let nested = tree
        .append_layout(
            child,
            LayoutStyle::row().size(LayoutDimension::Percent(0.5), LayoutDimension::Px(5.0)),
            0x0000ffff,
        )
        .unwrap();
    let plan = snapshot(0.5);
    assert!(plan.has_inline_layout());
    assert_eq!(plan, plan.clone());
    assert_ne!(
        plan,
        plan.clone().with_inline_layout(Widths { fraction: 0.5 })
    );
    tests::install(&mut tree, host, Some(plan));
    for w in [80.0, 120.0, 80.0] {
        tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap();
        assert_eq!(tree.layout_result(child).unwrap().size[0], w * 0.5);
        assert_eq!(tree.layout_result(nested).unwrap().size[0], w * 0.25);
    }
}

fn setup() -> (LogicalUi, NodeId, NodeId, NodeId) {
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let parent = ui
        .append_render_layout(scope, ui.root_render(), column(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(
            scope,
            parent,
            LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
            0,
        )
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(parent, Some(snapshot(0.5)));
            Ok(())
        },
    )
    .unwrap();
    (ui, parent, host, leaf)
}

#[test]
fn scoped_container_rules_under_custom_width_phase_match_independent_full_flex() {
    for local in [false, true] {
        let (mut ui, _, host, leaf) = setup();
        let scope = ui.root_scope();
        let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
        let mut first = None;
        for w in [80.0, 140.0, 80.0, 140.0] {
            let selected = fixed(20.0, if w < 100.0 { 10.0 } else { 30.0 });
            let render = |(): (), edit: &mut logical::LogicalEdit<'_, '_, '_>| {
                edit.responsive_rule(
                    scope,
                    1,
                    source,
                    leaf,
                    LayoutQuery::ContainerInline(host),
                    LayoutBreakpointStyles::new(
                        fixed(20.0, 10.0),
                        vec![(50.0, fixed(20.0, 30.0))],
                    )?,
                )?;
                edit.responsive_registered(&LayoutEnvironment::new([w, 180.0]))?;
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(constraints(w), [180.0; 2], |_| Ok(()), render)
                    .unwrap()
                    .1
                    .resolve()
            } else {
                ui.edit_frame(constraints(w), [180.0; 2], |_| Ok(()), render)
                    .unwrap()
                    .1
            };
            assert_eq!(ui.read(source).unwrap(), &selected);
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                [w * 0.5, if w < 100.0 { 10.0 } else { 30.0 }]
            );
            let mut full = UiTree::new_layout(column(), 0).unwrap();
            full.set_frame_build_policy(FrameBuildPolicy::Full);
            let parent = full.append_layout(full.root(), column(), 0).unwrap();
            let h = full
                .append_layout(parent, LayoutStyle::column().fixed_inline_size(w * 0.5), 0)
                .unwrap();
            full.append_layout(h, selected, 0xff0000ff).unwrap();
            assert!(
                pixels(frame.clone())
                    == pixels(full.frame_in_viewport(constraints(w), [180.0; 2]).unwrap())
            );
            assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
            if first.is_none() {
                first = Some((frame.clone(), pixels(frame)));
            }
        }
        drop(ui);
        let (old, expected) = first.unwrap();
        assert!(pixels(old) == expected);
    }
}

#[test]
fn standalone_queries_and_nested_custom_width_phases_follow_resizes_and_replacement() {
    let (mut ui, parent, host, leaf) = setup();
    let scope = ui.root_scope();
    // A second allocator composes with the first before its child is queried.
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(host, Some(snapshot(0.5)));
            edit.properties().layout_style(
                leaf,
                LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
            );
            Ok(())
        },
    )
    .unwrap();
    let child = ui
        .append_render_layout(scope, leaf, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            source,
            child,
            leaf,
            LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![(30.0, fixed(20.0, 30.0))])
                .unwrap(),
        )
        .unwrap();
    for (w, fraction) in [(80.0, 0.5), (140.0, 0.5), (140.0, 0.25), (80.0, 0.5)] {
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties()
                    .parent_layout(parent, Some(snapshot(fraction)));
                Ok(())
            },
        )
        .unwrap();
        let frame = rules
            .local_frame_in_viewport(
                &mut ui,
                &LayoutEnvironment::new([w, 180.0]),
                constraints(w),
                [180.0; 2],
            )
            .unwrap()
            .frame;
        let width = w * fraction * 0.5;
        let height = if width >= 30.0 { 30.0 } else { 10.0 };
        assert_eq!(ui.layout_result(leaf).unwrap().size, [width, height]);
        assert_eq!(ui.layout_result(parent).unwrap().size, [w, height]);
        assert_eq!(frame.hit_test(1.0, 1.0), Some(child));
        assert_eq!(
            ui.read(source).unwrap().size.height,
            LayoutDimension::Px(height)
        );
    }
}

#[derive(Debug)]
struct IndexedWidths;
impl ParentInlineLayout for IndexedWidths {
    fn allocate(
        &self,
        _: ParentInlineInput,
        children: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        Ok((0..children)
            .map(|index| ChildInlineAllocation {
                width: if index == 0 { 40.0 } else { 70.0 },
                percentage_basis: None,
            })
            .collect())
    }
}

#[test]
fn sibling_order_query_runs_before_custom_slot_consumer_registered_first() {
    let mut ui = LogicalUi::new_layout(
        column().fixed_inline_size(100.0),
        0,
        BackendCapabilities::default(),
    )
    .unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let sibling_style = fixed(10.0, 5.0);
    let sibling = ui
        .append_render_layout(scope, root, sibling_style.clone(), 0)
        .unwrap();
    let host = ui
        .append_render_layout(
            scope,
            root,
            LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
            0,
        )
        .unwrap();
    let leaf = ui
        .append_render_layout(scope, host, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                root,
                Some(ParentLayoutSnapshot::new(Stack).with_inline_layout(IndexedWidths)),
            );
            Ok(())
        },
    )
    .unwrap();
    let leaf_source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let order_source = ui.state(scope, 2, || sibling_style.clone()).unwrap();
    let mut rules = LayoutResponsiveBindings::new();
    rules
        .bind_container(
            &mut ui,
            leaf_source,
            leaf,
            host,
            LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![(50.0, fixed(20.0, 30.0))])
                .unwrap(),
        )
        .unwrap();
    rules
        .bind_container(
            &mut ui,
            order_source,
            sibling,
            root,
            LayoutBreakpointStyles::new(
                sibling_style.clone(),
                vec![(80.0, sibling_style.order(1))],
            )
            .unwrap(),
        )
        .unwrap();
    rules
        .local_frame_in_viewport(
            &mut ui,
            &LayoutEnvironment::new([100.0, 180.0]),
            constraints(100.0),
            [180.0; 2],
        )
        .unwrap();
    // Root query moves sibling after host; the consumer must see slot 0 (40),
    // rather than selecting the wide branch from its initial slot 1 (70).
    assert_eq!(ui.layout_result(host).unwrap().size, [40.0, 10.0]);
    assert_eq!(
        ui.read(leaf_source).unwrap().size.height,
        LayoutDimension::Px(10.0)
    );
    assert_eq!(ui.layout_result(sibling).unwrap().size[0], 70.0);
}

#[test]
fn inline_builder_under_custom_width_phase_preserves_content_height_and_final_proof() {
    let (mut ui, parent, host, leaf) = setup();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().layout_style(
                host,
                LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                    .container_type(LayoutContainerType::InlineSize),
            );
            Ok(())
        },
    )
    .unwrap();
    for w in [80.0, 140.0, 80.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.inline_constraint_builder(host, |input, edit| {
                        assert_eq!(input.max, w * 0.5);
                        edit.properties().layout_style(
                            leaf,
                            fixed(1000.0, if input.max < 50.0 { 10.0 } else { 30.0 }),
                        );
                        Ok(())
                    })
                },
            )
            .unwrap();
        assert_eq!(
            ui.layout_result(host).unwrap().size,
            [w * 0.5, if w < 100.0 { 10.0 } else { 30.0 }]
        );
        assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
    }
    let before = ui.published_local_frame().unwrap().revision();
    // Removing the protocol invalidates the proof even when numeric width would
    // happen to remain unchanged. The entire edit must roll back.
    let error = ui.edit_local_frame(
        constraints(80.0),
        [180.0; 2],
        |_| Ok(()),
        |(), edit| {
            edit.inline_constraint_builder(host, |_, _| Ok(()))?;
            edit.properties()
                .parent_layout(parent, Some(ParentLayoutSnapshot::new(Stack)));
            Ok(())
        },
    );
    assert!(matches!(error, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.published_local_frame().unwrap().revision(), before);
}

#[derive(Debug)]
struct InvalidWidths(u8);
impl ParentInlineLayout for InvalidWidths {
    fn allocate(
        &self,
        _: ParentInlineInput,
        children: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        Ok(vec![
            ChildInlineAllocation {
                width: if self.0 == 1 { f32::NAN } else { 10.0 },
                percentage_basis: if self.0 == 2 { Some(-1.0) } else { None }
            };
            children + usize::from(self.0 == 0)
        ])
    }
}
#[test]
fn invalid_width_plans_preserve_sources_snapshots_and_layout() {
    let (mut ui, parent, _, _) = setup();
    let scope = ui.root_scope();
    let source = ui.state(scope, 9, || 0u32).unwrap();
    let old = ui
        .edit_local_frame(constraints(80.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
        .unwrap()
        .1;
    let before = ui.stats();
    for mode in [0, 1, 2] {
        let error = ui.edit_local_frame(
            constraints(80.0),
            [180.0; 2],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.properties().parent_layout(
                    parent,
                    Some(ParentLayoutSnapshot::new(Stack).with_inline_layout(InvalidWidths(mode))),
                );
                Ok(())
            },
        );
        assert!(matches!(error, Err(Error::InvalidParentLayout)));
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(ui.stats(), before);
        assert_eq!(
            ui.published_local_frame().unwrap().revision(),
            old.revision()
        );
    }
}

#[test]
fn arbitrary_custom_parent_and_ancestor_feedback_still_reject_queries() {
    for arbitrary in [true, false] {
        let (mut ui, parent, host, leaf) = setup();
        let scope = ui.root_scope();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                if arbitrary {
                    edit.properties()
                        .parent_layout(parent, Some(ParentLayoutSnapshot::new(Stack)));
                } else {
                    // This ancestor can derive its width from the content height.
                    edit.properties().layout_style(
                        parent,
                        LayoutStyle {
                            aspect_ratio: Some(2.0),
                            ..column()
                        },
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        assert_eq!(
            rules.bind_container(
                &mut ui,
                source,
                leaf,
                host,
                LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![]).unwrap()
            ),
            Err(Error::UnsafeContainerQuery)
        );
    }
}

#[test]
fn query_dependent_computed_parent_replacement_rejects_before_registration() {
    let (mut ui, parent, host, leaf) = setup();
    let scope = ui.root_scope();
    let narrow = fixed(20.0, 10.0);
    let source = ui.state(scope, 1, || narrow.clone()).unwrap();
    let current = snapshot(0.5);
    let unsafe_parent = ParentLayoutSnapshot::new(Stack);
    let computed = ui
        .computed(scope, 2, &[source.dependency()], move |input| {
            Ok(Some(
                if input.read(source)?.size.height == LayoutDimension::Px(30.0) {
                    unsafe_parent.clone()
                } else {
                    current.clone()
                },
            ))
        })
        .unwrap();
    ui.bind(computed, reactive::Property::parent_layout(parent))
        .unwrap();
    let old = ui
        .local_frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    let before = ui.stats();
    let mut rules = LayoutResponsiveBindings::new();
    assert_eq!(
        rules.bind_container(
            &mut ui,
            source,
            leaf,
            host,
            LayoutBreakpointStyles::new(narrow, vec![(50.0, fixed(20.0, 30.0))]).unwrap(),
        ),
        Err(Error::UnsafeContainerQuery)
    );
    assert_eq!(ui.stats(), before);
    assert_eq!(
        ui.published_local_frame().unwrap().revision(),
        old.revision()
    );
    assert_eq!(
        ui.read(source).unwrap().size.height,
        LayoutDimension::Px(10.0)
    );
}

#[test]
fn observed_custom_plan_identity_and_child_topology_cannot_change_at_equal_width() {
    for replacement in [true, false] {
        for query in [true, false] {
            let (mut ui, parent, host, leaf) = setup();
            let scope = ui.root_scope();
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties().layout_style(
                        host,
                        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                            .container_type(LayoutContainerType::InlineSize),
                    );
                    Ok(())
                },
            )
            .unwrap();
            let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
            let old = ui
                .edit_local_frame(constraints(80.0), [180.0; 2], |_| Ok(()), |(), _| Ok(()))
                .unwrap()
                .1;
            let error = ui.edit_local_frame(
                constraints(80.0),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    if query {
                        edit.responsive_rule(
                            scope,
                            1,
                            source,
                            leaf,
                            LayoutQuery::ContainerInline(host),
                            LayoutBreakpointStyles::new(fixed(20.0, 10.0), vec![]).unwrap(),
                        )?;
                        edit.responsive_registered(&LayoutEnvironment::new([80.0, 180.0]))?;
                    } else {
                        edit.inline_constraint_builder(host, |_, _| Ok(()))?;
                    }
                    if replacement {
                        // Same numeric allocation but a new owned algorithm input.
                        edit.properties().parent_layout(parent, Some(snapshot(0.5)));
                    } else {
                        // This particular planner ignores count; the protocol allows
                        // others to divide by it, so structure is a real dependency.
                        edit.append_layout(scope, parent, fixed(20.0, 10.0), 0)?;
                    }
                    Ok(())
                },
            );
            assert!(matches!(
                error,
                Err(Error::UnsafeContainerQuery | Error::UnsafeConstraintBuilder)
            ));
            assert_eq!(
                ui.published_local_frame().unwrap().revision(),
                old.revision()
            );
        }
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_independent_parent_query_and_builder_match_fresh_full_flex() {
    let (mut ui, parent, host, leaf) = setup();
    let scope = ui.root_scope();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().layout_style(
                host,
                LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose)
                    .container_type(LayoutContainerType::InlineSize),
            );
            Ok(())
        },
    )
    .unwrap();
    let source = ui.state(scope, 1, || fixed(20.0, 10.0)).unwrap();
    let mut renderer = wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("independent parent adapter: {:?}", renderer.adapter_info());
    for w in [80.0, 140.0, 80.0, 140.0] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(w),
                [180.0; 2],
                |_| Ok(()),
                |(), edit| {
                    edit.responsive_rule(
                        scope,
                        1,
                        source,
                        leaf,
                        LayoutQuery::ContainerInline(host),
                        LayoutBreakpointStyles::new(
                            fixed(20.0, 10.0),
                            vec![(50.0, fixed(20.0, 30.0))],
                        )?,
                    )?;
                    edit.responsive_registered(&LayoutEnvironment::new([w, 180.0]))?;
                    edit.inline_constraint_builder(host, |input, _| {
                        assert_eq!(input.max, w * 0.5);
                        Ok(())
                    })
                },
            )
            .unwrap();
        renderer
            .submit_local(&frame, [180; 2], 1.0)
            .unwrap()
            .unwrap();
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let p = full.append_layout(full.root(), column(), 0).unwrap();
        let h = full
            .append_layout(p, LayoutStyle::column().fixed_inline_size(w * 0.5), 0)
            .unwrap();
        full.append_layout(
            h,
            fixed(w * 0.5, if w < 100.0 { 10.0 } else { 30.0 }),
            0xff0000ff,
        )
        .unwrap();
        let reference = full.frame_in_viewport(constraints(w), [180.0; 2]).unwrap();
        assert!(
            actual.as_raw()
                == renderer
                    .reference_image(&reference, [180; 2], 1.0)
                    .unwrap()
                    .as_raw()
        );
        assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
        assert_eq!(ui.layout_result(parent).unwrap().size[0], w);
    }
    renderer.finish().unwrap();
}
