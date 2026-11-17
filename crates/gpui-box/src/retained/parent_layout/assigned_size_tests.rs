use super::*;
use crate::retained::{logical::LogicalUi, reactive::Property};

#[test]
fn selected_size_and_percentage_definiteness_survive_public_input_round_trip() {
    for definite in [false, true] {
        let input = taffy::LayoutInput {
            known_dimensions: Size {
                width: Some(70.0),
                height: Some(40.0),
            },
            known_dimensions_are_definite: Size {
                width: true,
                height: definite,
            },
            available_space: Size {
                width: taffy::AvailableSpace::Definite(200.0),
                height: taffy::AvailableSpace::Unbounded,
            },
            parent_size: Size {
                width: Some(300.0),
                height: None,
            },
            ..taffy::LayoutInput::HIDDEN
        };
        let public = ChildLayoutInput::from_taffy(input);
        let back = public.to_taffy().unwrap();
        assert_eq!(back.known_dimensions, input.known_dimensions);
        assert_eq!(
            back.known_dimensions_are_definite,
            input.known_dimensions_are_definite
        );
        assert_eq!(back.size_bounds, input.size_bounds);
        assert_eq!(back.parent_size, input.parent_size);
        assert_eq!(back.available_space, input.available_space);
    }
}

fn assignment(definite: bool) -> ChildLayoutInput {
    ChildLayoutInput::new(LayoutConstraints::new(
        LayoutAvailableSpace::Definite(180.0),
        LayoutAvailableSpace::Unbounded,
    ))
    .with_known_size(
        LayoutSize::new(Some(80.0), Some(60.0)),
        LayoutSize::new(true, definite),
    )
}

#[test]
fn selected_input_validation_keeps_hard_intervals_and_tight_definiteness_authoritative() {
    let bounds = BoxConstraints::new([20.0, 10.0], [100.0, 80.0]);
    for invalid in [f32::NAN, f32::INFINITY, -1.0, 19.0, 101.0] {
        let mut input = assignment(false);
        input.constraints.bounds = bounds;
        input.known_size.width = Some(invalid);
        assert!(matches!(input.to_taffy(), Err(Error::InvalidParentLayout)));
    }
    let mut input = assignment(false);
    input.constraints.bounds = BoxConstraints::tight([80.0, 60.0]);
    assert!(
        input
            .to_taffy()
            .unwrap()
            .known_dimensions_are_definite
            .height
    );
    input.known_size.height = Some(50.0);
    assert!(matches!(input.to_taffy(), Err(Error::InvalidParentLayout)));
    let mut input = assignment(false);
    input.known_size = LayoutSize::new(None, None);
    input.known_size_is_definite = LayoutSize::new(false, false);
    input.constraints.bounds = BoxConstraints::tight([25.0, 15.0]);
    let kernel = input.to_taffy().unwrap();
    assert_eq!(
        kernel.known_dimensions,
        Size {
            width: Some(25.0),
            height: Some(15.0)
        }
    );
    assert_eq!(
        kernel.known_dimensions_are_definite,
        Size {
            width: true,
            height: true
        }
    );
    assert_eq!(
        ChildLayoutInput::default(),
        ChildLayoutInput::new(LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        ))
    );
    let input = ChildLayoutInput::default()
        .with_known_size(LayoutSize::new(None, None), LayoutSize::new(false, false));
    assert_eq!(
        input.to_taffy().unwrap().known_dimensions_are_definite,
        Size {
            width: true,
            height: true
        },
        "definiteness flags on absent dimensions have no effect"
    );
    let inline = ChildInlineAllocation {
        width: 25.0,
        percentage_basis: None,
    };
    let mut input = assignment(false);
    input.known_size_is_definite.width = false;
    let result = inline.apply(input).to_taffy().unwrap();
    assert_eq!(
        result.known_dimensions,
        Size {
            width: Some(25.0),
            height: Some(60.0)
        }
    );
    assert_eq!(
        result.known_dimensions_are_definite,
        Size {
            width: true,
            height: false
        }
    );
}

fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
        .align_items(LayoutAlignItems::Start)
}
fn add_percentage_leaf(tree: &mut UiTree, host: NodeId) -> NodeId {
    let child = tree
        .append_layout(
            host,
            LayoutStyle::column()
                .size(LayoutDimension::Px(10.0), LayoutDimension::Percent(0.5))
                .flex_shrink(0.0),
            0xff0000ff,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        child,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 10.0,
            height: 10.0,
            baseline: Some(3.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    child
}

#[derive(Debug)]
struct Assigned {
    slot: ChildLayoutInput,
    check_incoming: bool,
}
impl ParentAllocationLayout for Assigned {
    fn allocate(
        &self,
        input: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        if self.check_incoming {
            assert_eq!(input.known_size, self.slot.known_size);
            assert_eq!(
                input.known_size_is_definite,
                self.slot.known_size_is_definite
            );
            assert_eq!(input.constraints, self.slot.constraints);
            assert_eq!(input.percentage_basis, self.slot.percentage_basis);
        }
        Ok(vec![self.slot; count])
    }
}
impl ParentMeasure for Assigned {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        assert_eq!(children.len(), 1);
        assert_eq!(children.allocation(0)?, Some(self.slot));
        // Conflicting caller input must not replace any independently assigned field.
        let result = children.measure_baselines(
            0,
            ChildLayoutInput::new(LayoutConstraints::viewport(999.0, 999.0)),
        )?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(result.size[0]),
                input.known_size.height.unwrap_or(result.size[1]),
            ],
            baseline: result.baseline,
            last_baseline: result.last_baseline,
        })
    }
}
impl ParentLayout for Assigned {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let metrics = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: metrics.size,
            baseline: metrics.baseline,
            last_baseline: metrics.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: ChildLayoutInput::new(LayoutConstraints::viewport(999.0, 999.0)),
                origin: [0.0; 2],
            }],
        })
    }
}
fn snapshot(slot: ChildLayoutInput, nested: bool) -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Assigned {
        slot,
        check_incoming: nested,
    })
    .with_measure(Assigned {
        slot,
        check_incoming: nested,
    })
    .with_allocation_layout(Assigned {
        slot,
        check_incoming: nested,
    })
}
fn reference(definite: bool, nested: bool) -> (UiTree, Vec<NodeId>, FramePacket) {
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let mut nodes = vec![];
    let mut parent = tree.root();
    for _ in 0..if nested { 2 } else { 1 } {
        parent = tree
            .append_layout(parent, fixed(80.0, 60.0), 0x0000ffff)
            .unwrap();
        nodes.push(parent);
    }
    let leaf = add_percentage_leaf(&mut tree, parent);
    let mut tx = Transaction::default();
    tx.layout_style(leaf, fixed(10.0, if definite { 30.0 } else { 10.0 }));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    nodes.push(leaf);
    let frame = tree
        .frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
        .unwrap();
    (tree, nodes, frame)
}

#[test]
fn selected_indefinite_height_survives_nested_allocation_and_normal_source_updates() {
    for local in [false, true] {
        for nested in [false, true] {
            let mut ui =
                LogicalUi::new_layout(fixed(100.0, 100.0), 0, BackendCapabilities::default())
                    .unwrap();
            let root = ui.root_render();
            let scope = ui.root_scope();
            let mut nodes = vec![];
            let mut host = root;
            for _ in 0..if nested { 2 } else { 1 } {
                host = ui
                    .append_render_layout(
                        scope,
                        host,
                        LayoutStyle::column().align_items(LayoutAlignItems::Start),
                        0x0000ffff,
                    )
                    .unwrap();
                nodes.push(host);
            }
            let leaf = ui
                .append_render_layout(
                    scope,
                    host,
                    LayoutStyle::column()
                        .size(LayoutDimension::Px(10.0), LayoutDimension::Percent(0.5))
                        .flex_shrink(0.0),
                    0xff0000ff,
                )
                .unwrap();
            nodes.push(leaf);
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties().measure(
                        leaf,
                        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                            width: 10.0,
                            height: 10.0,
                            baseline: Some(3.0),
                        })),
                    );
                    Ok(())
                },
            )
            .unwrap();
            let value = ui
                .state(scope, 100, || Some(snapshot(assignment(false), false)))
                .unwrap();
            ui.bind(value, Property::parent_layout(root)).unwrap();
            let inner = nested.then(|| {
                ui.state(scope, 101, || Some(snapshot(assignment(false), true)))
                    .unwrap()
            });
            if let Some(source) = inner {
                ui.bind(source, Property::parent_layout(nodes[0])).unwrap();
            }
            let mut old = None;
            for definite in [false, true, false, true] {
                ui.transaction(|tx| {
                    tx.set(value, Some(snapshot(assignment(definite), false)))?;
                    if let Some(source) = inner {
                        tx.set(source, Some(snapshot(assignment(definite), true)))?;
                    }
                    Ok(())
                })
                .unwrap();
                let frame = if local {
                    ui.local_frame_in_viewport(
                        LayoutConstraints::viewport(100.0, 100.0),
                        [180.0; 2],
                    )
                    .unwrap()
                    .resolve()
                } else {
                    ui.frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
                        .unwrap()
                };
                assert_eq!(ui.layout_result(host).unwrap().size, [80.0, 60.0]);
                assert_eq!(
                    ui.layout_result(leaf).unwrap().size[1],
                    if definite { 30.0 } else { 10.0 }
                );
                let (full, controls, reference) = reference(definite, nested);
                for (&node, control) in nodes.iter().zip(controls) {
                    assert_eq!(
                        ui.layout_result(node).unwrap(),
                        full.layout_result(control).unwrap()
                    );
                }
                assert_eq!(tests::pixels(frame.clone()), tests::pixels(reference));
                assert!(frame.hit_test(1.0, 1.0).is_some());
                let again = ui
                    .frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
                    .unwrap();
                assert_eq!(again.layout_stats, LayoutStats::default());
                let original = ui.read(value).unwrap().clone();
                ui.transaction(|tx| {
                    tx.set(value, Some(snapshot(assignment(!definite), false)))?;
                    tx.set(value, original)?;
                    Ok(())
                })
                .unwrap();
                assert_eq!(
                    ui.frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
                        .unwrap()
                        .layout_stats,
                    LayoutStats::default()
                );
                old.get_or_insert(frame);
            }
            let old = old.unwrap();
            let pixels = tests::pixels(old.clone());
            drop(ui);
            assert_eq!(tests::pixels(old), pixels);
        }
    }
}

