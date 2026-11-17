use super::*;
use std::sync::{Mutex, atomic::AtomicUsize};

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
fn host(direction: LayoutDirection) -> LayoutStyle {
    LayoutStyle::row()
        .size(LayoutDimension::Percent(1.0), LayoutDimension::Auto)
        .direction(direction)
        .flex_wrap(LayoutFlexWrap::Wrap)
        .align_items(LayoutAlignItems::Start)
        .align_content(LayoutAlignContent::Start)
        .padding(LayoutEdges::all(LayoutLength::Px(3.0)))
        .gap(LayoutLength::Px(2.0), LayoutLength::Px(2.0))
        .flex_shrink(0.0)
}
fn close(actual: LayoutResult, expected: LayoutResult) {
    let mut a = vec![];
    let mut b = vec![];
    for (value, target) in [
        (actual.location.as_slice(), expected.location.as_slice()),
        (actual.size.as_slice(), expected.size.as_slice()),
        (
            actual.content_size.as_slice(),
            expected.content_size.as_slice(),
        ),
        (actual.border.as_slice(), expected.border.as_slice()),
        (actual.padding.as_slice(), expected.padding.as_slice()),
        (actual.margin.as_slice(), expected.margin.as_slice()),
        (
            actual.scrollable_overflow.as_slice(),
            expected.scrollable_overflow.as_slice(),
        ),
    ] {
        a.extend_from_slice(value);
        b.extend_from_slice(target);
    }
    for (value, target) in [
        (actual.baseline, expected.baseline),
        (actual.last_baseline, expected.last_baseline),
    ] {
        assert_eq!(value.is_some(), target.is_some());
        if let (Some(value), Some(target)) = (value, target) {
            a.push(value);
            b.push(target);
        }
    }
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 0.0001),
        "{actual:?} versus {expected:?}"
    );
}

