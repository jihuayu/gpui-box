//! Owned, pure intrinsic measurement inputs. No App/Window borrow survives a frame.
use super::*;
use taffy::util::ResolveOrZero;

/// Resolved soft content space, after the box model removes padding, border
/// and gutters. Definite space is a wrapping opportunity, not a hard size;
/// intrinsic and normal indefinite queries remain distinct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentMeasureInput {
    pub available: LayoutSize<layout::AvailableSpace>,
}

/// Natural content extent and optional horizontal baselines. The box model
/// chooses the used border box independently; content may overflow that box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentMeasureOutput {
    pub size: [f32; 2],
    pub baseline: Option<f32>,
    pub last_baseline: Option<f32>,
}
impl ContentMeasureOutput {
    fn validate(self) -> Result<Self, Error> {
        if self.size.iter().any(|v| !v.is_finite() || *v < 0.0)
            || [self.baseline, self.last_baseline]
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err(Error::InvalidMeasure);
        }
        Ok(self)
    }
}

/// Owned pure content measurement, independent of UI/renderer state. Calls may
/// repeat or be omitted by caches; all dependencies belong to this snapshot.
/// Return natural content extent; do not clamp it to an allocated height.
pub trait ContentMeasure: std::fmt::Debug + Send + Sync + 'static {
    fn measure(&self, input: ContentMeasureInput) -> Result<ContentMeasureOutput, Error>;
}

/// Clone preserves identity. Replace the handle when any captured input changes.
/// Interior mutation, mutable globals and invocation-dependent answers violate
/// purity; the type system cannot freeze user-owned captures.
#[derive(Clone, Debug)]
pub struct ContentMeasureSnapshot(Arc<dyn ContentMeasure>);
impl ContentMeasureSnapshot {
    pub fn new(value: impl ContentMeasure) -> Self {
        Self(Arc::new(value))
    }
}
impl PartialEq for ContentMeasureSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

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

/// Fixed content metrics with independent first and last horizontal baselines.
/// Offsets are from the content-box top, before padding/border. The legacy
/// CustomMeasureSnapshot represents one baseline used for both sets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineMeasureSnapshot {
    pub width: f32,
    pub height: f32,
    pub first_baseline: Option<f32>,
    pub last_baseline: Option<f32>,
}

