use super::*;
use std::sync::{Mutex, atomic::AtomicUsize};

/// A client protocol, not a framework preset: a wrapping strip of equal cells.
struct Cells {
    count: usize,
    queries: Arc<Mutex<Vec<ContentMeasureInput>>>,
}
impl std::fmt::Debug for Cells {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cells").field("count", &self.count).finish()
    }
}
impl ContentMeasure for Cells {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        self.queries.lock().unwrap().push(input);
        let width = match input.available.width {
            LayoutAvailableSpace::MinContent => 10.0,
            LayoutAvailableSpace::MaxContent | LayoutAvailableSpace::Unbounded => {
                self.count as f32 * 10.0
            }
            LayoutAvailableSpace::Definite(width) => width,
        };
        let columns = ((width / 10.0).floor() as usize).max(1).min(self.count);
        let height = self.count.div_ceil(columns) as f32 * 8.0;
        Ok(ContentMeasureOutput {
            size: [columns as f32 * 10.0, height],
            baseline: Some(6.0),
            last_baseline: Some(height - 2.0),
        })
    }
}
fn cells(count: usize) -> (MeasureSnapshot, Arc<Mutex<Vec<ContentMeasureInput>>>) {
    let queries = Arc::new(Mutex::new(vec![]));
    (
        MeasureSnapshot::Content(ContentMeasureSnapshot::new(Cells {
            count,
            queries: queries.clone(),
        })),
        queries,
    )
}
fn fixed(w: f32, h: f32) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Px(w), LayoutDimension::Px(h))
        .flex_shrink(0.0)
}
fn host() -> LayoutStyle {
    let mut style = LayoutStyle::row()
        .flex_wrap(LayoutFlexWrap::Wrap)
        .align_items(LayoutAlignItems::Start)
        .align_content(LayoutAlignContent::Start)
        .padding(LayoutEdges::all(LayoutLength::Px(2.0)))
        .flex_shrink(0.0);
    style.border = LayoutEdges::all(LayoutLength::Px(1.0));
    style
}
fn install(tree: &mut UiTree, node: NodeId, snapshot: Option<MeasureSnapshot>) {
    let mut tx = Transaction::default();
    tx.measure(node, snapshot);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
}
fn populate(tree: &mut UiTree, node: NodeId, count: usize) {
    for _ in 0..count {
        let cell = tree.append_layout(node, fixed(10.0, 8.0), 0).unwrap();
        install(
            tree,
            cell,
            Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
                width: 10.0,
                height: 8.0,
                baseline: Some(6.0),
            })),
        );
    }
}
fn pixels(frame: FramePacket) -> Vec<u8> {
    let mut job = software::SoftwareJob::new(
        frame,
        software::SoftwareOptions {
            size: [180, 140],
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
fn fixture(custom: bool, count: usize) -> (UiTree, NodeId, NodeId) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().align_items(LayoutAlignItems::Start),
        0,
    )
    .unwrap();
    let node = tree.append_layout(tree.root(), host(), 0xff0000ff).unwrap();
    if custom {
        install(&mut tree, node, Some(cells(count).0));
    } else {
        populate(&mut tree, node, count);
    }
    let footer = tree
        .append_layout(tree.root(), fixed(20.0, 5.0), 0x00ff00ff)
        .unwrap();
    (tree, node, footer)
}

#[test]
fn selected_inline_size_drives_content_height_and_full_flex_geometry_pixels() {
    for local in [false, true] {
        let (mut tree, node, footer) = fixture(true, 12);
        let (mut full, ref_node, ref_footer) = fixture(false, 12);
        full.set_frame_build_policy(FrameBuildPolicy::Full);
        let mut old = None;
        for width in [36.0, 76.0, 116.0, 36.0] {
            for (owner, id) in [(&mut tree, node), (&mut full, ref_node)] {
                let mut style = host();
                style.size.width = LayoutDimension::Px(width);
                let mut tx = Transaction::default();
                tx.layout_style(id, style);
                owner.commit(tx, BackendCapabilities::default()).unwrap();
            }
            let actual = if local {
                tree.local_frame([180.0, 140.0]).unwrap().resolve()
            } else {
                tree.frame([180.0, 140.0]).unwrap()
            };
            let reference = full.frame([180.0, 140.0]).unwrap();
            assert_eq!(
                tree.layout_result(node).unwrap(),
                full.layout_result(ref_node).unwrap(),
                "width={width}"
            );
            assert_eq!(
                tree.layout_result(footer).unwrap(),
                full.layout_result(ref_footer).unwrap()
            );
            assert_eq!(pixels(actual.clone()), pixels(reference));
            assert_eq!(actual.hit_test(1.0, 1.0), Some(node));
            old.get_or_insert(actual);
        }
        let old = old.unwrap();
        let expected = pixels(old.clone());
        drop((tree, full));
        assert_eq!(pixels(old), expected);
    }
}

#[test]
fn ordinary_flex_parent_allocations_remeasure_generic_content_in_both_axes() {
    for direction in [
        LayoutFlexDirection::Row,
        LayoutFlexDirection::RowReverse,
        LayoutFlexDirection::Column,
        LayoutFlexDirection::ColumnReverse,
    ] {
        for rtl in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
            for width in [46.0, 116.0] {
                let mut outputs = vec![];
                for generic in [false, true] {
                    let mut root_style = fixed(width, 100.0);
                    root_style.flex_direction = direction;
                    root_style.direction = rtl;
                    root_style.align_items = LayoutAlignItems::Start.into();
                    let mut tree = UiTree::new_layout(root_style, 0).unwrap();
                    let rail = tree
                        .append_layout(tree.root(), fixed(10.0, 10.0), 0x00ff00ff)
                        .unwrap();
                    let mut style = host()
                        .flex_grow(1.0)
                        .flex_shrink(1.0)
                        .flex_basis(LayoutDimension::Px(0.0));
                    style.min_size =
                        LayoutSize::new(LayoutDimension::Px(0.0), LayoutDimension::Px(0.0));
                    let node = tree.append_layout(tree.root(), style, 0xff0000ff).unwrap();
                    if generic {
                        install(&mut tree, node, Some(cells(12).0));
                    } else {
                        populate(&mut tree, node, 12);
                    }
                    let frame = tree.local_frame([180.0, 140.0]).unwrap();
                    outputs.push((
                        tree.layout_result(node).unwrap(),
                        tree.layout_result(rail).unwrap(),
                        pixels(frame.resolve()),
                    ));
                }
                assert_eq!(outputs[0], outputs[1], "{direction:?}/{rtl:?}/{width}");
            }
        }
    }
}

