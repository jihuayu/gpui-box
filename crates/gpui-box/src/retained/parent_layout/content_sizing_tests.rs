use super::*;
use crate::retained::{logical::LogicalUi, reactive::Property};

#[test]
fn content_query_does_not_resolve_the_queried_nodes_preferred_keyword() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::MinContent, LayoutDimension::Auto),
        0,
    )
    .unwrap();
    let root = tree.root();
    let mut tx = Transaction::default();
    tx.measure(
        root,
        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            Natural,
        ))),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for (mode, width) in [
        (LayoutSizingMode::Content, 100.0),
        (LayoutSizingMode::Inherent, 20.0),
        (LayoutSizingMode::Content, 100.0),
    ] {
        for complete in [false, true] {
            let input = query(mode, LayoutAvailableSpace::MaxContent);
            let result = if complete {
                tree.dry_measure_node_baselines(root, input)
            } else {
                tree.dry_measure_node(root, input)
            }
            .unwrap();
            assert_eq!(result.size, [width, 10.0]);
        }
    }
}

#[derive(Debug)]
struct Natural;
impl ContentMeasure for Natural {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        let width = match input.available.width {
            LayoutAvailableSpace::MinContent => 20.0,
            LayoutAvailableSpace::Definite(v) => v.min(100.0),
            _ => 100.0,
        };
        Ok(ContentMeasureOutput {
            size: [width, 10.0],
            baseline: Some(3.0),
            last_baseline: Some(7.0),
        })
    }
}
fn natural(tree: &mut UiTree, parent: NodeId) -> NodeId {
    let node = tree
        .append_layout(parent, LayoutStyle::column().flex_shrink(0.0), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        node,
        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            Natural,
        ))),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    node
}
fn query(mode: LayoutSizingMode, width: LayoutAvailableSpace) -> ChildLayoutInput {
    ChildLayoutInput::new(LayoutConstraints::new(
        width,
        LayoutAvailableSpace::Unbounded,
    ))
    .with_sizing_mode(mode)
}
fn authored() -> LayoutStyle {
    let mut style = LayoutStyle::column()
        .size(LayoutDimension::Px(200.0), LayoutDimension::Px(70.0))
        .min_size(LayoutDimension::Px(150.0), LayoutDimension::Px(50.0))
        .max_size(LayoutDimension::Px(250.0), LayoutDimension::Px(90.0))
        .align_items(LayoutAlignItems::Start)
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)));
    style.border = LayoutEdges::all(LayoutLength::Px(2.0));
    style.aspect_ratio = Some(2.0);
    style
}

#[test]
fn content_mode_ignores_only_queried_size_styles_across_leaf_flex_box_and_custom_hosts() {
    for kind in 0..4 {
        let style = if kind == 2 {
            let mut style = authored();
            style.display = LayoutDisplay::ConstraintBox {
                width: ConstraintFit::Loose,
                height: ConstraintFit::Loose,
            };
            style
        } else {
            authored()
        };
        let mut tree = UiTree::new_layout(style, 0).unwrap();
        let root = tree.root();
        let leaf = if kind == 0 {
            let mut tx = Transaction::default();
            tx.measure(
                root,
                Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
                    Natural,
                ))),
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            None
        } else {
            let leaf = natural(&mut tree, root);
            if kind == 3 {
                let mut tx = Transaction::default();
                tx.parent_layout(root, Some(snapshot(LayoutSizingMode::Inherent)));
                tree.commit(tx, BackendCapabilities::default()).unwrap();
            }
            Some(leaf)
        };
        for (space, width) in [
            (LayoutAvailableSpace::MaxContent, 110.0),
            (LayoutAvailableSpace::MinContent, 30.0),
            (LayoutAvailableSpace::MaxContent, 110.0),
        ] {
            let result = tree
                .dry_measure_node_baselines(root, query(LayoutSizingMode::Content, space))
                .unwrap();
            assert_eq!(result.size, [width, 20.0], "kind={kind} space={space:?}");
        }
        let inherent = tree
            .dry_measure_node_baselines(
                root,
                query(LayoutSizingMode::Inherent, LayoutAvailableSpace::MaxContent),
            )
            .unwrap();
        assert!(
            inherent.size[0] >= 150.0 && inherent.size[1] >= 50.0,
            "kind={kind}"
        );
        let bounds = query(LayoutSizingMode::Content, LayoutAvailableSpace::MaxContent)
            .with_known_size(
                LayoutSize::new(Some(60.0), None),
                LayoutSize::new(true, true),
            );
        assert_eq!(
            tree.dry_measure_node_baselines(root, bounds).unwrap().size,
            [60.0, 20.0],
            "kind={kind}"
        );
        let mut tight = query(LayoutSizingMode::Content, LayoutAvailableSpace::MaxContent);
        tight.constraints.bounds = BoxConstraints::tight([25.0, 15.0]);
        assert_eq!(
            tree.dry_measure_node_baselines(root, tight).unwrap().size,
            [25.0, 15.0],
            "kind={kind}"
        );
        if let Some(leaf) = leaf {
            let mut tx = Transaction::default();
            tx.layout_style(
                leaf,
                LayoutStyle::column().size(LayoutDimension::Px(40.0), LayoutDimension::Px(12.0)),
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            assert_eq!(
                tree.dry_measure_node_baselines(
                    root,
                    query(LayoutSizingMode::Content, LayoutAvailableSpace::MaxContent)
                )
                .unwrap()
                .size,
                [50.0, 22.0],
                "descendant styles remain authoritative kind={kind}"
            );
        }
        assert!(tree.published_frame().is_none());
    }
}

