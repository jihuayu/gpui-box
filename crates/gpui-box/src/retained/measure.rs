//! Owned, pure intrinsic measurement inputs. No App/Window borrow survives a frame.
use super::*;
use taffy::util::ResolveOrZero;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageIntrinsicSize {
    pub width: f32,
    pub height: f32,
}

/// Fixed local content metrics produced outside layout, for a custom leaf.
/// The baseline is measured from the content-box top in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomMeasureSnapshot {
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
}

/// Sizing inputs only: changing image pixels at the same intrinsic size must
/// use the resource channel, not invent a new intrinsic measurement revision.
#[derive(Clone, Debug, PartialEq)]
pub enum MeasureSnapshot {
    Image(ImageIntrinsicSize),
    Fixed(CustomMeasureSnapshot),
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    Paragraph(paragraph::ParagraphSnapshot),
}

/// Counts only callbacks actually reached by Taffy, after its internal cache.
/// `leaf_layouts` is not the number of all container/layout-node visits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutStats {
    pub leaf_layouts: u64,
    pub measure_calls: u64,
    /// Actual unrounded-layout writes, including equal and transient writes.
    pub layout_writes: u64,
    pub changed_layout_writes: u64,
    /// Unique nodes whose final result differs from the last published result.
    pub net_changed_nodes: u64,
}

impl MeasureSnapshot {
    pub(super) fn validate(&self) -> Result<(), Error> {
        let (width, height, baseline) = match self {
            Self::Image(image) => (image.width, image.height, None),
            Self::Fixed(value) => (value.width, value.height, value.baseline),
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => (
                value.max_content_width(),
                value.line_height() * value.hard_line_count() as f32,
                Some(value.first_baseline()),
            ),
        };
        if [width, height]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
            || baseline.is_some_and(|value| !value.is_finite() || value < 0.0 || value > height)
        {
            return Err(Error::InvalidMeasure);
        }
        Ok(())
    }

    fn size(&self, known: Size<Option<f32>>, _available: Size<AvailableSpace>) -> Size<f32> {
        match self {
            Self::Image(image) => match (known.width, known.height) {
                (Some(width), Some(height)) => Size { width, height },
                (Some(width), None) if image.width > 0.0 => Size {
                    width,
                    height: width / image.width * image.height,
                },
                (None, Some(height)) if image.height > 0.0 => Size {
                    width: height / image.height * image.width,
                    height,
                },
                _ => Size {
                    width: known.width.unwrap_or(image.width),
                    height: known.height.unwrap_or(image.height),
                },
            },
            Self::Fixed(value) => Size {
                width: known.width.unwrap_or(value.width),
                height: known.height.unwrap_or(value.height),
            },
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => value.measure(_available.width),
        }
    }

    pub(super) fn baseline(&self) -> Option<f32> {
        match self {
            Self::Fixed(value) => value.baseline,
            Self::Image(_) => None,
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => Some(value.first_baseline()),
        }
    }
}

