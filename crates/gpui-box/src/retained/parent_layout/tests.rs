use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A deliberately small client algorithm, rather than a built-in container:
/// give each child the host inline extent and stack measured content heights.
#[derive(Debug)]
struct Stack {
    gap: f32,
    calls: Arc<AtomicUsize>,
}
impl ParentLayout for Stack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let width = input.known_size.width.or(match input.available.width {
            layout::AvailableSpace::Definite(v) => Some(v),
            _ => None,
        });
        let mut placements = vec![];
        let mut y: f32 = 0.0;
        let mut used_width: f32 = 0.0;
        let mut baseline = None;
        for child in 0..children.len() {
            let bounds = width
                .map(|w| BoxConstraints::new([w, 0.0], [w, f32::INFINITY]))
                .unwrap_or(BoxConstraints::UNBOUNDED);
            let slot = ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    input.available.width,
                    layout::AvailableSpace::Unbounded,
                )
                .with_bounds(bounds),
                percentage_basis: input.percentage_basis,
            };
            let result = children.measure(child, slot)?;
            if child == 0 {
                baseline = result.baseline;
            }
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [0.0, y],
            });
            used_width = used_width.max(result.size[0]);
            y += result.size[1]
                + if child + 1 < children.len() {
                    self.gap
                } else {
                    0.0
                };
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(used_width),
                input.known_size.height.unwrap_or(y),
            ],
            baseline: baseline.filter(|v| *v <= input.known_size.height.unwrap_or(y)),
            placements,
        })
    }
}
fn snapshot(gap: f32) -> (ParentLayoutSnapshot, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        ParentLayoutSnapshot::new(Stack {
            gap,
            calls: calls.clone(),
        }),
        calls,
    )
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
        layout::AvailableSpace::Definite(w),
        layout::AvailableSpace::Unbounded,
    )
}
pub(super) fn install(
    tree: &mut UiTree,
    node: NodeId,
    delegate: Option<ParentLayoutSnapshot>,
) -> Commit {
    let mut tx = Transaction::default();
    tx.parent_layout(node, delegate);
    tree.commit(tx, BackendCapabilities::default()).unwrap()
}
pub(super) fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 180],
            scale: 1.0,
            background: 0,
            effects: pacing::SoftwareEffects::Exact,
            max_bytes: 2 * 1024 * 1024,
        },
    )
    .unwrap();
    while !job.step(4096).unwrap() {}
    job.into_frame().unwrap().pixels().to_vec()
}

#[test]
fn custom_parent_measures_real_wrapped_flex_and_moves_following_sibling() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let host = tree
        .append_layout(
            tree.root(),
            column().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let flex = tree
        .append_layout(host, LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap), 0)
        .unwrap();
    let leaves: Vec<_> = (0..3)
        .map(|_| {
            tree.append_layout(flex, fixed(40.0, 10.0), 0xff0000ff)
                .unwrap()
        })
        .collect();
    let lower = tree
        .append_layout(host, fixed(10.0, 8.0), 0x0000ffff)
        .unwrap();
    let footer = tree
        .append_layout(tree.root(), fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    let (delegate, calls) = snapshot(3.0);
    install(&mut tree, host, Some(delegate));
    let mut old = None;
    for (width, rows) in [(90.0, 2.0), (50.0, 3.0), (130.0, 1.0), (90.0, 2.0)] {
        let frame = tree
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        let h = rows * 10.0;
        assert_eq!(tree.layout_result(flex).unwrap().size, [width - 10.0, h]);
        assert_eq!(tree.layout_result(lower).unwrap().location, [5.0, h + 8.0]);
        assert_eq!(tree.layout_result(host).unwrap().size, [width, h + 21.0]);
        assert_eq!(
            tree.layout_result(footer).unwrap().location,
            [0.0, h + 21.0]
        );
        assert_eq!(frame.hit_test(6.0, 6.0), Some(leaves[0]));
        assert_eq!(frame.hit_test(6.0, h + 9.0), Some(lower));
        // Independent ordinary Flex tree, without invoking the custom algorithm.
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.frame_build_policy = FrameBuildPolicy::Full;
        let full_host = full
            .append_layout(
                full.root(),
                column()
                    .gap(LayoutLength::Px(3.0), LayoutLength::Px(0.0))
                    .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                0,
            )
            .unwrap();
        let full_flex = full
            .append_layout(
                full_host,
                LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap),
                0,
            )
            .unwrap();
        for _ in 0..3 {
            full.append_layout(full_flex, fixed(40.0, 10.0), 0xff0000ff)
                .unwrap();
        }
        full.append_layout(
            full_host,
            fixed(10.0, 8.0).size(LayoutDimension::Percent(1.0), LayoutDimension::Px(8.0)),
            0x0000ffff,
        )
        .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        assert_eq!(
            pixels(frame.clone()),
            pixels(
                full.frame_in_viewport(constraints(width), [180.0; 2])
                    .unwrap()
            )
        );
        if old.is_none() {
            old = Some(frame);
        }
    }
    let before = calls.load(Ordering::Relaxed);
    let mut color = Transaction::default();
    color.color(lower, 0xffffffff);
    tree.commit(color, BackendCapabilities::default()).unwrap();
    let frame = tree
        .frame_in_viewport(constraints(90.0), [180.0; 2])
        .unwrap();
    assert_eq!(frame.layout_runs, 0);
    assert_eq!(calls.load(Ordering::Relaxed), before);
    drop(tree);
    assert_eq!(old.unwrap().hit_test(6.0, 29.0), Some(lower));
}

