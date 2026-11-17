use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Debug, Default)]
struct Counts {
    dry: Arc<AtomicUsize>,
    plans: Arc<AtomicUsize>,
}
#[derive(Clone, Debug)]
struct Stack(Counts);
fn slot(input: ParentLayoutInput) -> ChildLayoutInput {
    ChildLayoutInput {
        constraints: LayoutConstraints::new(input.available.width, LayoutAvailableSpace::Unbounded)
            .with_bounds(
                input
                    .known_size
                    .width
                    .map(|width| BoxConstraints::new([width, 0.0], [width, f32::INFINITY]))
                    .unwrap_or(BoxConstraints::UNBOUNDED),
            ),
        percentage_basis: input.percentage_basis,
        ..ChildLayoutInput::default()
    }
}
fn metrics(
    input: ParentLayoutInput,
    children: &mut dyn ParentLayoutChildren,
) -> Result<ParentMeasureOutput, Error> {
    let mut width: f32 = 0.0;
    let mut height = 0.0;
    let mut baseline = None;
    let mut last = None;
    for child in 0..children.len() {
        let result = children.measure(child, slot(input))?;
        if child == 0 {
            baseline = result.baseline;
        }
        if let Some(value) = result.last_baseline {
            last = Some(height + value);
        }
        width = width.max(result.size[0]);
        height += result.size[1];
    }
    let height = input.known_size.height.unwrap_or(height);
    Ok(ParentMeasureOutput {
        size: [input.known_size.width.unwrap_or(width), height],
        baseline: baseline.filter(|v| *v <= height),
        last_baseline: last.filter(|v| *v <= height),
    })
}
impl ParentMeasure for Stack {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        self.0.dry.fetch_add(1, Ordering::Relaxed);
        metrics(input, children)
    }
}
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        self.0.plans.fetch_add(1, Ordering::Relaxed);
        let metrics = metrics(input, children)?;
        let mut y = 0.0;
        let mut placements = vec![];
        for child in 0..children.len() {
            let result = children.measure(child, slot(input))?;
            placements.push(ParentPlacement {
                child,
                input: slot(input),
                origin: [0.0, y],
            });
            y += result.size[1];
        }
        Ok(ParentLayoutOutput {
            size: metrics.size,
            baseline: metrics.baseline,
            last_baseline: metrics.last_baseline,
            placements,
        })
    }
}
fn snapshot(dry: bool) -> (ParentLayoutSnapshot, Counts) {
    let counts = Counts::default();
    let stack = Stack(counts.clone());
    let mut value = ParentLayoutSnapshot::new(stack.clone());
    if dry {
        value = value.with_measure(stack);
    }
    (value, counts)
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
fn fixture(dry: bool) -> (UiTree, NodeId, Counts) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
    )
    .unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let row = tree
        .append_layout(host, LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap), 0)
        .unwrap();
    for _ in 0..3 {
        tree.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
            .unwrap();
    }
    tree.append_layout(
        host,
        fixed(10.0, 8.0).size(LayoutDimension::Percent(1.0), LayoutDimension::Px(8.0)),
        0x00ff00ff,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(10.0, 10.0), 0x0000ffff)
        .unwrap();
    let (value, counts) = snapshot(dry);
    let mut tx = Transaction::default();
    tx.parent_layout(host, Some(value));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    (tree, host, counts)
}
fn reference(w: f32) -> FramePacket {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().size(LayoutDimension::Percent(1.0), LayoutDimension::Auto),
        0,
    )
    .unwrap();
    tree.set_frame_build_policy(FrameBuildPolicy::Full);
    let host = tree
        .append_layout(
            tree.root(),
            LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let row = tree
        .append_layout(host, LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap), 0)
        .unwrap();
    for _ in 0..3 {
        tree.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
            .unwrap();
    }
    tree.append_layout(
        host,
        fixed(10.0, 8.0).size(LayoutDimension::Percent(1.0), LayoutDimension::Px(8.0)),
        0x00ff00ff,
    )
    .unwrap();
    tree.append_layout(tree.root(), fixed(10.0, 10.0), 0x0000ffff)
        .unwrap();
    tree.frame_in_viewport(constraints(w), [180.0; 2]).unwrap()
}