/// An application-owned example, not a second built-in Flex implementation.
/// It supports finite row wrapping with numeric margins/bases, delegating each
/// actual child subtree to the normal kernel. Other policies reject explicitly.
#[derive(Debug)]
struct StyleWrap {
    direction: LayoutDirection,
    calls: Arc<AtomicUsize>,
}
fn resolved(value: LayoutLength, width: Option<f32>) -> Result<f32, Error> {
    match value {
        LayoutLength::Px(v) => Ok(v),
        LayoutLength::Percent(v) => Ok(v * width.unwrap_or(0.0)),
        LayoutLength::Auto => Err(Error::InvalidParentLayout),
    }
}
impl StyleWrap {
    fn plan(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        assert!(children.style(children.len()).is_err());
        let width = input.known_size.width.or(match input.available.width {
            LayoutAvailableSpace::Definite(w) => Some(w),
            _ => None,
        });
        let mut x: f32 = 0.0;
        let mut y: f32 = 0.0;
        let mut line_height: f32 = 0.0;
        let mut used_width: f32 = 0.0;
        let mut first = None;
        let mut last = None;
        let mut populated = false;
        let mut placements = vec![];
        for child in 0..children.len() {
            let style = children.style(child)?;
            if style.hidden {
                placements.push(ParentPlacement {
                    child,
                    input: ChildLayoutInput::new(LayoutConstraints::new(
                        LayoutAvailableSpace::Unbounded,
                        LayoutAvailableSpace::Unbounded,
                    )),
                    origin: [0.0; 2],
                });
                continue;
            }
            if style.position != LayoutPosition::Static || style.flex_grow != 0.0 {
                return Err(Error::InvalidParentLayout);
            }
            let [left, right, top, bottom] = [
                style.margin.left,
                style.margin.right,
                style.margin.top,
                style.margin.bottom,
            ]
            .map(|v| resolved(v, input.percentage_basis.width));
            let (left, right, top, bottom) = (left?, right?, top?, bottom?);
            let mut slot = ChildLayoutInput {
                constraints: LayoutConstraints::new(
                    width.map_or(LayoutAvailableSpace::MaxContent, |w| {
                        LayoutAvailableSpace::Definite((w - left - right).max(0.0))
                    }),
                    LayoutAvailableSpace::Unbounded,
                ),
                percentage_basis: input.percentage_basis,
                ..ChildLayoutInput::default()
            };
            match style.flex_basis {
                LayoutDimension::Auto => {}
                LayoutDimension::Px(mut basis) => {
                    if style.box_sizing == LayoutBoxSizing::ContentBox {
                        for inset in [
                            style.padding.left,
                            style.padding.right,
                            style.border.left,
                            style.border.right,
                        ] {
                            basis += resolved(inset, input.percentage_basis.width)?;
                        }
                    }
                    slot.constraints.bounds =
                        BoxConstraints::new([basis, 0.0], [basis, f32::INFINITY]);
                }
                _ => return Err(Error::InvalidParentLayout),
            }
            let result = children.measure_baselines(child, slot)?;
            let outer = result.size[0] + left + right;
            let gap = if populated { 2.0 } else { 0.0 };
            if populated && width.is_some_and(|w| x + gap + outer > w) {
                used_width = used_width.max(x);
                y += line_height + 2.0;
                line_height = 0.0;
                x = 0.0;
                populated = false;
            }
            if populated {
                x += 2.0;
            }
            let origin_x = match self.direction {
                LayoutDirection::Ltr => x + left,
                LayoutDirection::Rtl => {
                    width.ok_or(Error::InvalidParentLayout)? - x - right - result.size[0]
                }
            };
            let origin_y = y + top;
            first.get_or_insert(origin_y + result.baseline.unwrap_or(result.size[1]));
            last = Some(origin_y + result.last_baseline.unwrap_or(result.size[1]));
            placements.push(ParentPlacement {
                child,
                input: slot,
                origin: [origin_x, origin_y],
            });
            x += outer;
            line_height = line_height.max(result.size[1] + top + bottom);
            populated = true;
        }
        Ok(ParentLayoutOutput {
            size: [
                input.known_size.width.unwrap_or(used_width.max(x)),
                input.known_size.height.unwrap_or(y + line_height),
            ],
            baseline: first,
            last_baseline: last,
            placements,
        })
    }
}
impl ParentLayout for StyleWrap {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        self.plan(input, children)
    }
}
impl ParentMeasure for StyleWrap {
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error> {
        let plan = self.plan(input, children)?;
        Ok(ParentMeasureOutput {
            size: plan.size,
            baseline: plan.baseline,
            last_baseline: plan.last_baseline,
        })
    }
}
fn snapshot(direction: LayoutDirection) -> (ParentLayoutSnapshot, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        ParentLayoutSnapshot::new(StyleWrap {
            direction,
            calls: calls.clone(),
        })
        .with_measure(StyleWrap {
            direction,
            calls: calls.clone(),
        }),
        calls,
    )
}
fn fixture(
    custom: bool,
    direction: LayoutDirection,
) -> (UiTree, NodeId, Vec<NodeId>, NodeId, Arc<AtomicUsize>) {
    let mut tree = UiTree::new_layout(
        LayoutStyle::column().align_items(LayoutAlignItems::Start),
        0,
    )
    .unwrap();
    let root = tree.root();
    let parent = tree
        .append_layout(root, host(direction), 0x0000ffff)
        .unwrap();
    let mut nodes = vec![];
    for i in 0..4 {
        let mut style = fixed(18.0 + i as f32 * 4.0, 12.0 + i as f32 * 2.0);
        style.margin = LayoutEdges {
            left: LayoutLength::Px(1.0),
            right: LayoutLength::Percent(0.025),
            top: LayoutLength::Px(1.0),
            bottom: LayoutLength::Px(2.0),
        };
        if i == 1 {
            style.size.width = LayoutDimension::Auto;
            style.size.height = LayoutDimension::Auto;
            style.flex_wrap = LayoutFlexWrap::Wrap;
            style.align_content = LayoutAlignContent::Start.into();
            style.flex_basis = LayoutDimension::Px(24.0);
            style.box_sizing = LayoutBoxSizing::ContentBox;
            style.padding = LayoutEdges::all(LayoutLength::Px(1.0));
        }
        let node = tree.append_layout(parent, style, 0xff0000ff).unwrap();
        if i == 1 {
            for _ in 0..6 {
                tree.append_layout(node, fixed(10.0, 8.0), 0x00ff00ff)
                    .unwrap();
            }
        }
        nodes.push(node);
    }
    let footer = tree
        .append_layout(root, fixed(20.0, 5.0), 0xffffffff)
        .unwrap();
    let (snapshot, calls) = snapshot(direction);
    if custom {
        tests::install(&mut tree, parent, Some(snapshot));
    } else {
        tree.set_frame_build_policy(FrameBuildPolicy::Full);
    }
    (tree, parent, nodes, footer, calls)
}