#[test]
fn snapshot_identity_and_real_child_changes_drive_cache_invalidation() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let host = tree.append_layout(tree.root(), column(), 0).unwrap();
    let a = tree.append_layout(host, fixed(10.0, 10.0), 0).unwrap();
    let b = tree.append_layout(host, fixed(10.0, 10.0), 0).unwrap();
    let unrelated = tree
        .append_layout(tree.root(), fixed(10.0, 10.0), 0)
        .unwrap();
    let (delegate, calls) = snapshot(2.0);
    install(&mut tree, host, Some(delegate.clone()));
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    let before = calls.load(Ordering::Relaxed);
    assert_eq!(install(&mut tree, host, Some(delegate)).changed_nodes, 0);
    let mut tx = Transaction::default();
    tx.color(unrelated, 0xffffffff);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        before,
        "paint-only sibling edit does not invoke layout"
    );
    let mut tx = Transaction::default();
    tx.layout_style(a, fixed(10.0, 20.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert!(calls.load(Ordering::Relaxed) > before);
    assert_eq!(tree.layout_result(b).unwrap().location, [0.0, 22.0]);
    install(&mut tree, host, Some(snapshot(7.0).0));
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(b).unwrap().location, [0.0, 27.0]);
    install(&mut tree, host, None);
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(b).unwrap().location, [0.0, 20.0]);
}

#[derive(Debug)]
struct Invalid(u8);
impl ParentLayout for Invalid {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut output = Stack {
            gap: 0.0,
            calls: Arc::new(AtomicUsize::new(0)),
        }
        .layout(input, children)?;
        match self.0 {
            0 => output.size[0] = f32::NAN,
            1 => {
                output.placements.pop();
            }
            2 => output.placements[0].origin[1] = f32::INFINITY,
            3 => output.placements[0].input.percentage_basis.width = Some(-1.0),
            4 => output.placements[0].child = usize::MAX,
            5 => output.baseline = Some(output.size[1] + 1.0),
            6 => output.size[0] += 1.0,
            7 => return Err(Error::InvalidParentLayout),
            _ => {
                output.placements.push(output.placements[0].clone());
            }
        }
        Ok(output)
    }
}

#[test]
fn invalid_plans_preserve_persistent_layout_and_failed_caches_are_not_reused() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let child = tree
        .append_layout(root, fixed(10.0, 10.0), 0xff0000ff)
        .unwrap();
    install(&mut tree, root, Some(snapshot(0.0).0));
    let old = tree
        .frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    let geometry = tree.layout_result(child).unwrap();
    for mode in 0..9 {
        install(
            &mut tree,
            root,
            Some(ParentLayoutSnapshot::new(Invalid(mode))),
        );
        assert!(
            matches!(
                tree.frame_in_viewport(constraints(80.0), [180.0; 2]),
                Err(Error::InvalidParentLayout)
            ),
            "mode {mode}"
        );
        assert!(
            matches!(
                tree.frame_in_viewport(constraints(80.0), [180.0; 2]),
                Err(Error::InvalidParentLayout)
            ),
            "cached error {mode}"
        );
        assert_eq!(tree.layout_result(child).unwrap(), geometry);
        assert_eq!(tree.frame.as_ref().unwrap().revision, old.revision);
        assert_eq!(
            tree.layout
                .layout(tree.node(child).unwrap().layout)
                .unwrap()
                .size
                .width,
            geometry.size[0]
        );
    }
    install(&mut tree, root, Some(snapshot(0.0).0));
    assert_eq!(
        pixels(
            tree.frame_in_viewport(constraints(80.0), [180.0; 2])
                .unwrap()
        ),
        pixels(old)
    );
}