#[test]
fn dry_size_queries_reduce_complete_plans_and_match_ordinary_full_flex_pixels() {
    for local in [false, true] {
        let (mut dry, host, counts) = fixture(true);
        let (mut legacy, _, control) = fixture(false);
        let mut old = None;
        for width in [90.0, 50.0, 130.0, 90.0] {
            let frame = if local {
                dry.local_frame_in_viewport(constraints(width), [180.0; 2])
                    .unwrap()
                    .resolve()
            } else {
                dry.frame_in_viewport(constraints(width), [180.0; 2])
                    .unwrap()
            };
            let other = legacy
                .frame_in_viewport(constraints(width), [180.0; 2])
                .unwrap();
            let full = reference(width);
            let actual = super::tests::pixels(frame.clone());
            assert_eq!(actual, super::tests::pixels(full));
            assert_eq!(actual, super::tests::pixels(other));
            assert!(dry.layout_result(host).unwrap().size[1] >= 28.0);
            assert!(frame.hit_test(6.0, 6.0).is_some());
            old.get_or_insert((frame, actual));
        }
        assert!(counts.dry.load(Ordering::Relaxed) > 0);
        assert_eq!(
            counts.plans.load(Ordering::Relaxed),
            4,
            "one complete plan per final host layout"
        );
        assert!(control.plans.load(Ordering::Relaxed) > counts.plans.load(Ordering::Relaxed));
        eprintln!(
            "local={local}: complete plans dry={} legacy={}, dry metric queries={}",
            counts.plans.load(Ordering::Relaxed),
            control.plans.load(Ordering::Relaxed),
            counts.dry.load(Ordering::Relaxed)
        );
        let before = counts.plans.load(Ordering::Relaxed);
        dry.frame_in_viewport(constraints(90.0), [180.0; 2])
            .unwrap();
        assert_eq!(counts.plans.load(Ordering::Relaxed), before);
        drop(dry);
        let (frame, expected) = old.unwrap();
        assert_eq!(super::tests::pixels(frame), expected);
    }
}

#[derive(Debug)]
struct Bad(u8);
impl ParentMeasure for Bad {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let mut value = metrics(input, children)?;
        match self.0 {
            0 => value.size[0] = f32::NAN,
            1 => value.size[1] = -1.0,
            2 => value.baseline = Some(f32::NEG_INFINITY),
            3 => value.last_baseline = Some(f32::INFINITY),
            4 => value.size[0] += 1.0,
            5 => return Err(Error::InvalidParentLayout),
            _ => value.last_baseline = Some(1.0),
        }
        Ok(value)
    }
}
#[test]
fn dry_invalid_metrics_and_final_baseline_disagreement_reject_without_publishing() {
    use crate::retained::logical::LogicalUi;
    for mode in 0..7 {
        let mut ui =
            LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default())
                .unwrap();
        let scope = ui.root_scope();
        let root = ui.root_render();
        ui.append_render_layout(scope, root, fixed(20.0, 10.0), 0xff0000ff)
            .unwrap();
        let old = ui.frame([80.0, 60.0]).unwrap();
        let source = ui.state(scope, 0, || 0u32).unwrap();
        let result = ui.edit_frame(
            LayoutConstraints::viewport(80.0, 60.0),
            [80.0, 60.0],
            |tx| {
                tx.set(source, 1)?;
                Ok(())
            },
            |(), edit| {
                edit.properties().parent_layout(
                    root,
                    Some(
                        ParentLayoutSnapshot::new(Stack(Counts::default())).with_measure(Bad(mode)),
                    ),
                );
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(Error::InvalidParentLayout)),
            "mode {mode}"
        );
        assert_eq!(*ui.read(source).unwrap(), 0);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
    }
}