#[derive(Debug, Clone, Copy)]
struct MeasureChild(LayoutSizingMode);
impl ParentMeasure for MeasureChild {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let result = children.measure_baselines(0, query(self.0, input.available.width))?;
        if let Some(slot) = children.allocation(0)? {
            assert_eq!(slot.sizing_mode, LayoutSizingMode::Content);
        }
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
impl ParentLayout for MeasureChild {
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
                input: query(self.0, input.available.width),
                origin: [0.0; 2],
            }],
        })
    }
}
fn snapshot(mode: LayoutSizingMode) -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(MeasureChild(mode)).with_measure(MeasureChild(mode))
}

#[test]
fn intrinsic_fallback_survives_owned_parent_and_constraint_box_content_queries() {
    for custom in [false, true] {
        for kind in [LayoutContainerType::InlineSize, LayoutContainerType::Size] {
            let mut style = LayoutStyle::column()
                .container_type(kind)
                .contain_intrinsic_size(Some(60.0), Some(25.0))
                .padding(LayoutEdges::all(LayoutLength::Px(2.0)));
            if !custom {
                style.display = LayoutDisplay::ConstraintBox {
                    width: ConstraintFit::Loose,
                    height: ConstraintFit::Loose,
                };
            }
            let mut tree = UiTree::new_layout(style, 0).unwrap();
            let root = tree.root();
            let leaf = natural(&mut tree, root);
            if custom {
                let mut tx = Transaction::default();
                tx.parent_layout(root, Some(snapshot(LayoutSizingMode::Inherent)));
                tree.commit(tx, BackendCapabilities::default()).unwrap();
            }
            let expected = [
                64.0,
                if kind == LayoutContainerType::Size {
                    29.0
                } else {
                    14.0
                },
            ];
            for mode in [LayoutSizingMode::Content, LayoutSizingMode::Inherent] {
                let input = query(mode, LayoutAvailableSpace::MaxContent);
                assert_eq!(
                    tree.dry_measure_node_baselines(root, input).unwrap().size,
                    expected,
                    "{custom}/{kind:?}/{mode:?}"
                );
                let mut tight = input;
                tight.constraints.bounds = BoxConstraints::tight([40.0, 20.0]);
                assert_eq!(
                    tree.dry_measure_node_baselines(root, tight).unwrap().size,
                    [40.0, 20.0]
                );
            }
            let frame = tree.frame([300.0, 200.0]).unwrap();
            assert_eq!(tree.layout_result(root).unwrap().size, expected);
            assert!(frame.items.iter().any(|item| item.node == leaf));
            assert_eq!(tree.layout_result(root).unwrap().baseline, None);
        }
    }
}

