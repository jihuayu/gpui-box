use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Clone, Default)]
struct Calls {
    width: Arc<AtomicUsize>,
    height: Arc<AtomicUsize>,
    complete: Arc<AtomicUsize>,
    plans: Arc<AtomicUsize>,
}
#[derive(Debug, Clone)]
struct Intrinsic {
    calls: Calls,
    invalid: bool,
}
impl Intrinsic {
    fn width(input: ParentLayoutInput) -> f32 {
        input
            .known_size
            .width
            .unwrap_or(match input.available.width {
                LayoutAvailableSpace::MinContent => 20.0,
                LayoutAvailableSpace::MaxContent | LayoutAvailableSpace::Unbounded => 100.0,
                LayoutAvailableSpace::Definite(w) => w.min(100.0).max(20.0),
            })
    }
}
impl ParentMeasure for Intrinsic {
    fn measure(
        &self,
        input: ParentLayoutInput,
        _: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let width = Self::width(input);
        let height = input
            .known_size
            .height
            .unwrap_or(if width < 60.0 { 40.0 } else { 20.0 });
        let size = match input.requested_axis {
            Some(LayoutAxis::Horizontal) => {
                self.calls.width.fetch_add(1, Ordering::Relaxed);
                [width, input.known_size.height.unwrap_or(0.0)]
            }
            Some(LayoutAxis::Vertical) => {
                self.calls.height.fetch_add(1, Ordering::Relaxed);
                assert!(
                    input.known_size.width.is_some(),
                    "select dependent inline size first"
                );
                [width, height]
            }
            None => {
                self.calls.complete.fetch_add(1, Ordering::Relaxed);
                [width, height]
            }
        };
        Ok(ParentMeasureOutput {
            size: if self.invalid {
                [f32::NAN, size[1]]
            } else {
                size
            },
            baseline: None,
            last_baseline: None,
        })
    }
}
impl ParentLayout for Intrinsic {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        assert_eq!(
            input.requested_axis, None,
            "complete layout must not see scalar demand"
        );
        self.calls.plans.fetch_add(1, Ordering::Relaxed);
        let value = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: value.size,
            baseline: None,
            last_baseline: None,
            placements: vec![],
        })
    }
}
fn snapshot(dry: bool) -> (ParentLayoutSnapshot, Calls) {
    let calls = Calls::default();
    let client = Intrinsic {
        calls: calls.clone(),
        invalid: false,
    };
    let value = ParentLayoutSnapshot::new(client.clone());
    (
        if dry {
            value.with_measure(client)
        } else {
            value
        },
        calls,
    )
}
fn query(width: LayoutAvailableSpace) -> ChildLayoutInput {
    ChildLayoutInput::new(LayoutConstraints::new(
        width,
        LayoutAvailableSpace::Unbounded,
    ))
}

#[test]
fn public_scalar_intrinsic_queries_skip_plans_and_dependent_height_without_publishing() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(2.0))),
        0,
    )
    .unwrap();
    let root = tree.root();
    let (client, calls) = snapshot(true);
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(client));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for (space, width) in [
        (LayoutAvailableSpace::MinContent, 24.0),
        (LayoutAvailableSpace::MaxContent, 104.0),
        (LayoutAvailableSpace::Definite(44.0), 44.0),
    ] {
        assert_eq!(
            tree.dry_measure_node_axis(root, query(space), LayoutAxis::Horizontal)
                .unwrap(),
            width
        );
    }
    assert_eq!(calls.width.load(Ordering::Relaxed), 3);
    assert_eq!(calls.complete.load(Ordering::Relaxed), 0);
    assert_eq!(calls.height.load(Ordering::Relaxed), 0);
    assert_eq!(calls.plans.load(Ordering::Relaxed), 0);
    assert_eq!(
        tree.dry_measure_node_axis(
            root,
            query(LayoutAvailableSpace::Definite(44.0)),
            LayoutAxis::Vertical
        )
        .unwrap(),
        44.0
    );
    assert!(calls.complete.load(Ordering::Relaxed) > 0);
    assert_eq!(calls.height.load(Ordering::Relaxed), 1);
    let fixed = query(LayoutAvailableSpace::MaxContent).with_known_size(
        LayoutSize::new(Some(84.0), None),
        LayoutSize::new(true, true),
    );
    assert_eq!(
        tree.dry_measure_node_axis(root, fixed, LayoutAxis::Vertical)
            .unwrap(),
        24.0
    );
    assert_eq!(calls.plans.load(Ordering::Relaxed), 0);
    let all = tree
        .dry_measure_node(root, query(LayoutAvailableSpace::Definite(44.0)))
        .unwrap();
    assert_eq!(all.size, [44.0, 44.0]);
    let baselines = tree
        .dry_measure_node_baselines(root, query(LayoutAvailableSpace::Definite(44.0)))
        .unwrap();
    assert_eq!(all, baselines);
    assert!(calls.plans.load(Ordering::Relaxed) > 0);
    assert!(tree.published_frame().is_none());
}