#[test]
fn child_styles_drive_wrapping_and_normal_updates_against_ordinary_full_flex() {
    for local in [false, true] {
        for direction in [LayoutDirection::Ltr, LayoutDirection::Rtl] {
            let (mut tree, parent, nodes, footer, calls) = fixture(true, direction);
            let (mut full, ref_parent, ref_nodes, ref_footer, _) = fixture(false, direction);
            let mut old = None;
            for (step, width) in [100.0, 65.0, 100.0, 65.0, 100.0].into_iter().enumerate() {
                for (owner, ids, owner_parent) in [
                    (&mut tree, &nodes, parent),
                    (&mut full, &ref_nodes, ref_parent),
                ] {
                    let mut tx = Transaction::default();
                    tx.layout_style(
                        owner_parent,
                        host(direction).size(LayoutDimension::Px(width), LayoutDimension::Auto),
                    );
                    if step == 1 {
                        tx.layout_style(ids[2], fixed(17.0, 11.0).display(LayoutDisplay::None));
                    }
                    if step == 2 {
                        tx.layout_style(ids[2], fixed(35.0, 22.0));
                        tx.reorder(owner_parent, vec![ids[3], ids[0], ids[1], ids[2]]);
                    }
                    if step == 3 {
                        tx.layout_style(ids[0], fixed(26.0, 16.0));
                    }
                    owner.commit(tx, BackendCapabilities::default()).unwrap();
                }
                let frame = if local {
                    tree.local_frame_in_viewport(constraints(width), [180.0, 180.0])
                        .unwrap()
                        .resolve()
                } else {
                    tree.frame_in_viewport(constraints(width), [180.0, 180.0])
                        .unwrap()
                };
                let reference = full
                    .frame_in_viewport(constraints(width), [180.0, 180.0])
                    .unwrap();
                for (node, reference) in std::iter::once((parent, ref_parent))
                    .chain(nodes.iter().copied().zip(ref_nodes.iter().copied()))
                    .chain([(footer, ref_footer)])
                {
                    let actual = tree.layout_result(node).unwrap();
                    let mut expected = full.layout_result(reference).unwrap();
                    // ParentPlacement owns physical placement. The existing
                    // custom protocol publishes zero CSS margin metadata even
                    // when this client uses authored margins in its arithmetic.
                    if node != parent && node != footer {
                        assert_eq!(actual.margin, [0.0; 4]);
                        expected.margin = [0.0; 4];
                    }
                    close(actual, expected);
                }
                assert_eq!(tests::pixels(frame.clone()), tests::pixels(reference));
                old.get_or_insert(frame);
            }
            let before = calls.load(Ordering::Relaxed);
            tree.frame_in_viewport(constraints(100.0), [180.0, 180.0])
                .unwrap();
            assert_eq!(calls.load(Ordering::Relaxed), before);
            let old = old.unwrap();
            let bytes = tests::pixels(old.clone());
            drop((tree, full));
            assert_eq!(tests::pixels(old), bytes);
        }
    }
}

