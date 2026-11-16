//! Content-derived min/max sizes share the normal size-query path.
use super::sizing_keyword::{resolve_sizing_keyword, SizingKeywordResolution};
use crate::geometry::{AbsoluteAxis, Line, Size};
use crate::tree::{LayoutInput, LayoutPartialTree, LayoutPartialTreeExt, NodeId, SizingMode};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{BoxSizing, CoreStyle, Dimension};

pub(crate) fn set_axis<T>(size: &mut Size<T>, axis: AbsoluteAxis, value: T) {
    match axis {
        AbsoluteAxis::Horizontal => size.width = value,
        AbsoluteAxis::Vertical => size.height = value,
    }
}

/// Resolve only sizing-keyword limits. Numeric constraints remain the caller's
/// box-model calculation. Measurements return border-box dimensions, so callers
/// must not add content-box padding a second time. ContentSize ignores the
/// queried node's own preferred/min/max sizes, avoiding self-recursion; child
/// constraints continue to participate normally.
pub(crate) fn resolve_intrinsic_limits(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    min: Size<Dimension>,
    max: Size<Dimension>,
    inputs: LayoutInput,
) -> (Size<Option<f32>>, Size<Option<f32>>) {
    if inputs.sizing_mode == SizingMode::ContentSize {
        return (Size::NONE, Size::NONE);
    }
    if ![min.width, min.height, max.width, max.height].into_iter().any(Dimension::is_sizing_keyword) {
        return (Size::NONE, Size::NONE);
    }
    let style = tree.get_core_container_style(node);
    let pb = (style.padding().resolve_or_zero(inputs.parent_size.width, |v, b| tree.calc(v, b))
        + style.border().resolve_or_zero(inputs.parent_size.width, |v, b| tree.calc(v, b)))
    .sum_axes();
    let adjustment = if style.box_sizing() == BoxSizing::ContentBox { pb } else { Size::ZERO };
    let aspect_ratio = style.aspect_ratio();
    let numeric_min = min.maybe_resolve(inputs.parent_size, |v, b| tree.calc(v, b)).maybe_add(adjustment);
    let numeric_max = max.maybe_resolve(inputs.parent_size, |v, b| tree.calc(v, b)).maybe_add(adjustment);
    let mut known = inputs
        .known_dimensions
        .or(style
            .size()
            .maybe_resolve(inputs.parent_size, |v, b| tree.calc(v, b))
            .maybe_apply_aspect_ratio(style.aspect_ratio())
            .maybe_add(adjustment))
        .maybe_clamp(numeric_min, numeric_max);
    drop(style);
    let mut resolved = [Size::NONE, Size::NONE];
    for axis in [AbsoluteAxis::Horizontal, AbsoluteAxis::Vertical] {
        for (index, styles) in [min, max].into_iter().enumerate() {
            let style = styles.get_abs(axis);
            if !style.is_sizing_keyword() {
                continue;
            }
            let value = match resolve_sizing_keyword(
                style,
                inputs.available_space.get_abs(axis).into_option(),
                inputs.parent_size.get_abs(axis),
                |value, basis| tree.calc(value, basis),
            ) {
                Some(SizingKeywordResolution::Exact(value)) => Some(value),
                Some(SizingKeywordResolution::Measure(space)) => {
                    let mut query_known = known;
                    set_axis(&mut query_known, axis, None);
                    let mut available = inputs.available_space;
                    set_axis(&mut available, axis, space);
                    Some(tree.measure_child_size(
                        node,
                        query_known,
                        inputs.parent_size,
                        available,
                        SizingMode::ContentSize,
                        axis,
                        Line::FALSE,
                    ))
                }
                None => None,
            };
            set_axis(&mut resolved[index], axis, value);
        }
        // Width-dependent block-axis intrinsic limits use the constrained used
        // inline size when one is known, including newly resolved width limits.
        known = known.maybe_clamp(resolved[0].or(numeric_min), resolved[1].or(numeric_max));
    }
    // Match numeric min/max's existing ratio transfer. A newly resolved width
    // limit must constrain the auto height before an earlier preferred-width
    // ratio suggestion is treated as the final height by the parent.
    let transfer = |resolved: Size<Option<f32>>, styles: Size<Dimension>| {
        resolved
            .maybe_apply_aspect_ratio(aspect_ratio)
            .zip_map(styles, |value, style| (style.is_auto() || style.is_sizing_keyword()).then_some(value).flatten())
    };
    (transfer(resolved[0], min), transfer(resolved[1], max))
}