#[derive(Debug, Clone, Copy)]
struct Parent {
    independent: bool,
    width: f32,
}
impl ParentAllocationLayout for Parent {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![
            query(LayoutAvailableSpace::Definite(self.width));
            count
        ])
    }
}
impl ParentMeasure for Parent {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let requested = if self.independent {
            query(LayoutAvailableSpace::MaxContent)
        } else {
            query(LayoutAvailableSpace::Definite(self.width))
        };
        assert!(
            children
                .measure_axis(children.len(), requested, LayoutAxis::Horizontal)
                .is_err()
        );
        let width = children.measure_axis(0, requested, LayoutAxis::Horizontal)?;
        let height = children.measure_axis(0, requested, LayoutAxis::Vertical)?;
        Ok(ParentMeasureOutput {
            size: [
                input.known_size.width.unwrap_or(width),
                input.known_size.height.unwrap_or(height),
            ],
            baseline: None,
            last_baseline: None,
        })
    }
}
impl ParentLayout for Parent {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let value = self.measure(input, children)?;
        Ok(ParentLayoutOutput {
            size: value.size,
            baseline: None,
            last_baseline: None,
            placements: vec![ParentPlacement {
                child: 0,
                input: query(LayoutAvailableSpace::Definite(self.width)),
                origin: [0.0; 2],
            }],
        })
    }
}

#[test]
fn scalar_child_demand_survives_independent_assignments_and_normal_full_publication() {
    for independent in [false, true] {
        let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        let root = tree.root();
        let child = tree
            .append_layout(root, LayoutStyle::column(), 0xff0000ff)
            .unwrap();
        let (client, calls) = snapshot(true);
        let parent = Parent {
            independent,
            width: 44.0,
        };
        let mut value = ParentLayoutSnapshot::new(parent).with_measure(parent);
        if independent {
            value = value.with_allocation_layout(parent);
        }
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(value));
        tx.parent_layout(child, Some(client));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.local_frame([180.0; 2]).unwrap().resolve();
        assert_eq!(tree.layout_result(child).unwrap().size, [44.0, 40.0]);
        assert!(calls.width.load(Ordering::Relaxed) > 0);
        assert!(calls.height.load(Ordering::Relaxed) > 0);
        assert_eq!(calls.plans.load(Ordering::Relaxed), 1);
        let mut full = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let control = full
            .append_layout(
                full.root(),
                LayoutStyle::column().size(LayoutDimension::Px(44.0), LayoutDimension::Px(40.0)),
                0xff0000ff,
            )
            .unwrap();
        assert_eq!(
            tests::pixels(frame),
            tests::pixels(full.frame([180.0; 2]).unwrap())
        );
        assert_eq!(
            tree.layout_result(child).unwrap().size,
            full.layout_result(control).unwrap().size
        );
        let before = calls.plans.load(Ordering::Relaxed);
        tree.frame([180.0; 2]).unwrap();
        assert_eq!(calls.plans.load(Ordering::Relaxed), before);
    }
}

