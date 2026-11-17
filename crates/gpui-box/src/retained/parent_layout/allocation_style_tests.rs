use super::*;
use crate::retained::{
    logical::{LogicalEdit, LogicalUi, ScopeId},
    reactive::Property,
};

#[derive(Debug)]
struct Weighted;
impl ParentAllocationLayout for Weighted {
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        assert!(children.style(children.len()).is_err());
        let styles = (0..children.len())
            .map(|i| children.style(i))
            .collect::<Result<Vec<_>, _>>()?;
        if styles.iter().any(|s| {
            s.flex_grow <= 0.0
                || !s.flex_grow.is_finite()
                || s.flex_basis != LayoutDimension::Px(0.0)
        }) {
            return Err(Error::InvalidParentLayout);
        }
        let width = input
            .known_size
            .width
            .or(match input.constraints.width {
                LayoutAvailableSpace::Definite(w) => Some(w),
                _ => None,
            })
            .ok_or(Error::InvalidParentLayout)?;
        let total: f32 = styles
            .iter()
            .filter(|s| !s.hidden)
            .map(|s| s.flex_grow)
            .sum();
        if total <= 0.0 || !total.is_finite() {
            return Err(Error::InvalidParentLayout);
        }
        Ok(styles
            .into_iter()
            .map(|s| {
                let w = if s.hidden {
                    0.0
                } else {
                    width * s.flex_grow / total
                };
                ChildLayoutInput {
                    constraints: LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(w),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([w, 0.0], [w, f32::INFINITY])),
                    percentage_basis: LayoutSize::new(Some(w), None),
                    ..ChildLayoutInput::default()
                }
            })
            .collect())
    }
}
impl ParentLayout for Weighted {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut x: f32 = 0.0;
        let mut height: f32 = 0.0;
        let mut baseline = None;
        let mut last_baseline = None;
        let mut placements = vec![];
        for child in 0..children.len() {
            let style = children.style(child)?;
            let slot = children
                .allocation(child)?
                .ok_or(Error::InvalidParentLayout)?;
            let measured = children.measure_baselines(
                child,
                ChildLayoutInput::new(LayoutConstraints::viewport(999.0, 999.0)),
            )?;
            assert_eq!(measured.size[0], slot.constraints.bounds.min.width);
            if !style.hidden {
                baseline.get_or_insert(measured.baseline.unwrap_or(measured.size[1]));
                last_baseline = Some(measured.last_baseline.unwrap_or(measured.size[1]));
            }
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [x, 0.0],
            });
            x += measured.size[0];
            height = height.max(measured.size[1]);
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(x),
                input.known_size.height.unwrap_or(height),
            ],
            baseline,
            last_baseline,
            placements,
        })
    }
}
fn snapshot() -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Weighted).with_allocation_layout(Weighted)
}
fn style(weight: f32) -> LayoutStyle {
    LayoutStyle::column()
        .flex_basis(LayoutDimension::Px(0.0))
        .flex_grow(weight)
        .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto)
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn root_style() -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .align_items(LayoutAlignItems::Start)
}
fn constraints(width: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(width),
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixture() -> (LogicalUi, [NodeId; 2]) {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let hosts = [
        ui.append_render_layout(scope, root, style(1.0), 0).unwrap(),
        ui.append_render_layout(scope, root, style(2.0), 0).unwrap(),
    ];
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(root, Some(snapshot()));
            Ok(())
        },
    )
    .unwrap();
    (ui, hosts)
}
fn build(
    edit: &mut LogicalEdit<'_, '_, '_>,
    hosts: [NodeId; 2],
    scope: ScopeId,
    width: f32,
    weights: [f32; 2],
) -> Result<(), Error> {
    let branch = edit.reconcile_keys(scope, &[0])?[0];
    for (i, &host) in hosts.iter().enumerate() {
        edit.allocation_builder(host, |input, edit| {
            assert_eq!(
                input.constraints.bounds.min.width,
                width * weights[i] / (weights[0] + weights[1])
            );
            if edit.renders(branch)?.len() < (i + 1) * 4 {
                let row = edit.append_layout(
                    branch,
                    host,
                    LayoutStyle::row()
                        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
                        .flex_wrap(LayoutFlexWrap::Wrap),
                    0,
                )?;
                for _ in 0..3 {
                    edit.append_layout(
                        branch,
                        row,
                        fixed(20.0, 10.0),
                        if i == 0 { 0xff0000ff } else { 0x00ff00ff },
                    )?;
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}
fn reference(width: f32, weights: [f32; 2]) -> (UiTree, [NodeId; 2], FramePacket) {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let hosts = weights.map(|w| tree.append_layout(tree.root(), style(w), 0).unwrap());
    for (i, host) in hosts.into_iter().enumerate() {
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
            tree.append_layout(
                row,
                fixed(20.0, 10.0),
                if i == 0 { 0xff0000ff } else { 0x00ff00ff },
            )
            .unwrap();
        }
    }
    let frame = tree
        .frame_in_viewport(constraints(width), [180.0, 180.0])
        .unwrap();
    (tree, hosts, frame)
}

#[test]
fn independent_style_allocations_build_wrapped_flex_with_full_cpu_reference() {
    for local in [false, true] {
        let (mut ui, hosts) = fixture();
        let scope = ui.root_scope();
        let mut old = None;
        for (width, weights) in [
            (90.0, [1.0, 2.0]),
            (180.0, [2.0, 1.0]),
            (120.0, [1.0, 2.0]),
            (90.0, [1.0, 2.0]),
        ] {
            let render = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                for (i, &host) in hosts.iter().enumerate() {
                    edit.properties().layout_style(host, style(weights[i]));
                }
                build(edit, hosts, scope, width, weights)
            };
            let frame = if local {
                ui.edit_local_frame(constraints(width), [180.0, 180.0], |_| Ok(()), render)
                    .unwrap()
                    .1
                    .resolve()
            } else {
                ui.edit_frame(constraints(width), [180.0, 180.0], |_| Ok(()), render)
                    .unwrap()
                    .1
            };
            let (full, reference_hosts, reference) = reference(width, weights);
            for (node, expected) in hosts.into_iter().zip(reference_hosts) {
                assert_eq!(
                    ui.layout_result(node).unwrap(),
                    full.layout_result(expected).unwrap()
                );
            }
            assert_eq!(tests::pixels(frame.clone()), tests::pixels(reference));
            assert!(frame.hit_test(1.0, 1.0).is_some());
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let pixels = tests::pixels(old.clone());
        drop(ui);
        assert_eq!(tests::pixels(old), pixels);
    }
}

#[test]
fn every_sibling_effective_style_is_an_observed_dependency_even_for_equal_allocations() {
    for local in [false, true] {
        for mutation in 0..6 {
            let (mut ui, hosts) = fixture();
            let scope = ui.root_scope();
            let old = ui
                .frame_in_viewport(constraints(90.0), [180.0, 180.0])
                .unwrap();
            let signal = ui.state(scope, 500, || 0u32).unwrap();
            let before = ui.stats();
            let render = |(), edit: &mut LogicalEdit<'_, '_, '_>| {
                edit.allocation_builder(hosts[0], |_, edit| {
                    edit.append_layout(scope, hosts[0], fixed(10.0, 10.0), 1)?;
                    let target = hosts[usize::from(mutation != 0)];
                    match mutation {
                        0 | 1 => {
                            let mut replacement = style(if mutation == 0 { 1.0 } else { 2.0 });
                            replacement.align_self = LayoutAlignSelf::Start.into();
                            edit.properties().layout_style(target, replacement);
                        }
                        2 => edit.properties().measure(
                            target,
                            Some(MeasureSnapshot::Image(ImageIntrinsicSize {
                                width: 2.0,
                                height: 1.0,
                            })),
                        ),
                        3 => edit
                            .properties()
                            .image(target, Some(ImageResource::rgba8([2, 1], vec![255; 8])?)),
                        5 => edit.properties().layout_style(
                            target,
                            style(2.0).contain_intrinsic_size(Some(70.0), Some(25.0)),
                        ),
                        _ => edit.properties().layout_style(target, style(3.0)),
                    }
                    Ok(())
                })
            };
            let result = if local {
                ui.edit_local_frame(
                    constraints(90.0),
                    [180.0, 180.0],
                    |tx| {
                        tx.set(signal, 1)?;
                        Ok(())
                    },
                    render,
                )
                .map(|_| ())
            } else {
                ui.edit_frame(
                    constraints(90.0),
                    [180.0, 180.0],
                    |tx| {
                        tx.set(signal, 1)?;
                        Ok(())
                    },
                    render,
                )
                .map(|_| ())
            };
            assert!(
                matches!(result, Err(Error::UnsafeConstraintBuilder)),
                "mutation={mutation}/local={local}"
            );
            assert_eq!(ui.read(signal).unwrap(), &0);
            assert_eq!(ui.stats(), before);
            assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        }
    }
}

#[test]
fn restored_style_writes_are_allowed_and_descendant_styles_do_not_change_incoming_slots() {
    let (mut ui, hosts) = fixture();
    let scope = ui.root_scope();
    ui.edit_frame(
        constraints(90.0),
        [180.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(hosts[0], |_, edit| {
                edit.properties().layout_style(hosts[1], style(3.0));
                edit.properties().layout_style(hosts[1], style(2.0));
                let child = edit.append_layout(scope, hosts[0], fixed(10.0, 10.0), 1)?;
                edit.properties().layout_style(child, fixed(20.0, 20.0));
                Ok(())
            })
        },
    )
    .unwrap();
    assert_eq!(ui.layout_result(hosts[0]).unwrap().size, [30.0, 20.0]);
    let signal = ui.state(scope, 501, || style(1.0)).unwrap();
    ui.bind(signal, Property::layout_style(hosts[0])).unwrap();
    ui.transaction(|tx| {
        tx.set(signal, style(2.0))?;
        Ok(())
    })
    .unwrap();
    ui.frame_in_viewport(constraints(90.0), [180.0, 180.0])
        .unwrap();
    assert_eq!(ui.layout_result(hosts[0]).unwrap().size[0], 45.0);
    ui.transaction(|tx| {
        tx.set(signal, style(1.0))?;
        tx.set(signal, style(2.0))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        ui.frame_in_viewport(constraints(90.0), [180.0, 180.0])
            .unwrap()
            .layout_stats,
        LayoutStats::default()
    );
}

#[derive(Debug)]
struct Basis;
impl ParentAllocationLayout for Basis {
    fn allocate_children(
        &self,
        _: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        (0..children.len())
            .map(|i| {
                let LayoutDimension::Px(width) = children.style(i)?.flex_basis else {
                    return Err(Error::InvalidParentLayout);
                };
                Ok(ChildLayoutInput {
                    constraints: LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(width),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                    percentage_basis: LayoutSize::new(Some(width), None),
                    ..ChildLayoutInput::default()
                })
            })
            .collect()
    }
}

#[test]
fn computed_style_sinks_settle_before_queries_of_sibling_flex_allocation() {
    use crate::retained::responsive::*;
    for structural in [false, true] {
        let mut ui = LogicalUi::new_layout(
            LayoutStyle::row()
                .fixed_inline_size(300.0)
                .size(LayoutDimension::Px(300.0), LayoutDimension::Px(100.0)),
            0,
            BackendCapabilities::default(),
        )
        .unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let sibling = ui
            .append_render_layout(scope, root, LayoutStyle::row().flex_shrink(0.0), 0)
            .unwrap();
        let leaf = ui
            .append_render_layout(
                scope,
                sibling,
                fixed(20.0, 10.0).flex_basis(LayoutDimension::Px(20.0)),
                0,
            )
            .unwrap();
        let query_host = ui
            .append_render_layout(
                scope,
                root,
                LayoutStyle::column()
                    .container_type(LayoutContainerType::Size)
                    .flex_grow(1.0)
                    .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
                0,
            )
            .unwrap();
        let base = fixed(10.0, 10.0);
        let target = ui
            .append_render_layout(scope, query_host, base.clone(), 1)
            .unwrap();
        let marker_style = fixed(0.0, 0.0);
        let marker = ui
            .append_render_layout(scope, root, marker_style.clone(), 0)
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(
                    sibling,
                    Some(ParentLayoutSnapshot::new(Weighted).with_allocation_layout(Basis)),
                );
                Ok(())
            },
        )
        .unwrap();
        let producer = ui.state(scope, 100, || marker_style.clone()).unwrap();
        let consumer = ui.state(scope, 101, || base.clone()).unwrap();
        let derived = ui
            .computed(scope, 102, &[producer.dependency()], move |input| {
                Ok(fixed(20.0, 10.0).flex_basis(LayoutDimension::Px(
                    if input.read(producer)?.flex_direction == LayoutFlexDirection::Column {
                        100.0
                    } else {
                        20.0
                    },
                )))
            })
            .unwrap();
        let mut rules = LayoutResponsiveBindings::new();
        rules
            .bind_container(
                &mut ui,
                consumer,
                target,
                query_host,
                LayoutBreakpointStyles::new(
                    base.clone(),
                    vec![(250.0, base.flex_direction(LayoutFlexDirection::Column))],
                )
                .unwrap(),
            )
            .unwrap();
        rules
            .bind_container(
                &mut ui,
                producer,
                marker,
                root,
                LayoutBreakpointStyles::new(
                    marker_style.clone(),
                    vec![(
                        250.0,
                        marker_style.flex_direction(LayoutFlexDirection::Column),
                    )],
                )
                .unwrap(),
            )
            .unwrap();
        ui.bind(derived, Property::layout_style(leaf)).unwrap();
        if structural {
            ui.edit_frame(
                LayoutConstraints::viewport(300.0, 100.0),
                [300.0, 100.0],
                |_| Ok(()),
                |(), edit| {
                    edit.responsive(&rules, &LayoutEnvironment::new([300.0, 100.0]))?;
                    Ok(())
                },
            )
            .unwrap();
        } else {
            rules
                .frame(&mut ui, &LayoutEnvironment::new([300.0, 100.0]))
                .unwrap();
        }
        assert_eq!(ui.layout_result(leaf).unwrap().size[0], 100.0);
        assert_eq!(ui.layout_result(query_host).unwrap().size[0], 200.0);
        assert_eq!(
            ui.read(consumer).unwrap().flex_direction,
            LayoutFlexDirection::Row
        );
    }
}

#[test]
fn nested_allocator_observations_include_ancestor_sibling_styles() {
    let (mut ui, hosts) = fixture();
    let scope = ui.root_scope();
    let child = ui
        .append_render_layout(scope, hosts[0], style(1.0), 0)
        .unwrap();
    let peer = ui
        .append_render_layout(scope, hosts[0], style(1.0), 0)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(hosts[0], Some(snapshot()));
            Ok(())
        },
    )
    .unwrap();
    let old = ui
        .frame_in_viewport(constraints(120.0), [180.0, 180.0])
        .unwrap();
    for changed in [hosts[1], peer] {
        let original = ui.layout_result(child).unwrap();
        let result = ui.edit_local_frame(
            constraints(120.0),
            [180.0, 180.0],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(child, |input, edit| {
                    assert_eq!(input.constraints.bounds.min.width, 20.0);
                    edit.append_layout(scope, child, fixed(10.0, 10.0), 1)?;
                    let mut replacement = style(if changed == hosts[1] { 2.0 } else { 1.0 });
                    replacement.align_self = LayoutAlignSelf::Start.into();
                    edit.properties().layout_style(changed, replacement);
                    Ok(())
                })
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
        assert_eq!(ui.layout_result(child).unwrap(), original);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

#[test]
fn allocation_styles_follow_hidden_order_and_reparent_without_positional_copies() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let root = tree.root();
    let a = tree.append_layout(root, style(1.0), 1).unwrap();
    let b = tree.append_layout(root, style(2.0), 2).unwrap();
    for node in [a, b] {
        tree.append_layout(node, fixed(10.0, 10.0), 3).unwrap();
    }
    tests::install(&mut tree, root, Some(snapshot()));
    tree.frame_in_viewport(constraints(90.0), [180.0, 180.0])
        .unwrap();
    assert_eq!(tree.layout_result(a).unwrap().size[0], 30.0);
    let mut tx = Transaction::default();
    tx.reorder(root, vec![b, a]);
    tx.layout_style(a, style(1.0).display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(90.0), [180.0, 180.0])
        .unwrap();
    assert_eq!(tree.layout_result(b).unwrap().size[0], 90.0);
    assert_eq!(tree.layout_result(a).unwrap().size, [0.0; 2]);
    let mut tx = Transaction::default();
    tx.layout_style(a, style(1.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(90.0), [180.0, 180.0])
        .unwrap();
    assert_eq!(tree.layout_result(a).unwrap().location[0], 60.0);
    let other = tree.append_layout(root, style(3.0), 0).unwrap();
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(a, other)?;
        edit.properties().parent_layout(other, Some(snapshot()));
        Ok(())
    })
    .unwrap();
    tree.frame_in_viewport(constraints(90.0), [180.0, 180.0])
        .unwrap();
    assert_eq!(tree.layout_result(a).unwrap().size[0], 54.0);
    assert_eq!(tree.layout_result(other).unwrap().location[0], 36.0);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_independent_style_allocation_and_builders_match_full_flex() {
    let (mut ui, hosts) = fixture();
    let scope = ui.root_scope();
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("allocation-style adapter: {:?}", gpu.adapter_info());
    for (width, weights) in [(90.0, [1.0, 2.0]), (180.0, [2.0, 1.0]), (90.0, [1.0, 2.0])] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [180.0, 180.0],
                |_| Ok(()),
                |(), edit| {
                    for (i, &host) in hosts.iter().enumerate() {
                        edit.properties().layout_style(host, style(weights[i]));
                    }
                    build(edit, hosts, scope, width, weights)
                },
            )
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        let reference = reference(width, weights).2;
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