#[test]
fn public_dry_query_has_no_complete_plan_or_live_cache_geometry_publication() {
    let (mut tree, host, counts) = fixture(true);
    let query = |space| ChildLayoutInput {
        constraints: LayoutConstraints::new(space, LayoutAvailableSpace::Unbounded),
        percentage_basis: LayoutSize::new(Some(90.0), None),
        ..ChildLayoutInput::default()
    };
    let min = tree
        .dry_measure_node(host, query(LayoutAvailableSpace::MinContent))
        .unwrap();
    let max = tree
        .dry_measure_node(host, query(LayoutAvailableSpace::MaxContent))
        .unwrap();
    assert_eq!(min.size, [50.0, 48.0]);
    assert_eq!(max.size, [130.0, 28.0]);
    assert_eq!(counts.plans.load(Ordering::Relaxed), 0);
    assert!(tree.published_frame().is_none());
    let old = tree
        .frame_in_viewport(constraints(90.0), [180.0; 2])
        .unwrap();
    let geometry = tree.layout_result(host).unwrap();
    let plans = counts.plans.load(Ordering::Relaxed);
    let revision = tree.revision;
    for _ in 0..2 {
        assert_eq!(
            tree.dry_measure_node(host, query(LayoutAvailableSpace::MinContent))
                .unwrap(),
            min
        );
        assert_eq!(
            tree.dry_measure_node(host, query(LayoutAvailableSpace::MaxContent))
                .unwrap(),
            max
        );
    }
    assert_eq!(counts.plans.load(Ordering::Relaxed), plans);
    assert_eq!(tree.revision, revision);
    assert_eq!(tree.layout_result(host).unwrap(), geometry);
    assert_eq!(tree.published_frame().unwrap().revision, old.revision);
    let again = tree
        .frame_in_viewport(constraints(90.0), [180.0; 2])
        .unwrap();
    assert_eq!(again.layout_runs, 0);
    assert_eq!(counts.plans.load(Ordering::Relaxed), plans);
}

#[test]
fn dry_query_preserves_percentage_bases_hard_bounds_hidden_and_invalid_inputs() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let host = tree
        .append_layout(tree.root(), LayoutStyle::column(), 0)
        .unwrap();
    tree.append_layout(
        host,
        LayoutStyle::row().size(LayoutDimension::Percent(0.5), LayoutDimension::Percent(0.5)),
        1,
    )
    .unwrap();
    let (value, counts) = snapshot(true);
    let mut tx = Transaction::default();
    tx.parent_layout(host, Some(value));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    for h in [None, Some(40.0), Some(80.0), None] {
        let result = tree
            .dry_measure_node(
                host,
                ChildLayoutInput {
                    constraints: LayoutConstraints::new(
                        LayoutAvailableSpace::MaxContent,
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([80.0, 0.0], [80.0, f32::INFINITY])),
                    percentage_basis: LayoutSize::new(Some(500.0), h),
                    ..ChildLayoutInput::default()
                },
            )
            .unwrap();
        // Only the host's assigned width establishes its content percentage
        // basis. Its incoming height basis does not establish a host height.
        assert_eq!(result.size, [80.0, 0.0]);
    }
    assert_eq!(counts.plans.load(Ordering::Relaxed), 0);
    let bad = ChildLayoutInput {
        constraints: LayoutConstraints::viewport(80.0, 60.0),
        percentage_basis: LayoutSize::new(Some(f32::NAN), None),
        ..ChildLayoutInput::default()
    };
    assert!(matches!(
        tree.dry_measure_node(host, bad),
        Err(Error::InvalidParentLayout)
    ));
    let mut hidden = Transaction::default();
    hidden.layout_style(host, LayoutStyle::column().display(LayoutDisplay::None));
    tree.commit(hidden, BackendCapabilities::default()).unwrap();
    let before = counts.dry.load(Ordering::Relaxed);
    assert_eq!(
        tree.dry_measure_node(
            host,
            ChildLayoutInput::new(LayoutConstraints::viewport(80.0, 60.0))
        )
        .unwrap()
        .size,
        [0.0; 2]
    );
    assert_eq!(counts.dry.load(Ordering::Relaxed), before);
    let mut show = Transaction::default();
    show.layout_style(host, LayoutStyle::column());
    tree.commit(show, BackendCapabilities::default()).unwrap();
    assert!(
        tree.dry_measure_node(host, ChildLayoutInput::new(constraints(80.0)))
            .is_ok()
    );
    assert!(counts.dry.load(Ordering::Relaxed) > before);
}