#[test]
fn topology_and_properties_validate_parent_leaf_and_position_contracts() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let a = tree.append_layout(root, fixed(10.0, 10.0), 0).unwrap();
    install(&mut tree, root, Some(snapshot(0.0).0));
    let mut tx = Transaction::default();
    tx.measure(
        root,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 1.0,
            height: 1.0,
            baseline: None,
        })),
    );
    assert!(tree.commit(tx, BackendCapabilities::default()).is_err());
    let mut tx = Transaction::default();
    tx.style(
        a,
        Style {
            position: taffy::Position::Absolute,
            ..Default::default()
        },
    );
    assert!(matches!(
        tree.commit(tx, BackendCapabilities::default()),
        Err(Error::InvalidParentLayout)
    ));
    assert!(matches!(
        tree.append(
            root,
            Style {
                position: taffy::Position::Absolute,
                ..Default::default()
            },
            0
        ),
        Err(Error::InvalidParentLayout)
    ));
    let mut tx = Transaction::default();
    tx.layout_style(
        root,
        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose),
    );
    assert!(matches!(
        tree.commit(tx, BackendCapabilities::default()),
        Err(Error::InvalidParentLayout)
    ));
    // Replacing both sides of the protocol in one batch is valid.
    let mut tx = Transaction::default();
    tx.parent_layout(root, None);
    tx.layout_style(
        root,
        LayoutStyle::constraint_box(ConstraintFit::Tight, ConstraintFit::Loose),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
}

#[test]
fn custom_snapshot_construction_reparent_and_retirement_use_final_edit_topology() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let (delegate, _) = snapshot(2.0);
    let result = tree
        .edit(BackendCapabilities::default(), |edit| {
            let host = edit.append_layout(root, column(), 0)?;
            edit.properties().parent_layout(host, Some(delegate));
            let a = edit.append_layout(host, fixed(10.0, 10.0), 0xff0000ff)?;
            let b = edit.append_layout(host, fixed(10.0, 10.0), 0x0000ffff)?;
            Ok((host, a, b))
        })
        .unwrap();
    let (host, a, b) = result.value;
    assert!(tree.custom_parents.contains(&host));
    let old = tree
        .frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(b).unwrap().location, [0.0, 12.0]);
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(b, root)?;
        Ok(())
    })
    .unwrap();
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size[1], 10.0);
    assert_eq!(tree.layout_result(b).unwrap().location[1], 10.0);
    let mut tx = StructureTransaction::default();
    tx.remove(host);
    tree.commit_structure(tx).unwrap();
    assert!(tree.custom_parents.is_empty());
    assert!(tree.layout_result(a).is_err());
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(old.hit_test(1.0, 1.0), Some(a));
    assert_eq!(old.hit_test(1.0, 13.0), Some(b));
}

#[test]
fn atomic_logical_frame_rejects_plan_without_source_effect_or_frame_publication() {
    use logical::LogicalUi;
    let mut ui = LogicalUi::new_layout(column(), 0, BackendCapabilities::default()).unwrap();
    let root = ui.root_render();
    let scope = ui.root_scope();
    ui.append_render_layout(scope, root, fixed(10.0, 10.0), 0xff0000ff)
        .unwrap();
    let source = ui.state(scope, 11, || Some(snapshot(0.0).0)).unwrap();
    ui.bind(source, reactive::Property::parent_layout(root))
        .unwrap();
    let effects = std::rc::Rc::new(std::cell::Cell::new(0));
    let observer = effects.clone();
    ui.effect(scope, 13, &[source.dependency()], move |_| {
        observer.set(observer.get() + 1);
        None
    })
    .unwrap();
    let old = ui.frame_in_viewport(constraints(80.0), [180.0; 2]).unwrap();
    let saved = ui.read(source).unwrap().clone();
    let before_effects = effects.get();
    let result = ui.edit_frame(
        constraints(80.0),
        [180.0; 2],
        |event| {
            event.set(source, Some(ParentLayoutSnapshot::new(Invalid(2))))?;
            Ok(())
        },
        |(), _| Ok(()),
    );
    assert!(matches!(result, Err(Error::InvalidParentLayout)));
    assert_eq!(*ui.read(source).unwrap(), saved);
    assert_eq!(effects.get(), before_effects);
    let current = ui.frame_in_viewport(constraints(80.0), [180.0; 2]).unwrap();
    assert_eq!(current.revision, old.revision);
    assert_eq!(pixels(current), pixels(old));
}

