use super::*;

#[derive(Debug, Clone, Copy)]
struct Forward;
impl Forward {
    fn slot(input: ParentLayoutInput) -> ChildLayoutInput {
        ChildLayoutInput {
            percentage_basis: input.percentage_basis,
            ..ChildLayoutInput::default()
        }
    }
}
impl ParentMeasure for Forward {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let child = children.measure_baselines(0, Self::slot(input))?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: Some(child.size[0]),
            last_baseline: Some(child.size[1]),
        })
    }
}
impl ParentLayout for Forward {
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
                input: Self::slot(input),
                origin: [0.0; 2],
            }],
        })
    }
}
fn forward() -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Forward).with_measure(Forward)
}
fn host_style(box_protocol: bool) -> LayoutStyle {
    let style = if box_protocol {
        LayoutStyle::constraint_box(layout::ConstraintFit::Loose, layout::ConstraintFit::Loose)
    } else {
        LayoutStyle::column()
    };
    style.padding(LayoutEdges::all(LayoutLength::Px(2.0)))
}
fn leaf_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Percent(0.5), LayoutDimension::Percent(0.5))
        .padding(LayoutEdges::all(LayoutLength::Percent(0.1)))
        .flex_shrink(0.0)
}
fn metrics(tree: &mut UiTree, leaf: NodeId) {
    let mut tx = Transaction::default();
    tx.measure(
        leaf,
        Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
            width: 10.0,
            height: 12.0,
            first_baseline: Some(3.0),
            last_baseline: Some(9.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
}
fn assigned(definite: LayoutSize<bool>) -> ChildLayoutInput {
    ChildLayoutInput::default().with_known_size(LayoutSize::new(Some(84.0), Some(64.0)), definite)
}

#[test]
fn owned_parent_final_percentage_bases_preserve_selected_indefinite_width() {
    let mut tree = UiTree::new_layout(host_style(false), 0).unwrap();
    let root = tree.root();
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(forward()));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let leaf = tree.append_layout(root, leaf_style(), 1).unwrap();
    metrics(&mut tree, leaf);
    for definite in [
        LayoutSize::new(false, false),
        LayoutSize::new(true, true),
        LayoutSize::new(true, false),
        LayoutSize::new(false, true),
        LayoutSize::new(false, false),
    ] {
        let result = tree
            .dry_measure_node_baselines(root, assigned(definite))
            .unwrap();
        assert_eq!(result.size, [84.0, 64.0]);
        assert_eq!(
            result.baseline,
            Some(if definite.width { 42.0 } else { 12.0 })
        );
        assert_eq!(
            result.last_baseline,
            Some(if definite.height {
                32.0
            } else if definite.width {
                30.0
            } else {
                14.0
            })
        );
        assert!(tree.published_frame().is_none());
    }
}

#[derive(Debug, Clone, Copy)]
struct Assign(ChildLayoutInput);
impl ParentMeasure for Assign {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
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
impl ParentLayout for Assign {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let value = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: value.size,
            baseline: value.baseline,
            last_baseline: value.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: self.0,
                origin: [0.0; 2],
            }],
        })
    }
}
fn assign(slot: ChildLayoutInput) -> ParentLayoutSnapshot {
    ParentLayoutSnapshot::new(Assign(slot)).with_measure(Assign(slot))
}
fn root_style() -> LayoutStyle {
    LayoutStyle::column()
        .size(LayoutDimension::Px(100.0), LayoutDimension::Px(100.0))
        .align_items(LayoutAlignItems::Start)
}
fn reference(
    box_protocol: bool,
    definite: LayoutSize<bool>,
) -> (UiTree, NodeId, NodeId, FramePacket) {
    let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            host_style(box_protocol)
                .size(LayoutDimension::Px(84.0), LayoutDimension::Px(64.0))
                .flex_shrink(0.0),
            0x0000ffff,
        )
        .unwrap();
    if !box_protocol {
        let mut tx = Transaction::default();
        tx.parent_layout(host, Some(forward()));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
    }
    let leaf = tree
        .append_layout(
            host,
            LayoutStyle::column()
                .size(
                    LayoutDimension::Px(if definite.width { 40.0 } else { 10.0 }),
                    LayoutDimension::Px(if definite.height {
                        30.0
                    } else if definite.width {
                        28.0
                    } else {
                        12.0
                    }),
                )
                .padding(LayoutEdges::all(LayoutLength::Px(if definite.width {
                    8.0
                } else {
                    0.0
                })))
                .flex_shrink(0.0),
            0xff0000ff,
        )
        .unwrap();
    metrics(&mut tree, leaf);
    let frame = tree.frame([180.0; 2]).unwrap();
    (tree, host, leaf, frame)
}

