use super::*;
use crate::retained::logical::{LogicalEdit, LogicalUi};
use crate::retained::reactive::Property;

#[derive(Debug)]
struct Weight(f32);

#[derive(Debug)]
struct Weighted;
impl ParentAllocationLayout for Weighted {
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        assert!(children.parent_data(children.len()).is_err());
        let weights = (0..children.len())
            .map(|i| {
                children
                    .parent_data(i)?
                    .as_ref()
                    .and_then(|v| v.get::<Weight>())
                    .map(|v| v.0)
                    .ok_or(Error::InvalidParentLayout)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let sum: f32 = weights.iter().sum();
        if !sum.is_finite() || sum <= 0.0 || weights.iter().any(|v| *v <= 0.0) {
            return Err(Error::InvalidParentLayout);
        }
        let width = input
            .known_size
            .width
            .or(match input.constraints.width {
                LayoutAvailableSpace::Definite(v) => Some(v),
                _ => None,
            })
            .ok_or(Error::InvalidParentLayout)?;
        Ok(weights
            .into_iter()
            .map(|w| {
                let width = width * w / sum;
                ChildLayoutInput {
                    constraints: LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(width),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                    percentage_basis: LayoutSize::new(Some(width), None),
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
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut placements = vec![];
        assert!(children.parent_data(children.len()).is_err());
        for child in 0..children.len() {
            assert!(
                children
                    .parent_data(child)?
                    .unwrap()
                    .get::<Weight>()
                    .is_some()
            );
            let slot = children
                .allocation(child)?
                .ok_or(Error::InvalidParentLayout)?;
            // A measured algorithm cannot replace the independent input.
            let request = ChildLayoutInput::new(LayoutConstraints::viewport(999.0, 999.0));
            let result = children.measure(child, request)?;
            assert_eq!(result.size[0], slot.constraints.bounds.min.width);
            placements.push(ParentPlacement {
                child,
                input: request,
                origin: [x, 0.0],
            });
            x += result.size[0];
            height = height.max(result.size[1]);
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(x),
                input.known_size.height.unwrap_or(height),
            ],
            baseline: None,
            last_baseline: None,
            placements,
        })
    }
}
fn snapshot() -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Weighted).with_allocation_layout(Weighted)
}
fn root_style() -> LayoutStyle {
    LayoutStyle::row().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
}
fn constraints(w: f32) -> LayoutConstraints {
    LayoutConstraints::new(
        LayoutAvailableSpace::Definite(w),
        LayoutAvailableSpace::Unbounded,
    )
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn wrap() -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .flex_wrap(LayoutFlexWrap::Wrap)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [200, 120],
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
fn reference(width: f32, weights: [f32; 2]) -> FramePacket {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    for (i, weight) in weights.into_iter().enumerate() {
        let host = tree
            .append_layout(
                tree.root(),
                LayoutStyle::column()
                    .flex_basis(LayoutDimension::Px(0.0))
                    .flex_grow(weight)
                    .min_size(LayoutDimension::Px(0.0), LayoutDimension::Auto),
                0,
            )
            .unwrap();
        let row = tree.append_layout(host, wrap(), 0).unwrap();
        for _ in 0..3 {
            tree.append_layout(
                row,
                fixed(20.0, 10.0),
                if i == 0 { 0xff0000ff } else { 0x00ff00ff },
            )
            .unwrap();
        }
    }
    tree.frame_in_viewport(constraints(width), [200.0, 120.0])
        .unwrap()
}
fn fixture() -> (LogicalUi, [NodeId; 2]) {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let hosts = [
        ui.append_render_layout(scope, root, LayoutStyle::column(), 0)
            .unwrap(),
        ui.append_render_layout(scope, root, LayoutStyle::column(), 0)
            .unwrap(),
    ];
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(root, Some(snapshot()));
            for (i, &host) in hosts.iter().enumerate() {
                edit.properties()
                    .parent_data(host, Some(ParentData::new(Weight((i + 1) as f32))));
            }
            Ok(())
        },
    )
    .unwrap();
    (ui, hosts)
}
fn build(
    edit: &mut LogicalEdit<'_, '_, '_>,
    hosts: [NodeId; 2],
    scope: crate::retained::logical::ScopeId,
    width: f32,
    weights: [f32; 2],
) -> Result<(), Error> {
    for (i, host) in hosts.into_iter().enumerate() {
        edit.allocation_builder(host, |input, edit| {
            assert_eq!(
                input.constraints.bounds.min.width,
                width * weights[i] / (weights[0] + weights[1])
            );
            let branch = edit.reconcile_keys(scope, &[0])?[0];
            // Both hosts share scope ownership but output parentage is independent.
            let rows = edit.renders(branch)?;
            if rows.len() < (i + 1) * 4 {
                let row = edit.append_layout(branch, host, wrap(), 0)?;
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

#[test]
fn parent_data_weighted_builders_reflow_like_ordinary_flex_and_own_old_frames() {
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
            let render = |(): (), edit: &mut LogicalEdit<'_, '_, '_>| {
                for (i, &host) in hosts.iter().enumerate() {
                    edit.properties()
                        .parent_data(host, Some(ParentData::new(Weight(weights[i]))));
                }
                build(edit, hosts, scope, width, weights)
            };
            let frame = if local {
                ui.edit_local_frame(constraints(width), [200.0, 120.0], |_| Ok(()), render)
                    .unwrap()
                    .1
                    .resolve()
            } else {
                ui.edit_frame(constraints(width), [200.0, 120.0], |_| Ok(()), render)
                    .unwrap()
                    .1
            };
            assert_eq!(pixels(frame.clone()), pixels(reference(width, weights)));
            assert!(frame.hit_test(1.0, 1.0).is_some());
            old.get_or_insert_with(|| (frame.clone(), pixels(frame)));
        }
        drop(ui);
        let (frame, expected) = old.unwrap();
        assert_eq!(pixels(frame), expected);
    }
}

#[test]
fn parent_data_identity_coalesces_and_typed_sink_invalidates_cached_parent() {
    let (mut ui, hosts) = fixture();
    let scope = ui.root_scope();
    let one = ParentData::new(Weight(1.0));
    let two = ParentData::new(Weight(2.0));
    assert!(one.get::<u32>().is_none());
    assert_ne!(one, ParentData::new(Weight(1.0)));
    let signal = ui.state(scope, 8, || Some(one.clone())).unwrap();
    ui.bind(signal, Property::parent_data(hosts[0])).unwrap();
    let (_, frame) = ui
        .edit_frame(
            constraints(90.0),
            [200.0, 120.0],
            |_| Ok(()),
            |(), edit| build(edit, hosts, scope, 90.0, [1.0, 2.0]),
        )
        .unwrap();
    let unchanged = ui
        .transaction(|tx| {
            tx.set(signal, Some(one.clone()))?;
            Ok(())
        })
        .unwrap();
    assert!(!unchanged.commit.request_frame);
    let changed = ui
        .transaction(|tx| {
            tx.set(signal, Some(two.clone()))?;
            Ok(())
        })
        .unwrap();
    assert!(changed.commit.request_frame);
    let next = ui
        .frame_in_viewport(constraints(90.0), [200.0, 120.0])
        .unwrap();
    assert_eq!(ui.layout_result(hosts[0]).unwrap().size[0], 45.0);
    assert_eq!(pixels(next), pixels(reference(90.0, [2.0, 2.0])));
    assert_eq!(pixels(frame), pixels(reference(90.0, [1.0, 2.0])));
}

#[test]
fn parent_data_late_equal_number_dependency_replacement_rejects_atomically() {
    for sibling in [false, true] {
        let (mut ui, hosts) = fixture();
        let scope = ui.root_scope();
        let original = ui
            .frame_in_viewport(constraints(90.0), [200.0, 120.0])
            .unwrap();
        let before = ui.reactive().tree().revision;
        let signal = ui.state(scope, 9, || 0u32).unwrap();
        let result = ui.edit_frame(
            constraints(90.0),
            [200.0, 120.0],
            |tx| {
                tx.set(signal, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.allocation_builder(hosts[0], |_, edit| {
                    edit.append_layout(scope, hosts[0], fixed(10.0, 10.0), 0xff0000ff)?;
                    Ok(())
                })?;
                let index = usize::from(sibling);
                edit.properties().parent_data(
                    hosts[index],
                    Some(ParentData::new(Weight((index + 1) as f32))),
                );
                Ok(())
            },
        );
        assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
        assert_eq!(*ui.read(signal).unwrap(), 0);
        assert_eq!(ui.reactive().tree().revision, before);
        assert_eq!(ui.published_frame().unwrap().revision, original.revision);
    }
}

#[test]
fn parent_data_batch_coalesces_and_reorder_moves_data_with_node_identity() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let root = tree.root();
    let a = tree
        .append_layout(root, fixed(1.0, 10.0), 0xff0000ff)
        .unwrap();
    let b = tree
        .append_layout(root, fixed(1.0, 10.0), 0x00ff00ff)
        .unwrap();
    let one = ParentData::new(Weight(1.0));
    let two = ParentData::new(Weight(2.0));
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(snapshot()));
    tx.parent_data(a, Some(one.clone()));
    tx.parent_data(b, Some(two.clone()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let old = tree
        .frame_in_viewport(constraints(90.0), [200.0, 120.0])
        .unwrap();
    assert_eq!(tree.layout_result(a).unwrap().size[0], 30.0);
    let mut equal = Transaction::default();
    equal.parent_data(a, Some(two));
    equal.parent_data(a, Some(one.clone()));
    let result = tree.commit(equal, BackendCapabilities::default()).unwrap();
    assert!(!result.request_frame);
    assert_eq!(result.changed_nodes, 0);
    let mut order = Transaction::default();
    order.reorder(root, vec![b, a]);
    tree.commit(order, BackendCapabilities::default()).unwrap();
    let next = tree
        .frame_in_viewport(constraints(90.0), [200.0, 120.0])
        .unwrap();
    assert_eq!(tree.layout_result(b).unwrap().size[0], 60.0);
    assert_eq!(tree.layout_result(a).unwrap().location[0], 60.0);
    assert_eq!(tree.parent_data(a).unwrap(), Some(&one));
    assert_eq!(old.hit_test(1.0, 1.0), Some(a));
    assert_eq!(next.hit_test(1.0, 1.0), Some(b));
}

#[test]
fn parent_data_invalid_type_or_missing_data_rolls_back_sources_geometry_and_frame() {
    for data in [
        None,
        Some(ParentData::new(12u32)),
        Some(ParentData::new(Weight(-1.0))),
    ] {
        let (mut ui, hosts) = fixture();
        let scope = ui.root_scope();
        let original = ui
            .frame_in_viewport(constraints(90.0), [200.0, 120.0])
            .unwrap();
        let before = ui.layout_result(hosts[0]).unwrap();
        let source = ui.state(scope, 42, || 0u32).unwrap();
        let result = ui.edit_local_frame(
            constraints(90.0),
            [200.0, 120.0],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.properties().parent_data(hosts[0], data);
                Ok(())
            },
        );
        assert!(matches!(result, Err(Error::InvalidParentLayout)));
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(ui.layout_result(hosts[0]).unwrap(), before);
        assert_eq!(ui.published_frame().unwrap().revision, original.revision);
    }
}

#[derive(Debug)]
struct MeasuredData;
impl ParentLayout for MeasuredData {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let data = children.parent_data(0)?.ok_or(Error::InvalidParentLayout)?;
        let width = *data.get::<f32>().ok_or(Error::InvalidParentLayout)?;
        let slot = ChildLayoutInput::new(
            LayoutConstraints::viewport(width, 10.0)
                .with_bounds(BoxConstraints::tight([width, 10.0])),
        );
        let child = children.measure(0, slot)?;
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: None,
            last_baseline: None,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [0.0; 2],
            }],
        })
    }
}