#[derive(Debug)]
struct HeightBasis;
impl ParentLayout for HeightBasis {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let mut seen = vec![];
        let mut placements = vec![];
        // Same known width/available space with different vertical percentage
        // bases must not hit the size-only CSS measurement cache.
        for basis in [20.0, 40.0, 20.0] {
            let slot = ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    layout::AvailableSpace::Definite(80.0),
                    layout::AvailableSpace::Unbounded,
                ),
                percentage_basis: LayoutSize::new(Some(80.0), Some(basis)),
            };
            let result = children.measure(0, slot)?;
            seen.push(result.size[1]);
            if placements.is_empty() {
                placements.push(ParentPlacement {
                    child: 0,
                    input: slot,
                    origin: [0.0, 0.0],
                });
            }
        }
        assert_eq!(seen, [10.0, 20.0, 10.0]);
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(80.0),
                input.known_size.height.unwrap_or(10.0),
            ],
            baseline: None,
            placements,
        })
    }
}
#[test]
fn direct_child_measurement_preserves_vertical_percentage_bases() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    tree.append_layout(
        root,
        LayoutStyle::row().size(LayoutDimension::Px(10.0), LayoutDimension::Percent(0.5)),
        0,
    )
    .unwrap();
    install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(HeightBasis)),
    );
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
}

#[derive(Debug)]
struct RepeatedStack {
    repeats: usize,
}
impl ParentLayout for RepeatedStack {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let width = input.known_size.width.unwrap_or(80.0);
        let slot = ChildLayoutInput {
            constraints: constraints(width)
                .with_bounds(BoxConstraints::new([width, 0.0], [width, f32::INFINITY])),
            percentage_basis: LayoutSize::new(Some(width), None),
        };
        let result = children.measure(0, slot)?;
        for _ in 1..self.repeats {
            assert_eq!(children.measure(0, slot)?, result);
        }
        Ok(ParentLayoutOutput {
            size: [width, input.known_size.height.unwrap_or(result.size[1])],
            baseline: result.baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [0.0; 2],
            }],
        })
    }
}

#[test]
fn repeated_custom_child_measurements_reuse_real_flex_work_with_complete_baselines() {
    fn make(repeats: usize) -> (UiTree, NodeId, NodeId) {
        let mut tree = UiTree::new_layout(column(), 0).unwrap();
        let root = tree.root();
        let flex = tree
            .append_layout(root, LayoutStyle::row().flex_wrap(LayoutFlexWrap::Wrap), 0)
            .unwrap();
        let leaf = tree
            .append_layout(flex, LayoutStyle::row(), 0xff0000ff)
            .unwrap();
        tree.append_layout(flex, fixed(40.0, 12.0), 0x0000ffff)
            .unwrap();
        let mut tx = Transaction::default();
        tx.measure(
            leaf,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 35.0,
                height: 10.0,
                baseline: Some(7.0),
            })),
        );
        tx.parent_layout(
            root,
            Some(ParentLayoutSnapshot::new(RepeatedStack { repeats })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        (tree, flex, leaf)
    }
    let (mut once, once_flex, once_leaf) = make(1);
    let (mut many, many_flex, many_leaf) = make(12);
    let mut old = None;
    for width in [80.0, 45.0, 100.0, 80.0] {
        let a = once
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        let b = many
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        assert_eq!(a.layout_stats.leaf_layouts, b.layout_stats.leaf_layouts);
        assert_eq!(a.layout_stats.measure_calls, b.layout_stats.measure_calls);
        assert_eq!(
            once.layout_result(once_flex).unwrap(),
            many.layout_result(many_flex).unwrap()
        );
        assert_eq!(
            once.layout_result(once_leaf).unwrap(),
            many.layout_result(many_leaf).unwrap()
        );
        assert_eq!(many.layout_result(many_leaf).unwrap().baseline, Some(7.0));
        assert_eq!(
            many.layout_result(many.root()).unwrap().baseline,
            once.layout_result(once.root()).unwrap().baseline
        );
        assert_eq!(pixels(a), pixels(b.clone()));
        assert_eq!(b.hit_test(1.0, 1.0), Some(many_leaf));
        if old.is_none() {
            old = Some(b);
        }
    }
    // Replace the intrinsic resource: cached descendants and parents must change.
    for (tree, leaf) in [(&mut once, once_leaf), (&mut many, many_leaf)] {
        let mut tx = Transaction::default();
        tx.measure(
            leaf,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 35.0,
                height: 20.0,
                baseline: Some(16.0),
            })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
    }
    let a = once
        .frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    let b = many
        .local_frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(many.layout_result(many_leaf).unwrap().baseline, Some(16.0));
    assert_eq!(pixels(a), pixels(b.resolve()));
    assert_eq!(old.unwrap().hit_test(1.0, 1.0), Some(many_leaf));
}