/// Sizing inputs only: changing image pixels at the same intrinsic size must
/// use the resource channel, not invent a new intrinsic measurement revision.
#[derive(Clone, Debug, PartialEq)]
pub enum MeasureSnapshot {
    Image(ImageIntrinsicSize),
    Fixed(CustomMeasureSnapshot),
    FixedBaselines(BaselineMeasureSnapshot),
    Content(ContentMeasureSnapshot),
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
        let (width, height, baselines) = match self {
            Self::Image(image) => (image.width, image.height, [None, None]),
            Self::Fixed(value) => (value.width, value.height, [value.baseline, value.baseline]),
            Self::FixedBaselines(value) => (
                value.width,
                value.height,
                [value.first_baseline, value.last_baseline],
            ),
            // Constraint-dependent answers are checked when actually queried;
            // committing a snapshot must not invoke arbitrary layout code.
            Self::Content(_) => return Ok(()),
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => (
                value.max_content_width(),
                value.line_height() * value.hard_line_count() as f32,
                [Some(value.first_baseline()), None],
            ),
        };
        if [width, height]
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
            || baselines
                .into_iter()
                .flatten()
                .any(|value| !value.is_finite() || value < 0.0 || value > height)
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
            Self::FixedBaselines(value) => Size {
                width: known.width.unwrap_or(value.width),
                height: known.height.unwrap_or(value.height),
            },
            Self::Content(_) => unreachable!("content delegates use the fallible adapter"),
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => value.measure(_available.width),
        }
    }

    fn baselines(&self, _width: f32) -> taffy::Baselines {
        match self {
            Self::Fixed(value) => taffy::Baselines {
                first: value.baseline,
                last: value.baseline,
            },
            Self::FixedBaselines(value) => taffy::Baselines {
                first: value.first_baseline,
                last: value.last_baseline,
            },
            Self::Image(_) => taffy::Baselines::NONE,
            Self::Content(_) => unreachable!("content delegates use the fallible adapter"),
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            Self::Paragraph(value) => {
                let layout = value.layout(Some(_width)).expect("validated content width");
                taffy::Baselines {
                    first: Some(layout.first_baseline),
                    last: Some(layout.last_baseline),
                }
            }
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
    error: &mut Option<Error>,
) -> taffy::tree::LayoutOutput {
    stats.leaf_layouts += 1;
    if let Some(MeasureSnapshot::Content(snapshot)) = snapshot {
        return layout_content(inputs, snapshot, style, stats, error);
    }
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
            // Size containment suppresses a replaced image's natural aspect
            // ratio, including transfer from externally allocated dimensions.
            // Explicit authored aspect_ratio is still handled by the box model.
            let known = if matches!(snapshot, MeasureSnapshot::Image(_))
                && style.contain.intersects(taffy::Contain::INLINE_SIZE)
            {
                Size::NONE
            } else {
                known
            };
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
    if let Some(snapshot) = snapshot
        && !style.contain.suppresses_baseline()
    {
        let padding = style.padding.map(resolve);
        let border = style.border.map(resolve);
        let width = (output.size.width - padding.left - padding.right - border.left - border.right)
            .max(0.0);
        let baselines = snapshot.baselines(width);
        let inset = padding.top + border.top;
        output.baselines = taffy::Baselines {
            first: baselines.first.map(|v| v + inset),
            last: baselines.last.map(|v| v + inset),
        };
    }
    output
}

fn content_space(space: AvailableSpace) -> layout::AvailableSpace {
    match space {
        AvailableSpace::Definite(v) => layout::AvailableSpace::Definite(v),
        AvailableSpace::MinContent => layout::AvailableSpace::MinContent,
        AvailableSpace::MaxContent => layout::AvailableSpace::MaxContent,
        AvailableSpace::Unbounded => layout::AvailableSpace::Unbounded,
    }
}

fn layout_content(
    mut inputs: taffy::LayoutInput,
    snapshot: &ContentMeasureSnapshot,
    style: &Style,
    stats: &mut LayoutStats,
    error: &mut Option<Error>,
) -> taffy::LayoutOutput {
    let mut last_metrics = None;
    let mut query = |available: Size<AvailableSpace>| {
        if error.is_some() {
            return None;
        }
        if [available.width, available.height]
            .iter()
            .any(|space| matches!(space, AvailableSpace::Definite(v) if !v.is_finite()))
        {
            *error = Some(Error::InvalidMeasure);
            return None;
        }
        stats.measure_calls += 1;
        let available = available.map(|space| match space {
            AvailableSpace::Definite(v) => AvailableSpace::Definite(v.max(0.0)),
            other => other,
        });
        match snapshot
            .0
            .measure(ContentMeasureInput {
                available: LayoutSize::new(
                    content_space(available.width),
                    content_space(available.height),
                ),
            })
            .and_then(ContentMeasureOutput::validate)
        {
            Ok(output) => Some(output),
            Err(_) => {
                *error = Some(Error::InvalidMeasure);
                None
            }
        }
    };
    let mut output = {
        let mut measure = |mut input: taffy::LayoutInput| {
            // Evaluate only the leaf's pure arithmetic. This publishes no tree
            // geometry. Avoid its known-size early return so the same final
            // query provides natural overflow and baseline metrics.
            input.run_mode = taffy::RunMode::PerformLayout;
            taffy::compute_leaf_layout(
                input,
                style,
                |_, _| 0.0,
                |_, available| {
                    last_metrics = query(available);
                    last_metrics.map_or(Size::ZERO, |v| Size {
                        width: v.size[0],
                        height: v.size[1],
                    })
                },
            )
        };
        let mut output = measure(inputs);
        if inputs.known_dimensions.width.is_none() {
            // Select inline size once, then measure dependent height at that
            // width. Block space stays the parent's soft opportunity; an auto
            // content-derived height must not turn into a new hard input.
            inputs.known_dimensions.width = Some(output.size.width);
            output = measure(inputs);
        }
        output
    };
    let Some(metrics) = last_metrics else {
        return taffy::LayoutOutput::HIDDEN;
    };
    if !style.contain.suppresses_baseline() {
        let resolve = |value: taffy::LengthPercentage| {
            value.resolve_or_zero(inputs.parent_size.width, |_, _| 0.0)
        };
        let padding = style.padding.map(resolve);
        let border = style.border.map(resolve);
        let inset = padding.top + border.top;
        output.baselines = taffy::Baselines {
            first: metrics.baseline.map(|v| v + inset),
            last: metrics.last_baseline.map(|v| v + inset),
        };
    }
    if [
        output.size.width,
        output.size.height,
        output.scrollable_overflow_rect.right,
        output.scrollable_overflow_rect.bottom,
    ]
    .iter()
    .any(|v| !v.is_finite() || *v < 0.0)
        || [output.baselines.first, output.baselines.last]
            .iter()
            .flatten()
            .any(|v| !v.is_finite())
    {
        *error = Some(Error::InvalidMeasure);
        return taffy::LayoutOutput::HIDDEN;
    }
    output
}

pub(super) fn effective_style(style: &Style, snapshot: Option<&MeasureSnapshot>) -> Style {
    let mut effective = style.clone();
    if matches!(snapshot, Some(MeasureSnapshot::Image(_))) {
        effective.item_is_replaced = true;
    }
    if effective.aspect_ratio.is_none()
        && !effective.contain.intersects(taffy::Contain::INLINE_SIZE)
        && let Some(MeasureSnapshot::Image(image)) = snapshot
        && image.width > 0.0
        && image.height > 0.0
    {
        effective.aspect_ratio = Some(image.width / image.height);
    }
    effective
}

#[cfg(test)]
mod content_tests;
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