#[test]
fn measured_parent_reads_metadata_after_reparent_without_rewriting_data() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let a = tree.append_layout(root, LayoutStyle::column(), 0).unwrap();
    let b = tree.append_layout(root, LayoutStyle::column(), 0).unwrap();
    let leaf = tree
        .append_layout(a, LayoutStyle::row(), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.parent_layout(a, Some(ParentLayoutSnapshot::new(MeasuredData)));
    tx.parent_data(leaf, Some(ParentData::new(35.0f32)));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let old = tree
        .frame_in_viewport(constraints(90.0), [200.0, 120.0])
        .unwrap();
    assert_eq!(tree.layout_result(leaf).unwrap().size, [35.0, 10.0]);
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(leaf, b)?;
        edit.properties().parent_layout(a, None);
        edit.properties()
            .parent_layout(b, Some(ParentLayoutSnapshot::new(MeasuredData)));
        Ok(())
    })
    .unwrap();
    let next = tree
        .frame_in_viewport(constraints(90.0), [200.0, 120.0])
        .unwrap();
    assert_eq!(tree.layout_result(leaf).unwrap().size, [35.0, 10.0]);
    assert_eq!(
        tree.parent_data(leaf).unwrap().unwrap().get::<f32>(),
        Some(&35.0)
    );
    assert_eq!(pixels(old), pixels(next));
}