#[test]
fn intrinsic_queries_and_hard_selected_width_preserve_modes_and_live_state() {
    let (snapshot, queries) = cells(12);
    let mut tree = UiTree::new_layout(host(), 0xff0000ff).unwrap();
    let root = tree.root();
    install(&mut tree, root, Some(snapshot));
    let old = tree.frame([180.0, 140.0]).unwrap();
    let before = tree.layout_result(root).unwrap();
    let state = format!("{:?}", tree.layout);
    for (space, expected) in [
        (LayoutAvailableSpace::MinContent, [16.0, 102.0]),
        (LayoutAvailableSpace::MaxContent, [126.0, 14.0]),
        (LayoutAvailableSpace::Unbounded, [126.0, 14.0]),
    ] {
        let input = ChildLayoutInput::new(LayoutConstraints::new(
            space,
            LayoutAvailableSpace::Unbounded,
        ));
        let a = tree.dry_measure_node(root, input).unwrap();
        let b = tree.dry_measure_node_baselines(root, input).unwrap();
        assert_eq!(a.size, expected);
        assert_eq!(a, b);
    }
    let result = tree
        .dry_measure_node(
            root,
            ChildLayoutInput::new(
                LayoutConstraints::new(
                    LayoutAvailableSpace::Unbounded,
                    LayoutAvailableSpace::Unbounded,
                )
                .with_bounds(BoxConstraints::loose([46.0, f32::INFINITY])),
            ),
        )
        .unwrap();
    assert_eq!(result.size, [46.0, 30.0]);
    assert!(
        queries
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.available.width == LayoutAvailableSpace::Definite(40.0))
    );
    assert!(
        queries
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.available.width == LayoutAvailableSpace::MinContent)
    );
    assert!(
        queries
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.available.width == LayoutAvailableSpace::MaxContent)
    );
    assert!(
        queries
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.available.width == LayoutAvailableSpace::Unbounded)
    );
    assert_eq!(format!("{:?}", tree.layout), state);
    assert_eq!(tree.layout_result(root).unwrap(), before);
    assert_eq!(tree.published_frame().unwrap().revision, old.revision);
}