fn parent(independent: bool, width: f32) -> ParentLayoutSnapshot {
    let client = Parent { independent, width };
    let value = ParentLayoutSnapshot::new(client).with_measure(client);
    if independent {
        value.with_allocation_layout(client)
    } else {
        value
    }
}
fn full_reference(width: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    tree.append_layout(
        tree.root(),
        LayoutStyle::column().size(
            LayoutDimension::Px(width),
            LayoutDimension::Px(if width < 60.0 { 40.0 } else { 20.0 }),
        ),
        0xff0000ff,
    )
    .unwrap();
    tree.frame([180.0; 2]).unwrap()
}

#[test]
fn scalar_clients_update_normal_sources_keep_complete_frames_and_p0_lifetimes() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for independent in [false, true] {
        for local in [false, true] {
            let mut ui =
                LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default())
                    .unwrap();
            let root = ui.root_render();
            let scope = ui.root_scope();
            let leaf = ui
                .append_render_layout(scope, root, LayoutStyle::column(), 0xff0000ff)
                .unwrap();
            let (client, _) = snapshot(true);
            ui.edit(
                |_| Ok(()),
                |(), edit| {
                    edit.properties().parent_layout(leaf, Some(client));
                    Ok(())
                },
            )
            .unwrap();
            let source = ui
                .state(scope, 777, || Some(parent(independent, 44.0)))
                .unwrap();
            ui.bind(source, Property::parent_layout(root)).unwrap();
            let mut old = None;
            for width in [44.0, 84.0, 44.0] {
                let value = Some(parent(independent, width));
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
                let geometry = ui.layout_result(leaf).unwrap();
                assert_eq!(
                    geometry.size,
                    [width, if width < 60.0 { 40.0 } else { 20.0 }]
                );
                for axis in [LayoutAxis::Horizontal, LayoutAxis::Vertical] {
                    assert_eq!(
                        ui.dry_measure_node_axis(
                            leaf,
                            query(LayoutAvailableSpace::Definite(width)),
                            axis
                        )
                        .unwrap(),
                        axis.select(geometry.size)
                    );
                }
                assert_eq!(
                    tests::pixels(frame.clone()),
                    tests::pixels(full_reference(width))
                );
                ui.transaction(|tx| {
                    tx.set(source, Some(parent(independent, 99.0)))?;
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

#[test]
fn ordinary_flex_scalar_queries_equal_the_requested_full_extent_across_axes_and_modes() {
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        for wrap in [
            LayoutFlexWrap::NoWrap,
            LayoutFlexWrap::Wrap,
            LayoutFlexWrap::WrapReverse,
        ] {
            let mut tree = UiTree::new_layout(
                LayoutStyle::row()
                    .flex_direction(direction)
                    .flex_wrap(wrap)
                    .gap(LayoutLength::Px(3.0), LayoutLength::Px(5.0))
                    .padding(LayoutEdges::all(LayoutLength::Px(2.0))),
                0,
            )
            .unwrap();
            let root = tree.root();
            for size in [[30.0, 20.0], [40.0, 10.0], [50.0, 30.0]] {
                tree.append_layout(
                    root,
                    LayoutStyle::column()
                        .size(LayoutDimension::Px(size[0]), LayoutDimension::Px(size[1]))
                        .flex_shrink(0.0),
                    1,
                )
                .unwrap();
            }
            for space in [
                LayoutAvailableSpace::MinContent,
                LayoutAvailableSpace::MaxContent,
                LayoutAvailableSpace::Definite(75.0),
                LayoutAvailableSpace::Unbounded,
            ] {
                for mode in [LayoutSizingMode::Inherent, LayoutSizingMode::Content] {
                    let input = ChildLayoutInput::new(LayoutConstraints::new(space, space))
                        .with_sizing_mode(mode);
                    let full = tree.dry_measure_node(root, input).unwrap();
                    for axis in [LayoutAxis::Horizontal, LayoutAxis::Vertical] {
                        assert_eq!(
                            tree.dry_measure_node_axis(root, input, axis).unwrap(),
                            axis.select(full.size),
                            "{direction:?}/{wrap:?}/{space:?}/{mode:?}/{axis:?}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_scalar_parent_source_updates_match_complete_numeric_frames() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let leaf = tree
        .append_layout(root, LayoutStyle::column(), 0xff0000ff)
        .unwrap();
    let (client, _) = snapshot(true);
    let mut tx = Transaction::default();
    tx.parent_layout(leaf, Some(client));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("scalar-axis adapter: {:?}", gpu.adapter_info());
    for independent in [false, true] {
        for width in [44.0, 84.0, 44.0] {
            let mut tx = Transaction::default();
            tx.parent_layout(root, Some(parent(independent, width)));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.local_frame([180.0; 2]).unwrap();
            gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
            assert_eq!(
                gpu.image().unwrap().as_raw(),
                gpu.reference_image(&full_reference(width), [180, 180], 1.0)
                    .unwrap()
                    .as_raw()
            );
        }
    }
    gpu.finish().unwrap();
}

#[test]
fn scalar_queries_keep_complete_plan_fallback_and_errors_isolated() {
    for dry in [false, true] {
        let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        let root = tree.root();
        let (client, _) = snapshot(dry);
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(client));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            tree.dry_measure_node_axis(
                root,
                query(LayoutAvailableSpace::MinContent),
                LayoutAxis::Horizontal
            )
            .unwrap(),
            20.0
        );
        assert_eq!(
            tree.dry_measure_node_axis(
                root,
                query(LayoutAvailableSpace::Definite(40.0)),
                LayoutAxis::Vertical
            )
            .unwrap(),
            40.0
        );
    }
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let old = tree.frame([180.0; 2]).unwrap();
    let invalid = Intrinsic {
        calls: Calls::default(),
        invalid: true,
    };
    let mut tx = Transaction::default();
    tx.parent_layout(
        root,
        Some(ParentLayoutSnapshot::new(invalid.clone()).with_measure(invalid)),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(matches!(
        tree.dry_measure_node_axis(
            root,
            query(LayoutAvailableSpace::MaxContent),
            LayoutAxis::Horizontal
        ),
        Err(Error::InvalidParentLayout)
    ));
    assert_eq!(tree.published_frame().unwrap().revision, old.revision);
    let mut bad = query(LayoutAvailableSpace::MaxContent);
    bad.percentage_basis.width = Some(f32::NAN);
    assert!(
        tree.dry_measure_node_axis(root, bad, LayoutAxis::Horizontal)
            .is_err()
    );
}

#[test]
fn scalar_child_queries_keep_inline_slots_and_complete_baseline_demand_independent() {
    #[derive(Debug)]
    struct Inline;
    impl ParentInlineLayout for Inline {
        fn allocate(
            &self,
            _: ParentInlineInput,
            count: usize,
        ) -> Result<Vec<ChildInlineAllocation>, Error> {
            Ok(vec![
                ChildInlineAllocation {
                    width: 44.0,
                    percentage_basis: Some(88.0)
                };
                count
            ])
        }
    }
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let child = tree
        .append_layout(root, LayoutStyle::column(), 0xff0000ff)
        .unwrap();
    let (client, calls) = snapshot(true);
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(parent(false, 999.0).with_inline_layout(Inline)));
    tx.parent_layout(child, Some(client));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([180.0; 2]).unwrap();
    assert_eq!(tree.layout_result(child).unwrap().size, [44.0, 40.0]);
    assert!(calls.width.load(Ordering::Relaxed) > 0);
    assert!(calls.height.load(Ordering::Relaxed) > 0);
    assert_eq!(tests::pixels(frame), tests::pixels(full_reference(44.0)));
}