#[derive(Debug)]
struct Inspect(Arc<Mutex<Vec<ChildLayoutStyle>>>);
impl ParentLayout for Inspect {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error> {
        let style = children.style(0)?;
        self.0.lock().unwrap().push(style);
        let slot = ChildLayoutInput {
            constraints: LayoutConstraints::viewport(100.0, 100.0),
            percentage_basis: input.percentage_basis,
            ..ChildLayoutInput::default()
        };
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

#[test]
fn effective_item_styles_preserve_unresolved_values_and_replaced_image_classification() {
    let observed = Arc::new(Mutex::new(vec![]));
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    let root = tree.root();
    let leaf = tree.append_layout(root, fixed(20.0, 10.0), 0).unwrap();
    tests::install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(Inspect(observed.clone()))),
    );
    let dimensions = [
        LayoutDimension::Px(23.0),
        LayoutDimension::Percent(0.4),
        LayoutDimension::Auto,
        LayoutDimension::MinContent,
        LayoutDimension::MaxContent,
        LayoutDimension::FitContent,
        LayoutDimension::FitContentPx(30.0),
        LayoutDimension::FitContentPercent(0.7),
        LayoutDimension::Stretch,
    ];
    let alignments = [
        LayoutAlignSelf::Auto.into(),
        LayoutAlignSelf::Normal.into(),
        LayoutAlignSelf::Start.safe(),
        LayoutAlignSelf::End.unsafe_overflow(),
        LayoutAlignSelf::FlexStart.into(),
        LayoutAlignSelf::FlexEnd.into(),
        LayoutAlignSelf::SelfStart.into(),
        LayoutAlignSelf::SelfEnd.into(),
        LayoutAlignSelf::Center.into(),
        LayoutAlignSelf::Stretch.into(),
        LayoutAlignSelf::Baseline.into(),
        LayoutAlignSelf::LastBaseline.into(),
    ];
    for i in 0..alignments.len() {
        let mut style = fixed(20.0, 10.0);
        style.size.width = dimensions[i % dimensions.len()];
        style.min_size.width = dimensions[(i + 1) % dimensions.len()];
        style.max_size.width = dimensions[(i + 2) % dimensions.len()];
        style.flex_basis = if i == 0 {
            LayoutDimension::Content
        } else {
            dimensions[(i + 3) % dimensions.len()]
        };
        style.flex_grow = i as f32;
        style.flex_shrink = 0.5;
        style.align_self = alignments[i];
        style.direction = if i % 2 == 0 {
            LayoutDirection::Rtl
        } else {
            LayoutDirection::Ltr
        };
        style.margin = LayoutEdges {
            left: LayoutLength::Px(-2.0),
            right: LayoutLength::Percent(0.1),
            top: LayoutLength::Auto,
            bottom: LayoutLength::Px(3.0),
        };
        style.inset = style.margin;
        style.padding = LayoutEdges::all(LayoutLength::Percent(0.02));
        style.border = LayoutEdges::all(LayoutLength::Px(1.0));
        style.position = LayoutPosition::Relative;
        style.box_sizing = LayoutBoxSizing::ContentBox;
        style.container_type = [
            LayoutContainerType::Normal,
            LayoutContainerType::InlineSize,
            LayoutContainerType::Size,
        ][i % 3];
        style.overflow = LayoutSize::new(LayoutOverflow::Scroll, LayoutOverflow::Hidden);
        style.aspect_ratio = Some(2.0);
        style.contain_intrinsic_size = LayoutSize::new(Some(i as f32), Some(25.0));
        let mut tx = Transaction::default();
        tx.layout_style(leaf, style.clone());
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        tree.frame([100.0, 100.0]).unwrap();
        let item = *observed.lock().unwrap().last().unwrap();
        assert_eq!(item.size, style.size);
        assert_eq!(item.min_size, style.min_size);
        assert_eq!(item.max_size, style.max_size);
        assert_eq!(item.flex_basis, style.flex_basis);
        assert_eq!(item.flex_grow, style.flex_grow);
        assert_eq!(item.flex_shrink, style.flex_shrink);
        assert_eq!(item.align_self, style.align_self);
        assert_eq!(item.margin, style.margin);
        assert_eq!(item.inset, style.inset);
        assert_eq!(item.padding, style.padding);
        assert_eq!(item.border, style.border);
        assert_eq!(item.direction, style.direction);
        assert_eq!(item.position, style.position);
        assert_eq!(item.box_sizing, style.box_sizing);
        assert_eq!(item.aspect_ratio, style.aspect_ratio);
        assert_eq!(item.contain_intrinsic_size, style.contain_intrinsic_size);
        assert_eq!(item.overflow, style.overflow);
        assert_eq!(
            item.size_containment,
            LayoutSize::new(i % 3 != 0, i % 3 == 2)
        );
        assert_eq!(item.layout_containment, i % 3 != 0);
        assert!(!item.paint_containment);
        assert!(!item.hidden);
        assert!(!item.is_replaced);
        assert!(!item.is_compressible_replaced);
    }
    let mut tx = Transaction::default();
    tx.layout_style(leaf, fixed(20.0, 10.0).display(LayoutDisplay::None));
    tx.image(
        leaf,
        Some(ImageResource::rgba8([2, 2], vec![255; 16]).unwrap()),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    let item = *observed.lock().unwrap().last().unwrap();
    assert!(item.hidden && item.is_replaced && item.is_compressible_replaced);
    let mut tx = Transaction::default();
    tx.image(leaf, None);
    tx.layout_style(leaf, fixed(20.0, 10.0));
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree.frame([100.0, 100.0]).unwrap();
    assert!(!observed.lock().unwrap().last().unwrap().is_replaced);
}

#[test]
fn typed_style_sources_coalesce_and_measured_policy_errors_rollback_atomic_frames() {
    use crate::retained::{logical::LogicalUi, reactive::Property};
    for local in [false, true] {
        let (tree, _, nodes, _, _) = fixture(true, LayoutDirection::Ltr);
        let mut ui = LogicalUi::from_tree(tree, BackendCapabilities::default()).unwrap();
        let scope = ui.root_scope();
        let original = fixed(18.0, 12.0);
        let signal = ui.state(scope, 100, || original.clone()).unwrap();
        ui.bind(signal, Property::layout_style(nodes[0])).unwrap();
        let old = ui
            .frame_in_viewport(constraints(100.0), [180.0, 180.0])
            .unwrap();
        ui.transaction(|tx| {
            tx.set(signal, fixed(30.0, 20.0))?;
            tx.set(signal, original.clone())?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            ui.frame_in_viewport(constraints(100.0), [180.0, 180.0])
                .unwrap()
                .layout_stats,
            LayoutStats::default()
        );
        let before = ui.layout_result(nodes[0]).unwrap();
        let unsupported = fixed(30.0, 20.0).flex_grow(1.0);
        let result = if local {
            ui.edit_local_frame(
                constraints(100.0),
                [180.0, 180.0],
                |tx| {
                    tx.set(signal, unsupported)?;
                    Ok(())
                },
                |(), _| Ok(()),
            )
            .map(|_| ())
        } else {
            ui.edit_frame(
                constraints(100.0),
                [180.0, 180.0],
                |tx| {
                    tx.set(signal, unsupported)?;
                    Ok(())
                },
                |(), _| Ok(()),
            )
            .map(|_| ())
        };
        assert!(matches!(result, Err(Error::InvalidParentLayout)));
        assert_eq!(ui.read(signal).unwrap(), &original);
        assert_eq!(ui.layout_result(nodes[0]).unwrap(), before);
        assert_eq!(ui.published_frame().unwrap().revision, old.revision);
        ui.transaction(|tx| {
            tx.set(signal, fixed(30.0, 20.0))?;
            Ok(())
        })
        .unwrap();
        ui.frame_in_viewport(constraints(100.0), [180.0, 180.0])
            .unwrap();
        assert_eq!(ui.layout_result(nodes[0]).unwrap().size, [30.0, 20.0]);
    }
}

#[test]
fn unsupported_raw_item_style_rejects_without_corrupting_live_frame_or_query_state() {
    let observed = Arc::new(Mutex::new(vec![]));
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    let root = tree.root();
    let leaf = tree
        .append_layout(root, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    tests::install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(Inspect(observed))),
    );
    let old = tree.frame([100.0, 100.0]).unwrap();
    let before = tree.layout_result(leaf).unwrap();
    let mut raw = fixed(20.0, 10.0).to_taffy().unwrap();
    raw.position = taffy::Position::Sticky;
    let mut tx = Transaction::default();
    tx.style(leaf, raw);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let state = format!("{:?}", tree.layout);
    assert_eq!(
        tree.frame([100.0, 100.0]).unwrap_err(),
        Error::InvalidParentLayout
    );
    assert_eq!(
        tree.dry_measure_node(root, ChildLayoutInput::new(constraints(100.0)))
            .unwrap_err(),
        Error::InvalidParentLayout
    );
    assert_eq!(format!("{:?}", tree.layout), state);
    assert_eq!(tree.layout_result(leaf).unwrap(), before);
    assert_eq!(tree.published_frame().unwrap().revision, old.revision);
}

#[test]
fn style_reads_follow_reparent_and_queries_leave_geometry_and_frames_unchanged() {
    let observed = Arc::new(Mutex::new(vec![]));
    let mut tree = UiTree::new_layout(LayoutStyle::column(), 0).unwrap();
    let root = tree.root();
    let a = tree.append_layout(root, fixed(100.0, 60.0), 0).unwrap();
    let b = tree.append_layout(root, fixed(100.0, 60.0), 0).unwrap();
    let leaf = tree
        .append_layout(a, fixed(20.0, 10.0), 0xff0000ff)
        .unwrap();
    let mut raw = fixed(20.0, 10.0).to_taffy().unwrap();
    raw.scrollbar_width = 4.0;
    raw.contain = taffy::Contain::PAINT;
    raw.overflow = taffy::Point {
        x: taffy::Overflow::Clip,
        y: taffy::Overflow::Scroll,
    };
    let mut tx = Transaction::default();
    tx.style(leaf, raw);
    tx.parent_layout(
        a,
        Some(ParentLayoutSnapshot::new(Inspect(observed.clone()))),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let old = tree.frame([180.0, 180.0]).unwrap();
    let before = tree.layout_result(leaf).unwrap();
    let style = *observed.lock().unwrap().last().unwrap();
    assert_eq!(style.scrollbar_width, 4.0);
    assert!(style.paint_containment);
    assert_eq!(
        style.overflow,
        LayoutSize::new(LayoutOverflow::Clip, LayoutOverflow::Scroll)
    );
    for baselines in [false, true] {
        let input = ChildLayoutInput::new(constraints(100.0));
        let answer = if baselines {
            tree.dry_measure_node_baselines(a, input)
        } else {
            tree.dry_measure_node(a, input)
        }
        .unwrap();
        assert_eq!(answer.size, [100.0, 60.0]);
        assert_eq!(tree.layout_result(leaf).unwrap(), before);
        assert_eq!(tree.published_frame().unwrap().revision, old.revision);
    }
    tree.edit(BackendCapabilities::default(), |edit| {
        edit.reparent(leaf, b)?;
        edit.properties().parent_layout(a, None);
        edit.properties().parent_layout(
            b,
            Some(ParentLayoutSnapshot::new(Inspect(observed.clone()))),
        );
        Ok(())
    })
    .unwrap();
    tree.local_frame([180.0, 180.0]).unwrap();
    assert_eq!(*observed.lock().unwrap().last().unwrap(), style);
    assert_eq!(tree.layout_result(leaf).unwrap(), before);
    assert_eq!(old.hit_test(1.0, 1.0), Some(leaf));
}

#[derive(Debug)]
struct Independent;
impl ParentAllocationLayout for Independent {
    fn allocate(
        &self,
        _: ParentAllocationInput,
        count: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Ok(vec![
            ChildLayoutInput::new(
                LayoutConstraints::viewport(30.0, 20.0)
                    .with_bounds(BoxConstraints::tight([30.0, 20.0]))
            );
            count
        ])
    }
}
#[test]
fn measured_style_access_cannot_replace_enforced_slots_or_authorize_feedback_builders() {
    use crate::retained::logical::LogicalUi;
    let mut tree = UiTree::new_layout(fixed(100.0, 100.0), 0).unwrap();
    let root = tree.root();
    let leaf = tree
        .append_layout(root, fixed(50.0, 40.0), 0xff0000ff)
        .unwrap();
    let observed = Arc::new(Mutex::new(vec![]));
    tests::install(
        &mut tree,
        root,
        Some(
            ParentLayoutSnapshot::new(Inspect(observed.clone()))
                .with_allocation_layout(Independent),
        ),
    );
    tree.frame([180.0, 180.0]).unwrap();
    assert_eq!(tree.layout_result(leaf).unwrap().size, [30.0, 20.0]);
    assert_eq!(
        observed.lock().unwrap().last().unwrap().size,
        LayoutSize::new(LayoutDimension::Px(50.0), LayoutDimension::Px(40.0))
    );
    tests::install(
        &mut tree,
        root,
        Some(ParentLayoutSnapshot::new(Inspect(observed))),
    );
    let mut ui = LogicalUi::from_tree(tree, BackendCapabilities::default()).unwrap();
    let old = ui.frame([180.0, 180.0]).unwrap();
    let result = ui.edit_frame(
        constraints(100.0),
        [180.0, 180.0],
        |_| Ok(()),
        |(), edit| {
            edit.allocation_builder(leaf, |_, _| -> Result<(), Error> {
                panic!("measured style must not authorize independent construction")
            })
        },
    );
    assert!(matches!(result, Err(Error::UnsafeConstraintBuilder)));
    assert_eq!(ui.published_frame().unwrap().revision, old.revision);
}

#[cfg(feature = "retained-wgpu")]
#[test]
#[ignore = "requires a WGPU adapter; run explicitly"]
fn hardware_child_style_policy_updates_match_ordinary_full_flex() {
    let (mut tree, parent, nodes, _, _) = fixture(true, LayoutDirection::Rtl);
    let (mut full, ref_parent, ref_nodes, _, _) = fixture(false, LayoutDirection::Rtl);
    let mut gpu = crate::retained::wgpu::WgpuFrameRenderer::new(2).unwrap();
    eprintln!("child style adapter: {:?}", gpu.adapter_info());
    for (step, width) in [100.0, 65.0, 100.0].into_iter().enumerate() {
        for (owner, parent, ids) in [
            (&mut tree, parent, &nodes),
            (&mut full, ref_parent, &ref_nodes),
        ] {
            let mut tx = Transaction::default();
            tx.layout_style(
                parent,
                host(LayoutDirection::Rtl).size(LayoutDimension::Px(width), LayoutDimension::Auto),
            );
            if step == 1 {
                tx.layout_style(ids[2], fixed(35.0, 22.0));
            }
            owner.commit(tx, BackendCapabilities::default()).unwrap();
        }
        let frame = tree
            .local_frame_in_viewport(constraints(width), [180.0, 180.0])
            .unwrap();
        let reference = full
            .frame_in_viewport(constraints(width), [180.0, 180.0])
            .unwrap();
        gpu.submit_local(&frame, [180, 180], 1.0).unwrap().unwrap();
        assert_eq!(
            gpu.image().unwrap().as_raw(),
            gpu.reference_image(&reference, [180, 180], 1.0)
                .unwrap()
                .as_raw()
        );
    }
    gpu.finish().unwrap();
}