#[test]
fn intrinsic_limits_percent_insets_containment_and_direction_match_ordinary_flex() {
    let mut cases = 0;
    for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
        for kind in [
            LayoutContainerType::Normal,
            LayoutContainerType::InlineSize,
            LayoutContainerType::Size,
        ] {
            for mode in 0..5 {
                let mut outputs = vec![];
                for generic in [false, true] {
                    let mut tree = UiTree::new_layout(
                        LayoutStyle::column().align_items(LayoutAlignItems::Start),
                        0,
                    )
                    .unwrap();
                    let mut style = host().container_type(kind);
                    style.direction = direction;
                    style.padding = LayoutEdges::all(LayoutLength::Percent(0.025));
                    match mode {
                        0 => style.size.width = LayoutDimension::MinContent,
                        1 => style.size.width = LayoutDimension::MaxContent,
                        2 => style.max_size.width = LayoutDimension::FitContentPx(46.0),
                        3 => {
                            style.size.width = LayoutDimension::Percent(0.5);
                            style.min_size.height = LayoutDimension::MinContent;
                        }
                        _ => {
                            style.size.width = LayoutDimension::Px(46.0);
                            style.max_size.height = LayoutDimension::Px(15.0);
                        }
                    }
                    let node = tree.append_layout(tree.root(), style, 0xff0000ff).unwrap();
                    if generic {
                        install(&mut tree, node, Some(cells(12).0));
                    } else {
                        populate(&mut tree, node, 12);
                    }
                    let frame = tree.local_frame([180.0, 140.0]).unwrap();
                    outputs.push((tree.layout_result(node).unwrap(), pixels(frame.resolve())));
                }
                assert_eq!(
                    outputs[0], outputs[1],
                    "direction={direction:?}/kind={kind:?}/mode={mode}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 30);
}

#[derive(Debug)]
struct Forward;
impl ParentLayout for Forward {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::new(
                input.available.width,
                LayoutAvailableSpace::Unbounded,
            ),
            percentage_basis: input.percentage_basis,
            ..ChildLayoutInput::default()
        };
        let child = children.measure_baselines(0, slot)?;
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(child.size[0]),
                input.known_size.height.unwrap_or(child.size[1]),
            ],
            baseline: child.baseline,
            last_baseline: child.last_baseline,
            placements: vec![ParentPlacement {
                child: 0,
                input: slot,
                origin: [0.0, 0.0],
            }],
        })
    }
}