#[test]
fn public_dry_queries_distinguish_selected_size_percentage_base_and_soft_space() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().justify_content(LayoutJustifyContent::End),
        0,
    )
    .unwrap();
    let root = tree.root();
    add_percentage_leaf(&mut tree, root);
    for definite in [false, true, false] {
        let result = tree
            .dry_measure_node_baselines(root, assignment(definite))
            .unwrap();
        assert_eq!(result.size, [80.0, 60.0]);
        // End alignment reveals the percentage leaf's 10px vs 30px block size
        // through the final baseline, even though the selected host size is equal.
        assert_eq!(result.baseline, Some(if definite { 33.0 } else { 53.0 }));
    }
    assert!(tree.published_frame().is_none());
    let parent = tree
        .append_layout(
            root,
            LayoutStyle::column().size(LayoutDimension::Percent(0.5), LayoutDimension::Auto),
            0,
        )
        .unwrap();
    let query = ChildLayoutInput {
        percentage_basis: LayoutSize::new(Some(160.0), None),
        ..ChildLayoutInput::new(LayoutConstraints::new(
            LayoutAvailableSpace::MaxContent,
            LayoutAvailableSpace::Unbounded,
        ))
    };
    assert_eq!(tree.dry_measure_node(parent, query).unwrap().size[0], 80.0);
    assert_eq!(
        tree.dry_measure_node(
            parent,
            query.with_known_size(
                LayoutSize::new(Some(40.0), None),
                LayoutSize::new(true, true)
            )
        )
        .unwrap()
        .size[0],
        40.0
    );
}

#[derive(Debug, Clone, Copy)]
struct MeasuredSlot(ChildLayoutInput);
impl ParentMeasure for MeasuredSlot {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        assert_eq!(children.allocation(0)?, None);
        let child = children.measure_baselines(0, self.0)?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: child.baseline,
            last_baseline: child.last_baseline,
        })
    }
}
impl ParentLayout for MeasuredSlot {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let metrics = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: metrics.size,
            baseline: metrics.baseline,
            last_baseline: metrics.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: self.0,
                origin: [0.0; 2],
            }],
        })
    }
}