#[test]
fn dry_snapshot_identity_and_replacement_participate_in_typed_sources() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    let mut ui =
        LogicalUi::new_layout(LayoutStyle::column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    ui.append_render_layout(scope, root, fixed(20.0, 10.0), 1)
        .unwrap();
    let counts = Counts::default();
    let base = ParentLayoutSnapshot::new(Stack(counts.clone()));
    let dry = base.clone().with_measure(Stack(counts.clone()));
    assert!(dry.has_measure());
    assert!(!base.has_measure());
    assert_ne!(base, dry);
    let replacement = base.with_measure(Stack(counts.clone()));
    assert_ne!(dry, replacement);
    let source = ui.state(scope, 1, || Some(dry.clone())).unwrap();
    ui.bind(source, Property::parent_layout(root)).unwrap();
    ui.frame([80.0, 60.0]).unwrap();
    let equal = ui
        .transaction(|tx| {
            tx.set(source, Some(dry))?;
            Ok(())
        })
        .unwrap();
    assert!(!equal.commit.request_frame);
    let changed = ui
        .transaction(|tx| {
            tx.set(source, Some(replacement))?;
            Ok(())
        })
        .unwrap();
    assert!(changed.commit.request_frame);
    let plans = counts.plans.load(Ordering::Relaxed);
    let result = ui
        .dry_measure_node(root, ChildLayoutInput::new(constraints(80.0)))
        .unwrap();
    assert_eq!(result.size, [20.0, 10.0]);
    assert_eq!(counts.plans.load(Ordering::Relaxed), plans);
    ui.frame([80.0, 60.0]).unwrap();
    assert!(counts.plans.load(Ordering::Relaxed) > plans);
}