#[test]
fn sizing_mode_survives_public_round_trip_and_owned_parent_callback_inputs() {
    for mode in [LayoutSizingMode::Content, LayoutSizingMode::Inherent] {
        let input = query(mode, LayoutAvailableSpace::MaxContent);
        assert_eq!(
            ChildLayoutInput::from_taffy(input.to_taffy().unwrap()),
            input
        );
    }
    #[derive(Debug, Clone, Copy)]
    struct Check;
    impl ParentMeasure for Check {
        fn measure(
            &self,
            input: ParentLayoutInput,
            _: &mut dyn ParentLayoutChildren,
        ) -> Result<ParentMeasureOutput, Error> {
            let natural = if input.sizing_mode == LayoutSizingMode::Content {
                [35.0, 12.0]
            } else {
                [45.0, 14.0]
            };
            Ok(ParentMeasureOutput {
                size: [
                    input.known_size.width.unwrap_or(natural[0]),
                    input.known_size.height.unwrap_or(natural[1]),
                ],
                baseline: None,
                last_baseline: None,
            })
        }
    }
    impl ParentLayout for Check {
        fn layout(
            &self,
            input: ParentLayoutInput,
            children: &mut dyn ParentLayoutChildren,
        ) -> Result<ParentLayoutOutput, Error> {
            let metrics = self.measure(input, children)?;
            Ok(ParentLayoutOutput {
                size: metrics.size,
                baseline: None,
                last_baseline: None,
                placements: vec![],
            })
        }
    }
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let mut tx = Transaction::default();
    tx.parent_layout(
        root,
        Some(ParentLayoutSnapshot::new(Check).with_measure(Check)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for (mode, expected) in [
        (LayoutSizingMode::Content, [35.0, 12.0]),
        (LayoutSizingMode::Inherent, [45.0, 14.0]),
        (LayoutSizingMode::Content, [35.0, 12.0]),
    ] {
        assert_eq!(
            tree.dry_measure_node_baselines(root, query(mode, LayoutAvailableSpace::MaxContent))
                .unwrap()
                .size,
            expected
        );
    }
}

#[test]
fn independent_full_and_inline_planners_observe_the_actual_query_sizing_mode() {
    #[derive(Debug)]
    struct Slots;
    impl ParentAllocationLayout for Slots {
        fn allocate(
            &self,
            input: ParentAllocationInput,
            count: usize,
        ) -> Result<Vec<ChildLayoutInput>, Error> {
            let width = if input.sizing_mode == LayoutSizingMode::Content {
                40.0
            } else {
                60.0
            };
            Ok(vec![
                query(
                    LayoutSizingMode::Inherent,
                    LayoutAvailableSpace::MaxContent
                )
                .with_known_size(
                    LayoutSize::new(Some(width), None),
                    LayoutSize::new(true, true)
                );
                count
            ])
        }
    }
    impl ParentInlineLayout for Slots {
        fn allocate(
            &self,
            input: ParentInlineInput,
            count: usize,
        ) -> Result<Vec<ChildInlineAllocation>, Error> {
            let width = if input.sizing_mode == LayoutSizingMode::Content {
                40.0
            } else {
                60.0
            };
            Ok(vec![
                ChildInlineAllocation {
                    width,
                    percentage_basis: Some(width)
                };
                count
            ])
        }
    }
    #[derive(Debug)]
    struct Child;
    impl ParentLayout for Child {
        fn layout(
            &self,
            input: ParentLayoutInput,
            children: &mut dyn ParentLayoutChildren,
        ) -> Result<ParentLayoutOutput, Error> {
            let slot = children
                .allocation(0)?
                .unwrap_or_else(|| query(LayoutSizingMode::Inherent, input.available.width));
            let result = children.measure_baselines(0, slot)?;
            Ok(ParentLayoutOutput {
                size: [
                    input.known_size.width.unwrap_or(result.size[0]),
                    input.known_size.height.unwrap_or(result.size[1]),
                ],
                baseline: result.baseline,
                last_baseline: result.last_baseline,
                placements: vec![ParentPlacement {
                    child: 0,
                    input: slot,
                    origin: [0.0; 2],
                }],
            })
        }
    }
    for inline in [false, true] {
        let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        let root = tree.root();
        natural(&mut tree, root);
        let value = ParentLayoutSnapshot::new(Child);
        let value = if inline {
            value.with_inline_layout(Slots)
        } else {
            value.with_allocation_layout(Slots)
        };
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(value));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        for (mode, width) in [
            (LayoutSizingMode::Content, 40.0),
            (LayoutSizingMode::Inherent, 60.0),
            (LayoutSizingMode::Content, 40.0),
        ] {
            let result = tree
                .dry_measure_node_baselines(root, query(mode, LayoutAvailableSpace::MaxContent))
                .unwrap();
            assert_eq!(result.size, [width, 10.0], "inline={inline} mode={mode:?}");
        }
    }
}

fn container(content: bool) -> LayoutStyle {
    let mut style = if content {
        LayoutStyle::column()
    } else {
        authored()
    };
    style.aspect_ratio = None;
    style.border = LayoutEdges::all(LayoutLength::Px(2.0));
    style.padding = LayoutEdges::all(LayoutLength::Px(3.0));
    style.align_items = LayoutAlignItems::Start.into();
    style.flex_shrink = 0.0;
    style
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(280.0), LayoutDimension::Px(120.0))
        .align_items(LayoutAlignItems::Start)
}
fn reference(mode: LayoutSizingMode) -> (UiTree, [NodeId; 2], FramePacket) {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            container(mode == LayoutSizingMode::Content),
            0x0000ffff,
        )
        .unwrap();
    let leaf = natural(&mut tree, host);
    let frame = tree
        .frame_in_viewport(LayoutConstraints::viewport(280.0, 120.0), [300.0, 180.0])
        .unwrap();
    (tree, [host, leaf], frame)
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [300, 180],
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
fn content_and_inherent_child_measurement_use_normal_sources_full_cpu_controls_and_lifetimes() {
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let root = ui.root_render();
        let scope = ui.root_scope();
        let host = ui
            .append_render_layout(scope, root, container(false), 0x0000ffff)
            .unwrap();
        let leaf = ui
            .append_render_layout(
                scope,
                host,
                LayoutStyle::column().flex_shrink(0.0),
                0xff0000ff,
            )
            .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().measure(
                    leaf,
                    Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
                        Natural,
                    ))),
                );
                Ok(())
            },
        )
        .unwrap();
        let value = ui
            .state(scope, 100, || Some(snapshot(LayoutSizingMode::Inherent)))
            .unwrap();
        ui.bind(value, Property::parent_layout(root)).unwrap();
        let mut old = None;
        for mode in [
            LayoutSizingMode::Inherent,
            LayoutSizingMode::Content,
            LayoutSizingMode::Inherent,
            LayoutSizingMode::Content,
        ] {
            ui.transaction(|tx| {
                tx.set(value, Some(snapshot(mode)))?;
                Ok(())
            })
            .unwrap();
            let frame = if local {
                ui.local_frame_in_viewport(
                    LayoutConstraints::viewport(280.0, 120.0),
                    [300.0, 180.0],
                )
                .unwrap()
                .resolve()
            } else {
                ui.frame_in_viewport(LayoutConstraints::viewport(280.0, 120.0), [300.0, 180.0])
                    .unwrap()
            };
            let (full, controls, reference) = reference(mode);
            for (node, control) in [host, leaf].into_iter().zip(controls) {
                assert_eq!(
                    ui.layout_result(node).unwrap(),
                    full.layout_result(control).unwrap()
                );
            }
            assert_eq!(pixels(frame.clone()), pixels(reference));
            assert_eq!(
                ui.layout_result(host).unwrap().size,
                if mode == LayoutSizingMode::Content {
                    [110.0, 20.0]
                } else {
                    [200.0, 70.0]
                }
            );
            assert!(frame.hit_test(6.0, 6.0).is_some());
            let original = ui.read(value).unwrap().clone();
            ui.transaction(|tx| {
                tx.set(value, Some(snapshot(LayoutSizingMode::Content)))?;
                tx.set(value, original)?;
                Ok(())
            })
            .unwrap();
            assert_eq!(
                ui.frame_in_viewport(LayoutConstraints::viewport(280.0, 120.0), [300.0, 180.0])
                    .unwrap()
                    .layout_stats,
                LayoutStats::default()
            );
            old.get_or_insert(frame);
        }
        let old = old.unwrap();
        let bytes = pixels(old.clone());
        drop(ui);
        assert_eq!(pixels(old), bytes);
    }
}