#[test]
fn selected_definiteness_reaches_final_owned_and_constraint_box_frames_and_cpu_pixels() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        for box_protocol in [false, true] {
            let mut ui =
                LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
            let scope = ui.root_scope();
            let root = ui.root_render();
            let host = ui
                .append_render_layout(scope, root, host_style(box_protocol), 0x0000ffff)
                .unwrap();
            let leaf = ui
                .append_render_layout(scope, host, leaf_style(), 0xff0000ff)
                .unwrap();
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties().measure(
                        leaf,
                        Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                            width: 10.0,
                            height: 12.0,
                            first_baseline: Some(3.0),
                            last_baseline: Some(9.0),
                        })),
                    );
                    if !box_protocol {
                        edit.properties().parent_layout(host, Some(forward()));
                    }
                    Ok(())
                },
            )
            .unwrap();
            let source = ui
                .state(scope, 401, || {
                    Some(assign(assigned(LayoutSize::new(false, false))))
                })
                .unwrap();
            ui.bind(source, Property::parent_layout(root)).unwrap();
            let mut old = None;
            for definite in [
                LayoutSize::new(false, false),
                LayoutSize::new(true, true),
                LayoutSize::new(false, true),
                LayoutSize::new(true, false),
                LayoutSize::new(false, false),
            ] {
                let value = Some(assign(assigned(definite)));
                ui.transaction(|tx| {
                    tx.set(source, value.clone())?;
                    Ok(())
                })
                .unwrap();
                let frame = if local {
                    ui.local_frame([180.0; 2]).unwrap().resolve()
                } else {
                    ui.frame([180.0; 2]).unwrap()
                };
                let (full, a, b, control) = reference(box_protocol, definite);
                for (node, reference) in [(host, a), (leaf, b)] {
                    assert_eq!(
                        ui.layout_result(node).unwrap(),
                        full.layout_result(reference).unwrap(),
                        "{box_protocol}/{local}/{definite:?}"
                    );
                }
                assert_eq!(tests::pixels(frame.clone()), tests::pixels(control));
                ui.transaction(|tx| {
                    tx.set(
                        source,
                        Some(assign(assigned(LayoutSize::new(
                            !definite.width,
                            !definite.height,
                        )))),
                    )?;
                    tx.set(source, value)?;
                    Ok(())
                })
                .unwrap();
                let idle = ui.frame([180.0; 2]).unwrap();
                assert_eq!(idle.revision, frame.revision);
                assert_eq!(idle.layout_runs, 0);
                old.get_or_insert(frame);
            }
            let old = old.unwrap();
            let expected = tests::pixels(old.clone());
            drop(ui);
            assert_eq!(tests::pixels(old), expected);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Observe;
impl ParentMeasure for Observe {
    fn measure(
        &self,
        input: ParentLayoutInput,
        _: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        assert_eq!(
            input.known_size_is_definite.width,
            input.known_size.width.is_none() || input.percentage_basis.width.is_some()
        );
        assert_eq!(
            input.known_size_is_definite.height,
            input.known_size.height.is_none() || input.percentage_basis.height.is_some()
        );
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(20.0),
                input.known_size.height.unwrap_or(10.0),
            ],
            baseline: Some(if input.known_size_is_definite.width {
                1.0
            } else {
                2.0
            }),
            last_baseline: Some(if input.known_size_is_definite.height {
                3.0
            } else {
                4.0
            }),
        })
    }
}
impl ParentLayout for Observe {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let value = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: value.size,
            baseline: value.baseline,
            last_baseline: value.last_baseline,
            placements: vec![],
        })
    }
}
impl ParentInlineLayout for Observe {
    fn allocate(
        &self,
        input: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        assert_eq!(count, 0);
        assert_eq!(
            input.known_width_is_definite,
            input.known_width.is_none() || input.percentage_basis.is_some()
        );
        Ok(vec![])
    }
}