#[test]
fn typed_measure_sources_nested_custom_parents_and_restored_values_share_normal_routes() {
    let (mut tree, node, _) = fixture(true, 12);
    let host_id = tree.root();
    // Forward has one child; remove the independent footer first.
    let footer = tree.node(host_id).unwrap().children[1];
    tree.remove(footer).unwrap();
    let mut tx = Transaction::default();
    tx.parent_layout(host_id, Some(ParentLayoutSnapshot::new(Forward)));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let mut ui = logical::LogicalUi::from_tree(tree, BackendCapabilities::default()).unwrap();
    let scope = ui.root_scope();
    let original = cells(12).0;
    let source = ui.state(scope, 20, || Some(original.clone())).unwrap();
    ui.bind(source, reactive::Property::measure(node)).unwrap();
    let old = ui.local_frame([46.0, 140.0]).unwrap();
    let before = ui.layout_result(node).unwrap();
    let (changed, changed_queries) = cells(4);
    ui.transaction(|tx| {
        tx.set(source, Some(changed.clone()))?;
        tx.set(source, Some(original.clone()))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        ui.local_frame([46.0, 140.0])
            .unwrap()
            .resolve()
            .layout_stats,
        LayoutStats::default()
    );
    ui.transaction(|tx| {
        tx.set(source, Some(changed))?;
        Ok(())
    })
    .unwrap();
    let frame = ui.local_frame([46.0, 140.0]).unwrap();
    assert!(!changed_queries.lock().unwrap().is_empty());
    assert_eq!(
        frame.revision(),
        ui.published_local_frame().unwrap().revision()
    );
    assert!(ui.layout_result(node).unwrap().size[1] < before.size[1]);
    let expected = pixels(old.resolve());
    drop(ui);
    assert_eq!(pixels(old.resolve()), expected);
}

#[test]
fn scrollbar_gutters_deflate_content_and_final_metrics_share_the_same_query() {
    for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
        let mut style = host()
            .size(LayoutDimension::Px(60.0), LayoutDimension::Px(20.0))
            .overflow(LayoutOverflow::Scroll, LayoutOverflow::Scroll)
            .to_taffy()
            .unwrap();
        style.direction = match direction {
            LayoutDirection::Ltr => taffy::Direction::Ltr,
            LayoutDirection::Rtl => taffy::Direction::Rtl,
        };
        style.scrollbar_width = 6.0;
        let mut tree = UiTree::new(style, 0).unwrap();
        let root = tree.root();
        let (snapshot, queries) = cells(12);
        install(&mut tree, root, Some(snapshot));
        tree.frame([180.0, 140.0]).unwrap();
        let result = tree.layout_result(root).unwrap();
        assert_eq!(result.size, [60.0, 20.0]);
        assert_eq!(result.scrollable_overflow, [0.0, 44.0, 0.0, 28.0]);
        assert_eq!(
            (result.baseline, result.last_baseline),
            (Some(9.0), Some(25.0))
        );
        let input = *queries.lock().unwrap().last().unwrap();
        assert_eq!(
            input.available,
            LayoutSize::new(
                LayoutAvailableSpace::Definite(48.0),
                LayoutAvailableSpace::Definite(8.0)
            )
        );
    }
}

#[derive(Debug)]
struct SelectedWidthFailure;
impl ContentMeasure for SelectedWidthFailure {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        if input.available.width == LayoutAvailableSpace::Definite(20.0) {
            return Err(Error::InvalidMeasure);
        }
        Ok(ContentMeasureOutput {
            size: [20.0, 10.0],
            baseline: None,
            last_baseline: None,
        })
    }
}
#[test]
fn selected_width_failure_is_sticky_through_parent_and_private_tree_writes() {
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let node = tree
        .append_layout(root, LayoutStyle::row().flex_shrink(0.0), 1)
        .unwrap();
    let mut tx = Transaction::default();
    tx.parent_layout(root, Some(ParentLayoutSnapshot::new(Forward)));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let old = tree.frame([180.0, 140.0]).unwrap();
    let geometry = tree.layout_result(node).unwrap();
    install(
        &mut tree,
        node,
        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            SelectedWidthFailure,
        ))),
    );
    let state = format!("{:?}", tree.layout);
    assert_eq!(
        tree.frame([180.0, 140.0]).unwrap_err(),
        Error::InvalidMeasure
    );
    assert_eq!(format!("{:?}", tree.layout), state);
    assert_eq!(tree.layout_result(node).unwrap(), geometry);
    assert_eq!(tree.published_frame().unwrap().revision, old.revision);
    assert!(matches!(
        tree.append_layout(node, fixed(10.0, 10.0), 0),
        Err(Error::MeasuredNodeHasChildren)
    ));
}