/// Use Taffy's leaf algorithm for box sizing, percentages, min/max and overflow.
/// The actual content callback reads only an owned immutable input snapshot.
pub(super) fn layout_leaf(
    inputs: taffy::tree::LayoutInput,
    snapshot: Option<&MeasureSnapshot>,
    style: &Style,
    stats: &mut LayoutStats,
) -> taffy::tree::LayoutOutput {
    stats.leaf_layouts += 1;
    let mut output = taffy::compute_leaf_layout(
        inputs,
        style,
        |_, _| 0.0,
        |known, available| {
            let Some(snapshot) = snapshot else {
                return Size::ZERO;
            };
            stats.measure_calls += 1;
            // Available space alone is not a known content dimension.
            snapshot.size(known, available)
        },
    );
    let resolve = |value: taffy::LengthPercentage| {
        value.resolve_or_zero(inputs.parent_size.width, |_, _| 0.0)
    };
    if matches!(snapshot, Some(MeasureSnapshot::Image(_))) {
        // A replaced image fits its allocated content box. Its natural bitmap
        // dimensions must not create overflow when CSS constraints resize it.
        let padding = style.padding.map(resolve);
        let border = style.border.map(resolve);
        let scroll_x = style.overflow.x == taffy::Overflow::Scroll;
        let scroll_y = style.overflow.y == taffy::Overflow::Scroll;
        let rtl = style.direction == taffy::Direction::Rtl;
        output.scrollable_overflow_rect = taffy::Rect {
            left: 0.0,
            top: 0.0,
            right: (output.size.width
                - border.left
                - border.right
                - if rtl { padding.left } else { padding.right }
                - if scroll_y { style.scrollbar_width } else { 0.0 }
                + if scroll_x {
                    if rtl { padding.left } else { padding.right }
                } else {
                    0.0
                })
            .max(0.0),
            bottom: (output.size.height
                - border.top
                - border.bottom
                - padding.bottom
                - if scroll_x { style.scrollbar_width } else { 0.0 }
                + if scroll_y { padding.bottom } else { 0.0 })
            .max(0.0),
        };
    }
    if let Some(baseline) = snapshot.and_then(MeasureSnapshot::baseline) {
        // Resolve percentage top padding/border against containing inline size,
        // as the leaf algorithm does. Baseline output remains in border-box space.
        output.baselines = taffy::tree::Baselines::from_first(Some(
            baseline + resolve(style.padding.top) + resolve(style.border.top),
        ));
    }
    output
}