#[test]
fn measured_parent_selected_slots_and_dry_metrics_match_complete_reference() {
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(
            root,
            LayoutStyle::column().align_items(LayoutAlignItems::Start),
            0x0000ffff,
        )
        .unwrap();
    let leaf = add_percentage_leaf(&mut tree, host);
    for definite in [false, true, false] {
        let client = MeasuredSlot(assignment(definite));
        let mut tx = Transaction::default();
        tx.parent_layout(
            root,
            Some(ParentLayoutSnapshot::new(client).with_measure(client)),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
            .unwrap();
        let query = tree
            .dry_measure_node_baselines(
                root,
                ChildLayoutInput::new(LayoutConstraints::viewport(100.0, 100.0)),
            )
            .unwrap();
        let (full, controls, reference) = reference(definite, false);
        assert_eq!(query.size, [100.0; 2]);
        assert_eq!(query.baseline, tree.layout_result(root).unwrap().baseline);
        for (node, control) in [host, leaf].into_iter().zip(controls) {
            assert_eq!(
                tree.layout_result(node).unwrap(),
                full.layout_result(control).unwrap()
            );
        }
        assert_eq!(tests::pixels(frame), tests::pixels(reference));
    }
}

#[test]
fn selected_input_builders_observe_complete_assignments_and_rollback_invalid_sources() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(fixed(100.0, 100.0), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(
                scope,
                root,
                LayoutStyle::column().align_items(LayoutAlignItems::Start),
                0x0000ffff,
            )
            .unwrap();
        let value = ui
            .state(scope, 200, || Some(snapshot(assignment(false), false)))
            .unwrap();
        ui.bind(value, Property::parent_layout(root)).unwrap();
        let mut old = None;
        for definite in [false, true, false] {
            let slot = assignment(definite);
            let render = |(), edit: &mut crate::retained::logical::LogicalEdit<'_, '_, '_>| {
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.known_size, slot.known_size);
                    assert_eq!(input.known_size_is_definite, slot.known_size_is_definite);
                    assert_eq!(input.constraints.bounds, BoxConstraints::UNBOUNDED);
                    assert_eq!(
                        input.constraints.width,
                        LayoutAvailableSpace::Definite(180.0)
                    );
                    assert_eq!(input.percentage_basis, LayoutSize::new(None, None));
                    let branch = edit.reconcile_keys(scope, &[u64::from(definite)])?[0];
                    if edit.renders(branch)?.is_empty() {
                        edit.append_layout(
                            branch,
                            host,
                            fixed(10.0, if definite { 30.0 } else { 10.0 }),
                            0xff0000ff,
                        )?;
                    }
                    Ok(())
                })
            };
            let sources = |tx: &mut crate::retained::reactive::ReactiveTransaction<'_>| {
                tx.set(value, Some(snapshot(slot, false)))?;
                Ok(())
            };
            let frame = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(100.0, 100.0),
                    [180.0; 2],
                    sources,
                    render,
                )
                .unwrap()
                .1
                .resolve()
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(100.0, 100.0),
                    [180.0; 2],
                    sources,
                    render,
                )
                .unwrap()
                .1
            };
            let actual = tests::pixels(frame.clone());
            let reference = reference(definite, false).2;
            assert_eq!(ui.layout_result(host).unwrap().size, [80.0, 60.0]);
            assert!(
                frame
                    .hit_test(1.0, if definite { 29.0 } else { 9.0 })
                    .is_some()
            );
            assert_eq!(actual, tests::pixels(reference));
            old = Some(frame);
        }
        let before = ui.stats();
        let source_before = ui.read(value).unwrap().clone();
        let revision = old.unwrap().revision;
        let mut invalid = assignment(false);
        invalid.known_size.width = Some(f32::NAN);
        let sources = |tx: &mut crate::retained::reactive::ReactiveTransaction<'_>| {
            tx.set(value, Some(snapshot(invalid, false)))?;
            Ok(())
        };
        let result = if local {
            ui.edit_local_frame(
                LayoutConstraints::viewport(100.0, 100.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        } else {
            ui.edit_frame(
                LayoutConstraints::viewport(100.0, 100.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        };
        assert!(matches!(result, Err(Error::InvalidParentLayout)));
        assert_eq!(ui.read(value).unwrap(), &source_before);
        assert_eq!(ui.stats(), before);
        assert_eq!(ui.published_frame().unwrap().revision, revision);
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_selected_size_and_definiteness_updates_match_complete_flex_reference() {
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(
            root,
            LayoutStyle::column().align_items(LayoutAlignItems::Start),
            0x0000ffff,
        )
        .unwrap();
    let leaf = add_percentage_leaf(&mut tree, host);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("selected-size adapter: {:?}", gpu.adapter_info());
    for definite in [false, true, false] {
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(snapshot(assignment(definite), false)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .local_frame_in_viewport(LayoutConstraints::viewport(100.0, 100.0), [180.0; 2])
            .unwrap();
        assert_eq!(
            tree.layout_result(leaf).unwrap().size[1],
            if definite { 30.0 } else { 10.0 }
        );
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(definite, false).2, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