#[test]
fn computed_parent_data_sinks_join_container_query_dependency_order() {
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
            .append_render_layout(scope, root, LayoutStyle::row(), 0)
            .unwrap();
        let leaf = ui
            .append_render_layout(scope, sibling, LayoutStyle::row(), 0)
            .unwrap();
        let host = ui
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
            .append_render_layout(scope, host, base.clone(), 1)
            .unwrap();
        let marker_style = fixed(0.0, 0.0);
        let marker = ui
            .append_render_layout(scope, root, marker_style.clone(), 0)
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties()
                    .parent_layout(sibling, Some(ParentLayoutSnapshot::new(MeasuredData)));
                Ok(())
            },
        )
        .unwrap();
        let producer = ui.state(scope, 100, || marker_style.clone()).unwrap();
        let consumer = ui.state(scope, 101, || base.clone()).unwrap();
        let data = ui
            .computed(scope, 102, &[producer.dependency()], move |input| {
                Ok(Some(ParentData::new(
                    if input.read(producer)?.flex_direction == LayoutFlexDirection::Column {
                        100.0f32
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
                host,
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
        // Register the indirect layout sink after both rules.
        ui.bind(data, Property::parent_data(leaf)).unwrap();
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
        assert_eq!(ui.layout_result(host).unwrap().size[0], 200.0);
        assert_eq!(
            ui.read(consumer).unwrap().flex_direction,
            LayoutFlexDirection::Row
        );
        assert_eq!(
            ui.parent_data(leaf).unwrap().unwrap().get::<f32>(),
            Some(&100.0)
        );
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_parent_data_weighted_allocation_matches_full_flex() {
    let (mut ui, hosts) = fixture();
    let scope = ui.root_scope();
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("parent-data adapter: {:?}", gpu.adapter_info());
    for (width, weights) in [(90.0, [1.0, 2.0]), (180.0, [2.0, 1.0]), (90.0, [1.0, 2.0])] {
        let (_, frame) = ui
            .edit_local_frame(
                constraints(width),
                [200.0, 120.0],
                |_| Ok(()),
                |(), edit| {
                    for (i, &host) in hosts.iter().enumerate() {
                        edit.properties()
                            .parent_data(host, Some(ParentData::new(Weight(weights[i]))));
                    }
                    build(edit, hosts, scope, width, weights)
                },
            )
            .unwrap();
        gpu.submit_local(&frame, [200, 120], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(width, weights), [200, 120], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}

#[test]
fn new_keyed_children_receive_metadata_before_builder_and_retire_owned_values() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let root = ui.root_render();
    let token = Arc::new(());
    let weak = Arc::downgrade(&token);
    let (_, frame) = ui
        .edit_local_frame(
            constraints(90.0),
            [200.0, 120.0],
            |_| Ok(()),
            |(), edit| {
                let branch = edit.reconcile_keys(scope, &[1])?[0];
                let host = edit.append_layout(branch, root, LayoutStyle::column(), 0)?;
                edit.properties().parent_layout(root, Some(snapshot()));
                edit.properties()
                    .parent_data(host, Some(ParentData::new(Weight(1.0))));
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.constraints.bounds.min.width, 90.0);
                    let leaf = edit.append_layout(branch, host, fixed(20.0, 10.0), 0xff0000ff)?;
                    // The ordinary Flex host ignores this metadata; it stays authored.
                    edit.properties()
                        .parent_data(leaf, Some(ParentData::new(token)));
                    Ok(())
                })
            },
        )
        .unwrap();
    assert!(weak.upgrade().is_some());
    ui.edit_local_frame(
        constraints(90.0),
        [200.0, 120.0],
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(root, None);
            edit.reconcile_keys(scope, &[])?;
            Ok(())
        },
    )
    .unwrap();
    assert!(
        weak.upgrade().is_none(),
        "render packets retain pixels/resources, not parent metadata"
    );
    assert!(frame.hit_test(1.0, 1.0).is_some());
    assert!(pixels(frame.resolve()).chunks_exact(4).any(|p| p[0] > 0));
}