pub(super) fn effective_style(style: &Style, snapshot: Option<&MeasureSnapshot>) -> Style {
    let mut effective = style.clone();
    if matches!(snapshot, Some(MeasureSnapshot::Image(_))) {
        effective.item_is_replaced = true;
    }
    if effective.aspect_ratio.is_none()
        && let Some(MeasureSnapshot::Image(image)) = snapshot
        && image.width > 0.0
        && image.height > 0.0
    {
        effective.aspect_ratio = Some(image.width / image.height);
    }
    effective
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: f32, height: f32) -> Option<MeasureSnapshot> {
        Some(MeasureSnapshot::Image(ImageIntrinsicSize { width, height }))
    }

    fn fixed(width: f32, height: f32, baseline: Option<f32>) -> Option<MeasureSnapshot> {
        Some(MeasureSnapshot::Fixed(CustomMeasureSnapshot {
            width,
            height,
            baseline,
        }))
    }

    fn fixture() -> UiTree {
        UiTree::new(
            Style {
                flex_direction: taffy::FlexDirection::Column,
                align_items: taffy::AlignItems::START,
                size: Size {
                    width: percent(1.0),
                    height: percent(1.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap()
    }

    fn set(tree: &mut UiTree, node: NodeId, snapshot: Option<MeasureSnapshot>) -> Commit {
        let mut tx = Transaction::default();
        tx.measure(node, snapshot);
        tree.commit(tx, BackendCapabilities::default()).unwrap()
    }

    fn bounds(frame: &FramePacket, node: NodeId) -> Rect {
        frame
            .items
            .iter()
            .find(|item| item.node == node)
            .unwrap()
            .bounds
    }

    #[test]
    fn owned_intrinsic_inputs_relayout_dependents_and_reuse_unaffected_measure_cache() {
        let mut tree = fixture();
        let a = tree
            .append(tree.root(), Style::default(), 0xff0000ff)
            .unwrap();
        let b = tree
            .append(tree.root(), Style::default(), 0x00ff00ff)
            .unwrap();
        set(&mut tree, a, fixed(40.0, 20.0, None));
        set(&mut tree, b, fixed(30.0, 10.0, None));
        let before = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(bounds(&before, a).height, 20.0);
        assert_eq!(bounds(&before, b).y, 20.0);
        assert!(before.layout_stats.measure_calls >= 2);
        let equal = set(&mut tree, a, fixed(40.0, 20.0, None));
        assert!(!equal.request_frame);
        assert_eq!(
            tree.frame([200.0, 200.0]).unwrap().layout_stats,
            LayoutStats::default()
        );

        set(&mut tree, a, fixed(40.0, 50.0, None));
        let after = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(bounds(&after, b).y, 50.0);
        assert_eq!(after.recorded_chunks, 0);
        assert!(Arc::ptr_eq(
            &before.items[1].commands,
            &after.items[1].commands
        ));
        // The second intrinsic leaf is cached despite its world position changing.
        assert!(after.layout_stats.measure_calls < before.layout_stats.measure_calls);
        let mut tx = Transaction::default();
        tx.color(a, 0x112233ff);
        tx.translation(b, [2.0, 3.0]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            tree.frame([200.0, 200.0]).unwrap().layout_stats,
            LayoutStats::default()
        );
        // Published geometry survives input replacement and owner disposal.
        drop(tree);
        assert_eq!(bounds(&before, b).y, 20.0);
    }

    #[test]
    fn images_keep_ratio_with_explicit_dimensions_and_constraints_are_not_known_sizes() {
        let mut tree = fixture();
        let a = tree.append(tree.root(), Style::default(), 0).unwrap();
        set(&mut tree, a, image(40.0, 20.0));
        let before = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(
            (bounds(&before, a).width, bounds(&before, a).height),
            (40.0, 20.0)
        );
        for (width, height, expected) in [
            (length(80.0), taffy::Dimension::auto(), (80.0, 40.0)),
            (taffy::Dimension::auto(), length(30.0), (60.0, 30.0)),
            (length(80.0), length(30.0), (80.0, 30.0)),
        ] {
            let mut tx = Transaction::default();
            tx.style(
                a,
                Style {
                    size: Size { width, height },
                    ..Default::default()
                },
            );
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let rect = bounds(&tree.frame([200.0, 200.0]).unwrap(), a);
            assert_eq!((rect.width, rect.height), expected);
        }
        let mut tx = Transaction::default();
        tx.style(
            a,
            Style {
                size: Size {
                    width: percent(0.5),
                    height: taffy::Dimension::auto(),
                },
                ..Default::default()
            },
        );
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let rect = bounds(&tree.frame([200.0, 200.0]).unwrap(), a);
        assert_eq!((rect.width, rect.height), (100.0, 50.0));
        set(&mut tree, a, image(20.0, 20.0));
        let rect = bounds(&tree.frame([200.0, 200.0]).unwrap(), a);
        assert_eq!((rect.width, rect.height), (100.0, 100.0));
    }

    #[test]
    fn fixed_baseline_and_padding_participate_in_parent_flex_alignment() {
        let mut tree = UiTree::new(
            Style {
                align_items: taffy::AlignItems::BASELINE,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let a = tree
            .append(
                tree.root(),
                Style {
                    padding: taffy::Rect {
                        top: length(3.0),
                        ..taffy::Rect::zero()
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        let b = tree.append(tree.root(), Style::default(), 0).unwrap();
        set(&mut tree, a, fixed(30.0, 20.0, Some(15.0)));
        set(&mut tree, b, fixed(20.0, 10.0, Some(7.0)));
        let before = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(bounds(&before, a).y + 18.0, bounds(&before, b).y + 7.0);
        set(&mut tree, a, fixed(30.0, 20.0, Some(5.0)));
        let after = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(bounds(&after, a).y + 8.0, bounds(&after, b).y + 7.0);
        assert_ne!(bounds(&before, b).y, bounds(&after, b).y);
        assert_eq!(after.recorded_chunks, 0);
    }

    #[test]
    fn invalid_measures_and_container_attachment_reject_whole_transaction() {
        let mut tree = fixture();
        let a = tree
            .append(tree.root(), Style::default(), 0x112233ff)
            .unwrap();
        let before = tree.frame([200.0, 200.0]).unwrap();
        for invalid in [
            image(f32::NAN, 1.0),
            image(1.0, -1.0),
            fixed(10.0, 20.0, Some(30.0)),
        ] {
            let mut tx = Transaction::default();
            tx.color(a, 0);
            tx.measure(a, invalid);
            assert_eq!(
                tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
                Error::InvalidMeasure
            );
            assert_eq!(
                tree.frame([200.0, 200.0]).unwrap().revision,
                before.revision
            );
        }
        let mut tx = Transaction::default();
        tx.measure(tree.root(), fixed(10.0, 10.0, None));
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::MeasuredNodeHasChildren
        );
        set(&mut tree, a, fixed(10.0, 10.0, None));
        assert_eq!(
            tree.append(a, Style::default(), 0).unwrap_err(),
            Error::MeasuredNodeHasChildren
        );
        set(&mut tree, a, None);
        tree.append(a, Style::default(), 0).unwrap();
        // Zero-size resources are legal and do not divide by zero.
        let b = tree.append(tree.root(), Style::default(), 0).unwrap();
        set(&mut tree, b, image(0.0, 0.0));
        let rect = bounds(&tree.frame([200.0, 200.0]).unwrap(), b);
        assert_eq!((rect.width, rect.height), (0.0, 0.0));
    }

    #[test]
    fn measure_signal_changes_are_atomic_and_retired_with_logical_owner() {
        use reactive::Property;
        let mut ui =
            logical::LogicalUi::from_tree(fixture(), BackendCapabilities::default()).unwrap();
        let root = ui.root_scope();
        let scope = ui.reconcile_keys(root, &[1]).unwrap().order[0];
        let node = ui
            .append_render(scope, ui.root_render(), Style::default(), 0)
            .unwrap();
        let measure = ui.state(scope, 0, || image(40.0, 20.0)).unwrap();
        ui.bind(measure, Property::measure(node)).unwrap();
        let before = ui.frame([200.0, 200.0]).unwrap();
        let result = ui.transaction(|tx| {
            tx.set(measure, image(-1.0, 20.0))?;
            Ok(())
        });
        assert!(matches!(result, Err(Error::InvalidMeasure)));
        assert_eq!(ui.read(measure).unwrap(), &image(40.0, 20.0));
        assert_eq!(ui.frame([200.0, 200.0]).unwrap().revision, before.revision);
        ui.transaction(|tx| {
            tx.set(measure, image(20.0, 40.0))?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            bounds(&ui.frame([200.0, 200.0]).unwrap(), node).height,
            40.0
        );
        ui.reconcile_keys(root, &[]).unwrap();
        assert!(ui.read(measure).is_err());
        assert_eq!(ui.reactive().binding_count(), 0);
    }

    #[test]
    fn image_resizing_updates_overflow_and_explicit_style_ratio_takes_precedence() {
        let mut tree = fixture();
        let node = tree
            .append(
                tree.root(),
                Style {
                    size: Size {
                        width: length(30.0),
                        height: taffy::Dimension::auto(),
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        set(&mut tree, node, image(400.0, 200.0));
        let frame = tree.frame([200.0, 200.0]).unwrap();
        assert_eq!(bounds(&frame, node).height, 15.0);
        let layout = tree.layout.layout(tree.node(node).unwrap().layout).unwrap();
        assert_eq!((layout.scroll_width(), layout.scroll_height()), (0.0, 0.0));
        let mut tx = Transaction::default();
        let mut style = tree.style(node).unwrap().clone();
        style.aspect_ratio = Some(1.0);
        tx.style(node, style);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            bounds(&tree.frame([200.0, 200.0]).unwrap(), node).height,
            30.0
        );
        // Removing intrinsic data must not leave the inferred image ratio behind.
        let mut tx = Transaction::default();
        tx.style(
            node,
            Style {
                size: Size {
                    width: length(30.0),
                    height: taffy::Dimension::auto(),
                },
                ..Default::default()
            },
        );
        tx.measure(node, None);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            bounds(&tree.frame([200.0, 200.0]).unwrap(), node).height,
            0.0
        );
    }
}