#[test]
fn repeated_direct_leaf_measurements_keep_baseline_after_exact_hits_and_resource_replacement() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let leaf = tree
        .append_layout(root, LayoutStyle::row(), 0xff0000ff)
        .unwrap();
    install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(RepeatedStack { repeats: 12 })),
    );
    for baseline in [7.0, 16.0, 7.0] {
        let mut tx = Transaction::default();
        tx.measure(
            leaf,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 35.0,
                height: 20.0,
                baseline: Some(baseline),
            })),
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .frame_in_viewport(constraints(80.0), [180.0; 2])
            .unwrap();
        assert_eq!(tree.layout_result(root).unwrap().baseline, Some(baseline));
        assert_eq!(tree.layout_result(leaf).unwrap().baseline, Some(baseline));
        assert_eq!(frame.hit_test(1.0, 1.0), Some(leaf));
    }
}

#[test]
fn nested_custom_parents_apply_hard_bounds_and_publish_overflow_for_scrolling() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(
            root,
            LayoutStyle::column()
                .size(LayoutDimension::Px(500.0), LayoutDimension::Px(100.0))
                .padding(LayoutEdges::all(LayoutLength::Px(5.0)))
                .overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll),
            0,
        )
        .unwrap();
    let nested = tree.append_layout(host, LayoutStyle::column(), 0).unwrap();
    let red = tree
        .append_layout(nested, fixed(300.0, 30.0), 0xff0000ff)
        .unwrap();
    let blue = tree
        .append_layout(nested, fixed(300.0, 30.0), 0x0000ffff)
        .unwrap();
    install(&mut tree, host, Some(snapshot(0.0).0));
    install(&mut tree, nested, Some(snapshot(4.0).0));
    let bounds = constraints(80.0).with_bounds(BoxConstraints::tight([80.0, 40.0]));
    // The root assigns a hard slot to its child too.
    install(&mut tree, root, Some(snapshot(0.0).0));
    let frame = tree.frame_in_viewport(bounds, [180.0; 2]).unwrap();
    assert_eq!(tree.layout_result(root).unwrap().size, [80.0, 40.0]);
    assert_eq!(tree.layout_result(host).unwrap().size[0], 80.0);
    assert_eq!(tree.layout_result(nested).unwrap().size[1], 64.0);
    assert_eq!(tree.layout_result(blue).unwrap().location[1], 34.0);
    assert_eq!(frame.hit_test(6.0, 6.0), Some(red));
    // Replacing the authored height gives a viewport smaller than content.
    let mut style = tree.node(host).unwrap().style.clone();
    style.size.height = taffy::Dimension::length(40.0);
    let mut tx = Transaction::default();
    tx.style(host, style);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(bounds, [180.0; 2]).unwrap();
    let layout = tree.layout_result(host).unwrap();
    assert!(layout.scrollable_overflow[3] > layout.size[1]);
    let mut tx = Transaction::default();
    tx.scroll(host, [0.0, 34.0]);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let scrolled = tree.frame_in_viewport(bounds, [180.0; 2]).unwrap();
    assert_eq!(scrolled.layout_runs, 0);
    assert_eq!(scrolled.hit_test(6.0, 15.0), Some(blue));
}