#[derive(Debug)]
struct ModeSensitive;
impl ContentMeasure for ModeSensitive {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        // Same selected width, different normal/intrinsic block query.
        let height = match input.available.height {
            LayoutAvailableSpace::MinContent => 10.0,
            LayoutAvailableSpace::MaxContent => 20.0,
            LayoutAvailableSpace::Unbounded => 30.0,
            LayoutAvailableSpace::Definite(_) => 40.0,
        };
        Ok(ContentMeasureOutput {
            size: [20.0, height],
            baseline: Some(-2.0),
            last_baseline: Some(height + 3.0),
        })
    }
}
#[test]
fn complete_cache_keys_do_not_merge_query_modes_and_fixed_height_keeps_natural_extent() {
    let mut tree = UiTree::new_layout(
        LayoutStyle::row().size(LayoutDimension::Px(20.0), LayoutDimension::Auto),
        1,
    )
    .unwrap();
    let root = tree.root();
    install(
        &mut tree,
        root,
        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            ModeSensitive,
        ))),
    );
    for (space, h) in [
        (LayoutAvailableSpace::Unbounded, 30.0),
        (LayoutAvailableSpace::MaxContent, 20.0),
        (LayoutAvailableSpace::MinContent, 10.0),
        (LayoutAvailableSpace::Unbounded, 30.0),
    ] {
        let frame = tree
            .frame_in_viewport(
                LayoutConstraints::new(LayoutAvailableSpace::Unbounded, space),
                [180.0, 140.0],
            )
            .unwrap();
        let result = tree.layout_result(root).unwrap();
        assert_eq!(result.size, [20.0, h]);
        assert_eq!(result.baseline, Some(-2.0));
        assert_eq!(result.last_baseline, Some(h + 3.0));
        assert!(frame.layout_stats.measure_calls > 0);
    }
    let mut tx = Transaction::default();
    tx.layout_style(root, fixed(20.0, 5.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([180.0, 140.0]).unwrap();
    let result = tree.layout_result(root).unwrap();
    assert_eq!(result.size, [20.0, 5.0]);
    assert_eq!(result.scrollable_overflow, [0.0, 20.0, 0.0, 40.0]);
    assert_eq!(result.last_baseline, Some(43.0));
}

#[derive(Debug)]
struct Invalid(u8);
impl ContentMeasure for Invalid {
    fn measure(&self, _: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
        let mut output = ContentMeasureOutput {
            size: [20.0, 10.0],
            baseline: None,
            last_baseline: None,
        };
        match self.0 {
            0 => return Err(Error::InvalidMeasure),
            1 => output.size[0] = f32::NAN,
            2 => output.size[1] = -1.0,
            3 => output.baseline = Some(f32::INFINITY),
            _ => output.last_baseline = Some(f32::NEG_INFINITY),
        }
        Ok(output)
    }
}
#[test]
fn fallible_measurements_reject_queries_and_atomic_frames_without_poisoning_caches() {
    for local in [false, true] {
        for mode in 0..5 {
            let mut ui = logical::LogicalUi::new_layout(
                LayoutStyle::column(),
                0,
                BackendCapabilities::default(),
            )
            .unwrap();
            let root = ui.root_render();
            let scope = ui.root_scope();
            let leaf = ui
                .append_render_layout(scope, root, fixed(20.0, 10.0), 0xff0000ff)
                .unwrap();
            let source = ui.state(scope, 100, || 0u32).unwrap();
            let old = ui.frame([180.0, 140.0]).unwrap();
            let before = ui.layout_result(leaf).unwrap();
            let snapshot = MeasureSnapshot::Content(ContentMeasureSnapshot::new(Invalid(mode)));
            let result = if local {
                ui.edit_local_frame(
                    LayoutConstraints::viewport(180.0, 140.0),
                    [180.0, 140.0],
                    |tx| {
                        tx.set(source, 1)?;
                        Ok(())
                    },
                    |(), edit| {
                        edit.properties().measure(leaf, Some(snapshot));
                        Ok(())
                    },
                )
                .map(|_| ())
            } else {
                ui.edit_frame(
                    LayoutConstraints::viewport(180.0, 140.0),
                    [180.0, 140.0],
                    |tx| {
                        tx.set(source, 1)?;
                        Ok(())
                    },
                    |(), edit| {
                        edit.properties().measure(leaf, Some(snapshot));
                        Ok(())
                    },
                )
                .map(|_| ())
            };
            assert!(
                matches!(result, Err(Error::InvalidMeasure)),
                "mode={mode}/local={local}"
            );
            assert_eq!(*ui.read(source).unwrap(), 0);
            assert_eq!(ui.layout_result(leaf).unwrap(), before);
            assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        }
    }
    let (mut tree, node, _) = fixture(true, 12);
    let old = tree.frame([180.0, 140.0]).unwrap();
    install(
        &mut tree,
        node,
        Some(MeasureSnapshot::Content(ContentMeasureSnapshot::new(
            Invalid(0),
        ))),
    );
    let state = format!("{:?}", tree.layout);
    for _ in 0..2 {
        assert_eq!(
            tree.frame([180.0, 140.0]).unwrap_err(),
            Error::InvalidMeasure
        );
        assert_eq!(
            tree.dry_measure_node(
                node,
                ChildLayoutInput::new(LayoutConstraints::viewport(100.0, 100.0))
            )
            .unwrap_err(),
            Error::InvalidMeasure
        );
        assert_eq!(
            tree.dry_measure_node_baselines(
                node,
                ChildLayoutInput::new(LayoutConstraints::viewport(100.0, 100.0))
            )
            .unwrap_err(),
            Error::InvalidMeasure
        );
        assert_eq!(
            tree.measure_node_with_constraints(node, LayoutConstraints::viewport(100.0, 100.0))
                .unwrap_err(),
            Error::InvalidMeasure
        );
        assert_eq!(format!("{:?}", tree.layout), state);
        assert_eq!(tree.published_frame().unwrap().revision, old.revision);
    }
    install(&mut tree, node, Some(cells(12).0));
    assert!(tree.frame([180.0, 140.0]).is_ok());
}

#[test]
fn snapshot_identity_coalescing_hidden_removal_and_old_frame_lifetimes() {
    #[derive(Debug)]
    struct Alive(Arc<AtomicUsize>);
    impl Drop for Alive {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl ContentMeasure for Alive {
        fn measure(&self, _: ContentMeasureInput) -> Result<ContentMeasureOutput, Error> {
            Ok(ContentMeasureOutput {
                size: [20.0, 10.0],
                baseline: None,
                last_baseline: None,
            })
        }
    }
    let dead = Arc::new(AtomicUsize::new(0));
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let node = tree
        .append_layout(tree.root(), fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    let original = MeasureSnapshot::Content(ContentMeasureSnapshot::new(Alive(dead.clone())));
    install(&mut tree, node, Some(original.clone()));
    let old = tree.frame([180.0, 140.0]).unwrap();
    let mut tx = Transaction::default();
    tx.measure(node, Some(cells(12).0));
    tx.measure(node, Some(original.clone()));
    assert!(
        !tree
            .commit(tx, BackendCapabilities::default())
            .unwrap()
            .request_frame
    );
    assert_eq!(
        tree.frame([180.0, 140.0]).unwrap().layout_stats,
        LayoutStats::default()
    );
    let mut tx = Transaction::default();
    tx.layout_style(node, fixed(20.0, 10.0).display(LayoutDisplay::None));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert_eq!(
        tree.frame([180.0, 140.0])
            .unwrap()
            .layout_stats
            .measure_calls,
        0
    );
    let mut tx = Transaction::default();
    tx.layout_style(node, fixed(20.0, 10.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    assert!(
        tree.frame([180.0, 140.0])
            .unwrap()
            .layout_stats
            .measure_calls
            > 0
    );
    drop(original);
    tree.remove(node).unwrap();
    assert!(tree.content_measures.is_empty());
    assert_eq!(dead.load(Ordering::Relaxed), 1);
    let expected = pixels(old.clone());
    drop(tree);
    assert_eq!(pixels(old), expected);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_content_measure_local_updates_match_full_flex_reference() {
    let (mut tree, node, _) = fixture(true, 12);
    let (mut full, ref_node, _) = fixture(false, 12);
    full.set_frame_build_policy(FrameBuildPolicy::Full);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("content measure adapter: {:?}", gpu.adapter_info());
    for width in [36.0, 76.0, 116.0, 36.0] {
        for (owner, id) in [(&mut tree, node), (&mut full, ref_node)] {
            let mut style = host();
            style.size.width = LayoutDimension::Px(width);
            let mut tx = Transaction::default();
            tx.layout_style(id, style);
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let frame = tree.local_frame([180.0, 140.0]).unwrap();
        gpu.submit_local(&frame, [180, 140], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&full.frame([180.0, 140.0]).unwrap(), [180, 140], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