#[test]
fn measured_and_inline_callbacks_observe_known_size_provenance_and_tight_overrides() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let mut tx = Transaction::default();
    tx.parent_layout(
        root,
        Some(
            ParentLayoutSnapshot::new(Observe)
                .with_measure(Observe)
                .with_inline_layout(Observe),
        ),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for definite in [
        LayoutSize::new(false, false),
        LayoutSize::new(true, false),
        LayoutSize::new(false, true),
        LayoutSize::new(true, true),
        LayoutSize::new(false, false),
    ] {
        let input = assigned(definite);
        let result = tree.dry_measure_node_baselines(root, input).unwrap();
        assert_eq!(
            result.baseline,
            Some(if definite.width { 1.0 } else { 2.0 })
        );
        assert_eq!(
            result.last_baseline,
            Some(if definite.height { 3.0 } else { 4.0 })
        );
        let mut tight = input;
        tight.constraints.bounds = BoxConstraints::tight([84.0, 64.0]);
        let result = tree.dry_measure_node_baselines(root, tight).unwrap();
        assert_eq!(result.baseline, Some(1.0));
        assert_eq!(result.last_baseline, Some(3.0));
    }
    let none = ChildLayoutInput::default()
        .with_known_size(LayoutSize::new(None, None), LayoutSize::new(false, false));
    let result = tree.dry_measure_node_baselines(root, none).unwrap();
    assert_eq!(result.size, [20.0, 10.0]);
    assert_eq!(result.baseline, Some(1.0));
    assert_eq!(result.last_baseline, Some(4.0));
    assert!(tree.published_frame().is_none());
}

#[derive(Debug)]
struct RequireDefinite;
impl ParentMeasure for RequireDefinite {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        if input.known_size.width.is_some() && !input.known_size_is_definite.width {
            return Err(Error::InvalidParentLayout);
        }
        Forward.measure(input, children)
    }
}
impl ParentLayout for RequireDefinite {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        self.measure(input, children)?;
        Forward.layout(input, children)
    }
}

#[test]
fn observed_definiteness_policy_error_rolls_back_atomic_source_layout_and_frame() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        let mut ui =
            LogicalUi::new_layout(root_style(), 0, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        let host = ui
            .append_render_layout(scope, root, host_style(false), 0x0000ffff)
            .unwrap();
        ui.append_render_layout(
            scope,
            host,
            LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Px(12.0)),
            0xff0000ff,
        )
        .unwrap();
        ui.edit(
            |_| Ok(()),
            |(), edit| {
                edit.properties().parent_layout(
                    host,
                    Some(ParentLayoutSnapshot::new(RequireDefinite).with_measure(RequireDefinite)),
                );
                Ok(())
            },
        )
        .unwrap();
        let value = Some(assign(assigned(LayoutSize::new(true, true))));
        let source = ui.state(scope, 402, || value.clone()).unwrap();
        ui.bind(source, Property::parent_layout(root)).unwrap();
        let old = ui.frame([180.0; 2]).unwrap();
        let geometry = ui.layout_result(host).unwrap();
        let stats = ui.stats();
        let sources = |tx: &mut crate::retained::reactive::ReactiveTransaction<'_>| {
            tx.set(source, Some(assign(assigned(LayoutSize::new(false, true)))))?;
            Ok(())
        };
        let result = if local {
            ui.edit_local_frame(
                LayoutConstraints::viewport(180.0, 180.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        } else {
            ui.edit_frame(
                LayoutConstraints::viewport(180.0, 180.0),
                [180.0; 2],
                sources,
                |(), _| Ok(()),
            )
            .map(|_| ())
        };
        assert!(matches!(result, Err(Error::InvalidParentLayout)));
        assert_eq!(ui.read(source).unwrap(), &value);
        assert_eq!(ui.stats(), stats);
        assert_eq!(ui.layout_result(host).unwrap(), geometry);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        let next = ui.frame([180.0; 2]).unwrap();
        assert_eq!(next.layout_runs, 0);
        assert_eq!(tests::pixels(next), tests::pixels(old));
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_owned_parent_definiteness_matches_independent_numeric_size_controls() {
    for box_protocol in [false, true] {
        let mut tree = UiTree::new_layout(root_style(), 0).unwrap();
        let root = tree.root();
        let host = tree
            .append_layout(root, host_style(box_protocol), 0x0000ffff)
            .unwrap();
        let leaf = tree.append_layout(host, leaf_style(), 0xff0000ff).unwrap();
        metrics(&mut tree, leaf);
        if !box_protocol {
            let mut tx = Transaction::default();
            tx.parent_layout(host, Some(forward()));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let mut renderer = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
        eprintln!("owned-definiteness adapter: {:?}", renderer.adapter_info());
        for definite in [
            LayoutSize::new(false, false),
            LayoutSize::new(true, true),
            LayoutSize::new(false, true),
            LayoutSize::new(true, false),
            LayoutSize::new(false, false),
        ] {
            let mut tx = Transaction::default();
            tx.parent_layout(root, Some(assign(assigned(definite))));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([180.0; 2]).unwrap();
            renderer
                .submit_local(&frame, [180, 180], 1.0)
                .unwrap()
                .unwrap();
            let control = reference(box_protocol, definite).3;
            assert_eq!(
                renderer.image().unwrap().as_raw(),
                renderer
                    .reference_image(&control, [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
        renderer.finish().unwrap();
    }
}