#[test]
fn hidden_intrinsic_baseline_subtree_query_and_local_frames_share_real_parent_protocol() {
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(
            root,
            LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(3.0))),
            0,
        )
        .unwrap();
    let leaf = tree
        .append_layout(host, LayoutStyle::row(), 0xff0000ff)
        .unwrap();
    let mut tx = Transaction::default();
    tx.measure(
        leaf,
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width: 20.0,
            height: 10.0,
            baseline: Some(7.0),
        })),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    install(&mut tree, host, Some(snapshot(0.0).0));
    let intrinsic = tree
        .measure_node_with_constraints(
            host,
            LayoutConstraints::new(
                layout::AvailableSpace::MaxContent,
                layout::AvailableSpace::Unbounded,
            ),
        )
        .unwrap();
    assert_eq!(intrinsic.size, [26.0, 16.0]);
    assert_eq!(intrinsic.baseline, Some(10.0));
    let resolved = tree
        .frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    let local = tree
        .local_frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(pixels(local.resolve()), pixels(resolved.clone()));
    assert_eq!(tree.layout_result(host).unwrap().baseline, Some(10.0));
    let hidden = LayoutStyle {
        display: LayoutDisplay::None,
        ..column()
    };
    let mut tx = Transaction::default();
    tx.layout_style(host, hidden);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame_in_viewport(constraints(80.0), [180.0; 2])
        .unwrap();
    assert_eq!(tree.layout_result(host).unwrap().size, [0.0; 2]);
    assert_eq!(tree.layout_result(leaf).unwrap().size, [0.0; 2]);
    let mut tx = Transaction::default();
    tx.layout_style(
        host,
        LayoutStyle::column().padding(LayoutEdges::all(LayoutLength::Px(3.0))),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert_eq!(
        pixels(
            tree.frame_in_viewport(constraints(80.0), [180.0; 2])
                .unwrap()
        ),
        pixels(resolved)
    );
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_custom_parent_matches_independent_full_flex_pixels() {
    use crate::retained::wgpu::WgpuFrameRenderer;
    let mut tree = UiTree::new_layout(column(), 0).unwrap();
    let root = tree.root();
    let host = tree
        .append_layout(
            root,
            column().padding(LayoutEdges::all(LayoutLength::Px(5.0))),
            0,
        )
        .unwrap();
    let a = tree
        .append_layout(host, fixed(10.0, 20.0), 0xff0000ff)
        .unwrap();
    tree.append_layout(host, fixed(10.0, 8.0), 0x0000ffff)
        .unwrap();
    tree.append_layout(root, fixed(20.0, 10.0), 0x00ff00ff)
        .unwrap();
    install(&mut tree, host, Some(snapshot(3.0).0));
    let mut renderer = WgpuFrameRenderer::new(2).unwrap();
    eprintln!("custom parent adapter: {:?}", renderer.adapter_info());
    for (width, height) in [(80.0, 20.0), (140.0, 40.0), (80.0, 20.0)] {
        let mut tx = Transaction::default();
        tx.layout_style(a, fixed(10.0, height));
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        assert!(renderer.submit(&frame, [180, 180], 1.0).unwrap().is_some());
        let actual = renderer.image().unwrap();
        let mut full = UiTree::new_layout(column(), 0).unwrap();
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let host = full
            .append_layout(
                full.root(),
                column()
                    .gap(LayoutLength::Px(3.0), LayoutLength::Px(0.0))
                    .padding(LayoutEdges::all(LayoutLength::Px(5.0))),
                0,
            )
            .unwrap();
        full.append_layout(
            host,
            fixed(10.0, height).size(LayoutDimension::Percent(1.0), LayoutDimension::Px(height)),
            0xff0000ff,
        )
        .unwrap();
        full.append_layout(
            host,
            fixed(10.0, 8.0).size(LayoutDimension::Percent(1.0), LayoutDimension::Px(8.0)),
            0x0000ffff,
        )
        .unwrap();
        full.append_layout(full.root(), fixed(20.0, 10.0), 0x00ff00ff)
            .unwrap();
        let reference = full
            .frame_in_viewport(constraints(width), [180.0; 2])
            .unwrap();
        assert_eq!(
            actual.as_raw(),
            renderer
                .reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
        assert_eq!(
            &actual.as_raw()[(6 * 180 + 6) * 4..(6 * 180 + 6) * 4 + 4],
            &[255, 0, 0, 255]
        );
    }
    renderer.finish().unwrap();
}