#[derive(Debug)]
struct Inline;
impl ParentInlineLayout for Inline {
    fn allocate(
        &self,
        _input: ParentInlineInput,
        count: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error> {
        Ok(vec![
            ChildInlineAllocation {
                width: 30.0,
                percentage_basis: Some(80.0)
            };
            count
        ])
    }
}

#[test]
fn dry_intrinsic_limits_containment_aspect_and_box_bounds_match_complete_plan_control() {
    for style in [
        LayoutStyle::column(),
        LayoutStyle::column().size(LayoutDimension::MinContent, LayoutDimension::Auto),
        LayoutStyle::column().size(LayoutDimension::MaxContent, LayoutDimension::Auto),
        LayoutStyle::column().max_size(LayoutDimension::FitContentPx(80.0), LayoutDimension::Auto),
        LayoutStyle::column().min_size(LayoutDimension::MinContent, LayoutDimension::Auto),
        LayoutStyle::column().container_type(LayoutContainerType::InlineSize),
        LayoutStyle::column().container_type(LayoutContainerType::Size),
        LayoutStyle {
            aspect_ratio: Some(2.0),
            ..LayoutStyle::column().size(LayoutDimension::Px(80.0), LayoutDimension::Auto)
        },
    ] {
        for bounds in [
            BoxConstraints::UNBOUNDED,
            BoxConstraints::loose([70.0, 100.0]),
            BoxConstraints::tight([50.0, 60.0]),
        ] {
            let mut results = vec![];
            for enabled in [false, true] {
                let mut tree = UiTree::new_layout(
                    style
                        .clone()
                        .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                    0,
                )
                .unwrap();
                let root = tree.root();
                let row = tree
                    .append_layout(root, LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap), 0)
                    .unwrap();
                for _ in 0..3 {
                    tree.append_layout(row, fixed(40.0, 10.0), 0xff0000ff)
                        .unwrap();
                }
                let (value, _) = snapshot(enabled);
                let mut tx = Transaction::default();
                tx.parent_layout(root, Some(value));
                tree.commit(tx, BackendCapabilities::default()).unwrap();
                let constraints = constraints(90.0).with_bounds(bounds);
                let query = tree
                    .dry_measure_node(
                        root,
                        ChildLayoutInput {
                            constraints,
                            percentage_basis: LayoutSize::new(Some(90.0), None),
                            ..ChildLayoutInput::default()
                        },
                    )
                    .unwrap();
                let frame = tree.frame_in_viewport(constraints, [180.0; 2]).unwrap();
                results.push((
                    query,
                    tree.layout_result(root).unwrap(),
                    super::tests::pixels(frame),
                ));
            }
            assert_eq!(results[0], results[1], "style {style:?} bounds {bounds:?}");
        }
    }
}
#[derive(Debug)]
struct Full;
impl ParentAllocationLayout for Full {
    fn allocate_children(
        &self,
        _input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        (0..children.len())
            .map(|child| {
                let data = children
                    .parent_data(child)?
                    .ok_or(Error::InvalidParentLayout)?;
                let width = *data.get::<f32>().ok_or(Error::InvalidParentLayout)?;
                Ok(ChildLayoutInput {
                    constraints: LayoutConstraints::new(
                        LayoutAvailableSpace::Definite(width),
                        LayoutAvailableSpace::Unbounded,
                    )
                    .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
                    percentage_basis: LayoutSize::new(Some(80.0), Some(40.0)),
                    ..ChildLayoutInput::default()
                })
            })
            .collect()
    }
}
#[test]
fn dry_and_full_measurement_share_enforced_inline_full_metadata_and_baselines() {
    for full in [false, true] {
        let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
        let root = tree.root();
        let leaf = tree
            .append_layout(
                root,
                LayoutStyle::row().size(LayoutDimension::Px(999.0), LayoutDimension::Percent(0.5)),
                1,
            )
            .unwrap();
        let counts = Counts::default();
        let stack = Stack(counts.clone());
        let mut value = ParentLayoutSnapshot::new(stack.clone()).with_measure(stack);
        value = if full {
            value.with_allocation_layout(Full)
        } else {
            value.with_inline_layout(Inline)
        };
        let mut tx = Transaction::default();
        tx.parent_layout(root, Some(value));
        tx.parent_data(leaf, Some(ParentData::new(50.0f32)));
        tx.measure(
            leaf,
            Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                width: 10.0,
                height: 10.0,
                first_baseline: Some(3.0),
                last_baseline: Some(7.0),
            })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let output = tree
            .dry_measure_node(root, ChildLayoutInput::new(constraints(100.0)))
            .unwrap();
        assert_eq!(output.size, if full { [50.0, 20.0] } else { [30.0, 10.0] });
        assert_eq!(output.baseline, Some(3.0));
        assert_eq!(output.last_baseline, Some(7.0));
        assert_eq!(counts.plans.load(Ordering::Relaxed), 0);
        tree.frame_in_viewport(constraints(100.0), [180.0; 2])
            .unwrap();
        assert_eq!(tree.layout_result(root).unwrap().size, output.size);
        assert_eq!(tree.layout_result(root).unwrap().baseline, output.baseline);
        assert_eq!(
            tree.layout_result(root).unwrap().last_baseline,
            output.last_baseline
        );
        assert_eq!(counts.plans.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn dry_nested_custom_parents_publish_one_complete_plan_each_after_query() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let inner = tree
        .append_layout(
            root,
            LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(2.0))),
            0,
        )
        .unwrap();
    tree.append_layout(inner, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    let (outer, a) = snapshot(true);
    let (nested, b) = snapshot(true);
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(outer));
    tx.parent_layout(inner, Some(nested));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let output = tree
        .dry_measure_node(root, ChildLayoutInput::new(constraints(100.0)))
        .unwrap();
    assert_eq!(output.size, [24.0, 14.0]);
    assert_eq!(a.plans.load(Ordering::Relaxed), 0);
    assert_eq!(b.plans.load(Ordering::Relaxed), 0);
    tree.frame_in_viewport(constraints(100.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, output.size);
    assert_eq!(a.plans.load(Ordering::Relaxed), 1);
    assert_eq!(b.plans.load(Ordering::Relaxed), 1);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_dry_parent_layout_matches_ordinary_full_reference() {
    let (mut tree, _, _) = fixture(true);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("dry parent adapter: {:?}", gpu.adapter_info());
    for width in [90.0, 50.0, 130.0, 90.0] {
        let frame = tree
            .local_frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        gpu.submit_local(&frame, [180; 2], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference(width), [180; 2], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