#[derive(Debug)]
struct Enforced;
impl ParentAllocationLayout for Enforced {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        children: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![
            query(
                LayoutSizingMode::Content,
                LayoutAvailableSpace::MaxContent
            );
            children
        ])
    }
}

#[test]
fn enforced_content_assignment_is_preserved_in_readback_measurement_and_keyed_builder() {
    let mut ui = LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    let host = ui
        .append_render_layout(scope, root, container(false), 0x0000ffff)
        .unwrap();
    ui.edit(
        |_| Ok(()),
        |(), edit| {
            edit.properties().parent_layout(
                root,
                Some(snapshot(LayoutSizingMode::Inherent).with_allocation_layout(Enforced)),
            );
            Ok(())
        },
    )
    .unwrap();
    let (_, frame) = ui
        .edit_local_frame(
            LayoutConstraints::viewport(280.0, 120.0),
            [300.0, 180.0],
            |_| Ok(()),
            |(), edit| {
                edit.allocation_builder(host, |input, edit| {
                    assert_eq!(input.sizing_mode, LayoutSizingMode::Content);
                    let branch = edit.reconcile_keys(scope, &[0])?[0];
                    let leaf = edit.append_layout(
                        branch,
                        host,
                        LayoutStyle::column().flex_shrink(0.0),
                        0xff0000ff,
                    )?;
                    edit.properties().measure(
                        leaf,
                        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
                            Natural,
                        ))),
                    );
                    Ok(())
                })
            },
        )
        .unwrap();
    assert_eq!(ui.layout_result(host).unwrap().size, [110.0, 20.0]);
    assert_eq!(
        pixels(frame.resolve()),
        pixels(reference(LayoutSizingMode::Content).2)
    );
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_content_mode_updates_match_ordinary_complete_reference() {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(root, container(false), 0x0000ffff)
        .unwrap();
    natural(&mut tree, host);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("content-sizing adapter: {:?}", gpu.adapter_info());
    for mode in [
        LayoutSizingMode::Inherent,
        LayoutSizingMode::Content,
        LayoutSizingMode::Inherent,
    ] {
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(snapshot(mode)));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .local_frame_in_viewport(LayoutConstraints::viewport(280.0, 120.0), [300.0, 180.0])
            .unwrap();
        gpu.submit_local(&frame, [300, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(mode).2, [300, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
