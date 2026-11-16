//! Custom parents use the same host box model and real cached child algorithms.
use crate::tree::{CustomLayout, CustomLayoutChildren, CustomLayoutInput, CustomLayoutOutput};
use crate::util::{MaybeMath, MaybeResolve, ResolveOrZero};
use crate::{
    Baselines, BoxSizing, CoreStyle, Layout, LayoutInput, LayoutOutput, LayoutPartialTree, Line, NodeId, Overflow,
    Point, Rect, RunMode, Size, SizingMode, TaffyError,
};

struct Children<'a, T> {
    tree: &'a mut T,
    parent: NodeId,
}
impl<T: LayoutPartialTree> CustomLayoutChildren for Children<'_, T> {
    fn len(&self) -> usize {
        self.tree.child_count(self.parent)
    }
    fn measure(&mut self, child: usize, mut input: LayoutInput) -> Result<LayoutOutput, TaffyError> {
        if child >= self.len() || !valid_input(&input) {
            return Err(TaffyError::InvalidCustomLayout);
        }
        input.run_mode = RunMode::ComputeSize;
        input.vertical_margins_are_collapsible = Line::FALSE;
        let id = self.tree.get_child_id(self.parent, child);
        Ok(self.tree.compute_custom_child_layout(id, input))
    }
}

fn valid_input(input: &LayoutInput) -> bool {
    input.size_bounds.is_valid()
        && [
            input.known_dimensions.width,
            input.known_dimensions.height,
            input.parent_size.width,
            input.parent_size.height,
        ]
        .iter()
        .flatten()
        .all(|v| v.is_finite() && *v >= 0.0)
        && [input.available_space.width, input.available_space.height]
            .iter()
            .all(|space| !matches!(space, crate::AvailableSpace::Definite(v) if !v.is_finite() || *v < 0.0))
}

fn plan(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    delegate: &dyn CustomLayout,
    input: CustomLayoutInput,
) -> Result<CustomLayoutOutput, TaffyError> {
    let count = tree.child_count(node);
    let output = delegate.layout(input, &mut Children { tree, parent: node })?;
    if ![output.size.width, output.size.height].iter().all(|v| v.is_finite() && *v >= 0.0)
        || input.known_size.width.is_some_and(|v| v != output.size.width)
        || input.known_size.height.is_some_and(|v| v != output.size.height)
        || [output.baselines.first, output.baselines.last]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0 || *v > output.size.height)
        || output.placements.len() != count
    {
        return Err(TaffyError::InvalidCustomLayout);
    }
    let mut seen = crate::util::sys::Vec::with_capacity(count);
    seen.resize(count, false);
    for placement in &output.placements {
        if placement.child >= count
            || core::mem::replace(&mut seen[placement.child], true)
            || !placement.origin.x.is_finite()
            || !placement.origin.y.is_finite()
            || !valid_input(&placement.input)
        {
            return Err(TaffyError::InvalidCustomLayout);
        }
    }
    Ok(output)
}

#[cfg(all(test, feature = "flexbox"))]
mod tests {
    use super::*;
    use crate::tree::{CustomPlacement, SizeBounds};
    use crate::style_helpers::TaffyMaxContent;
    use crate::{Dimension, Display, Style, TaffyTree};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Debug)]
    struct Single(Arc<AtomicUsize>);
    impl CustomLayout for Single {
        fn layout(
            &self,
            input: CustomLayoutInput,
            children: &mut dyn CustomLayoutChildren,
        ) -> Result<CustomLayoutOutput, TaffyError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            let mut slot = LayoutInput::HIDDEN;
            slot.run_mode = RunMode::ComputeSize;
            slot.available_space = input.available;
            slot.parent_size = input.percentage_basis;
            slot.size_bounds = SizeBounds::NONE;
            let child = children.measure(0, slot)?;
            Ok(CustomLayoutOutput {
                size: input.known_size.unwrap_or(child.size),
                baselines: Baselines::NONE,
                placements: vec![CustomPlacement { child: 0, input: slot, origin: Point { x: 0.0, y: 0.0 } }],
            })
        }
    }

    #[test]
    fn final_cache_hidden_remove_and_clear_own_delegate_lifetimes() {
        let mut tree = TaffyTree::<()>::new();
        tree.disable_rounding();
        let child = tree
            .new_leaf(Style {
                size: Size { width: Dimension::length(20.0), height: Dimension::length(10.0) },
                ..Style::DEFAULT
            })
            .unwrap();
        let root = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let delegate: Arc<dyn CustomLayout> = Arc::new(Single(calls.clone()));
        tree.set_custom_layout(root, Some(delegate.clone())).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(root).unwrap().size, Size { width: 20.0, height: 10.0 });
        let before = calls.load(Ordering::Relaxed);
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), before);
        tree.set_style(root, Style { display: Display::None, ..Style::DEFAULT }).unwrap();
        tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
        assert_eq!(tree.layout(child).unwrap().size, Size::ZERO);
        assert_eq!(calls.load(Ordering::Relaxed), before);
        tree.remove(root).unwrap();
        assert_eq!(Arc::strong_count(&delegate), 1);
        tree.set_custom_layout(child, Some(delegate.clone())).unwrap();
        tree.clear();
        assert_eq!(Arc::strong_count(&delegate), 1);
    }

    #[test]
    fn direct_oof_child_rejects_before_observer_writes() {
        let mut tree = TaffyTree::<()>::new();
        let child = tree.new_leaf(Style { position: crate::Position::Absolute, ..Style::DEFAULT }).unwrap();
        let root = tree.new_with_children(Style::DEFAULT, &[child]).unwrap();
        tree.set_custom_layout(root, Some(Arc::new(Single(Arc::new(AtomicUsize::new(0)))))).unwrap();
        let mut writes = 0;
        let result = tree.compute_layout_with_measure_and_observer(
            root,
            Size::MAX_CONTENT,
            |inputs, _, _, style| crate::compute_leaf_layout(inputs, style, |_, _| 0.0, |_, _| Size::ZERO),
            |_, _, _| writes += 1,
        );
        assert_eq!(result, Err(TaffyError::InvalidCustomLayout));
        assert_eq!(writes, 0);
        assert!(tree.set_custom_layout(crate::NodeId::from(u64::MAX), None).is_err());
    }
}

/// Apply an owned algorithm inside the host's standard sizing/inset model.
/// An error can follow internal child writes; callers needing atomic mutation
/// must run this on a private candidate. Failed outputs are never cached.
pub fn compute_custom_layout(
    tree: &mut impl LayoutPartialTree,
    node: NodeId,
    mut inputs: LayoutInput,
    delegate: &dyn CustomLayout,
) -> Result<LayoutOutput, TaffyError> {
    let style = tree.get_core_container_style(node);
    let padding = style.padding().resolve_or_zero(inputs.parent_size.width, |v, b| tree.resolve_calc_value(v, b));
    let border = style.border().resolve_or_zero(inputs.parent_size.width, |v, b| tree.resolve_calc_value(v, b));
    let overflow = style.overflow();
    let rtl = style.direction().is_rtl();
    let gutter = Size {
        width: if overflow.y == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
        height: if overflow.x == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
    };
    let mut inset = padding + border;
    if rtl {
        inset.left += gutter.width;
    } else {
        inset.right += gutter.width;
    }
    inset.bottom += gutter.height;
    let inset_size = inset.sum_axes();
    let size_style = style.size();
    let min_style = style.min_size();
    let max_style = style.max_size();
    let ratio = style.aspect_ratio();
    let adjustment =
        if style.box_sizing() == BoxSizing::ContentBox { (padding + border).sum_axes() } else { Size::ZERO };
    let numeric_min = min_style
        .maybe_resolve(inputs.parent_size, |v, b| tree.resolve_calc_value(v, b))
        .maybe_apply_aspect_ratio(ratio)
        .maybe_add(adjustment);
    let numeric_max = max_style
        .maybe_resolve(inputs.parent_size, |v, b| tree.resolve_calc_value(v, b))
        .maybe_apply_aspect_ratio(ratio)
        .maybe_add(adjustment);
    let preferred = size_style
        .maybe_resolve(inputs.parent_size, |v, b| tree.resolve_calc_value(v, b))
        .maybe_apply_aspect_ratio(ratio)
        .maybe_add(adjustment);
    let contain = style.contain();
    let scrollbar_width = style.scrollbar_width();
    let direction = style.direction();
    drop(style);
    let (intrinsic_min, intrinsic_max) =
        super::common::intrinsic_limits::resolve_intrinsic_limits(tree, node, min_style, max_style, inputs);
    let (min, max) = if inputs.sizing_mode == SizingMode::InherentSize {
        inputs.size_bounds.limits(intrinsic_min.or(numeric_min), intrinsic_max.or(numeric_max))
    } else {
        inputs.size_bounds.limits(Size::NONE, Size::NONE)
    };
    inputs.available_space = crate::SizeBounds {
        min: min.map(|v| v.unwrap_or(0.0)),
        max: max.zip_map(min, |v, min| v.unwrap_or(f32::INFINITY).max(min.unwrap_or(0.0))),
    }
    .available(inputs.available_space);
    inputs.known_dimensions = inputs
        .size_bounds
        .min
        .zip_map(inputs.size_bounds.max, |min, max| (min == max).then_some(min))
        .or(inputs.known_dimensions)
        .or(if inputs.sizing_mode == SizingMode::InherentSize { preferred } else { Size::NONE })
        .maybe_clamp(min, max);
    inputs.known_dimensions_are_definite = inputs
        .known_dimensions_are_definite
        .zip_map(inputs.size_bounds.min.zip_map(inputs.size_bounds.max, |min, max| min == max), |definite, tight| {
            definite || tight
        });
    let dimensions =
        |size: Size<Option<f32>>| size.map(|v| v.map(crate::Dimension::length).unwrap_or_else(crate::Dimension::auto));
    let lengths = |r: Rect<f32>| Rect {
        left: crate::LengthPercentage::length(r.left),
        right: crate::LengthPercentage::length(r.right),
        top: crate::LengthPercentage::length(r.top),
        bottom: crate::LengthPercentage::length(r.bottom),
    };
    let common = crate::Style::<crate::sys::DefaultCheapStr> {
        size: Size { width: crate::Dimension::auto(), height: crate::Dimension::auto() },
        min_size: dimensions(min),
        max_size: dimensions(max),
        aspect_ratio: ratio,
        padding: lengths(padding),
        border: lengths(border),
        overflow,
        scrollbar_width,
        contain,
        box_sizing: BoxSizing::BorderBox,
        direction,
        ..crate::Style::DEFAULT
    };
    let measure = |tree: &mut _, inputs: LayoutInput| -> Result<LayoutOutput, TaffyError> {
        let mut error = None;
        let output = super::compute_leaf_layout(
            inputs,
            &common,
            |_, _| 0.0,
            |_, available| {
                let known_size = inputs.known_dimensions.maybe_sub(inset_size).maybe_max(Size::ZERO);
                if error.is_some() {
                    return Size::ZERO;
                }
                let percentage_basis = inputs
                    .known_dimensions
                    .zip_map(inputs.known_dimensions_are_definite, |v, definite| if definite { v } else { None })
                    .maybe_sub(inset_size)
                    .maybe_max(Size::ZERO);
                match plan(tree, node, delegate, CustomLayoutInput { known_size, available, percentage_basis }) {
                    Ok(plan) => plan.size,
                    Err(value) => {
                        error = Some(value);
                        Size::ZERO
                    }
                }
            },
        );
        error.map_or(Ok(output), Err)
    };
    let mut output = measure(tree, inputs)?;
    if inputs.known_dimensions.width.is_none() {
        // Select inline size before dependent height. No fixed-point iteration.
        inputs.known_dimensions.width = Some(output.size.width);
        inputs.known_dimensions_are_definite.width = true;
        output = measure(tree, inputs)?;
    }
    let inner = (output.size - inset_size).map(|v| v.max(0.0));
    let percentage_basis = Size {
        width: Some(inner.width),
        height: inputs
            .known_dimensions
            .height
            .filter(|_| inputs.known_dimensions_are_definite.height)
            .map(|h| (h - inset_size.height).max(0.0)),
    };
    let final_plan = plan(
        tree,
        node,
        delegate,
        CustomLayoutInput {
            known_size: inner.map(Some),
            available: inner.map(crate::AvailableSpace::Definite),
            percentage_basis,
        },
    )?;
    output.baselines = if common.contain.suppresses_baseline() {
        Baselines::NONE
    } else {
        Baselines {
            first: final_plan.baselines.first.map(|v| v + inset.top),
            last: final_plan.baselines.last.map(|v| v + inset.top),
        }
    };
    if inputs.run_mode != RunMode::PerformLayout {
        return Ok(output);
    }
    for placement in final_plan.placements {
        let child = tree.get_child_id(node, placement.child);
        let mut input = placement.input;
        input.run_mode = RunMode::PerformLayout;
        input.vertical_margins_are_collapsible = Line::FALSE;
        let mut child_output = super::constraint_box::layout_child(tree, child, input);
        let location = Point { x: inset.left + placement.origin.x, y: inset.top + placement.origin.y };
        if ![
            location.x,
            location.y,
            child_output.size.width,
            child_output.size.height,
            location.x + child_output.size.width,
            location.y + child_output.size.height,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return Err(TaffyError::InvalidCustomLayout);
        }
        let style = tree.get_core_container_style(child);
        let hidden = style.box_generation_mode() == crate::BoxGenerationMode::None;
        let child_overflow = style.overflow();
        let child_contain = style.contain();
        let child_layout = if hidden {
            Layout::with_order(placement.child as u32)
        } else {
            Layout {
                order: placement.child as u32,
                location,
                size: child_output.size,
                baselines: child_output.baselines,
                padding: style.padding().resolve_or_zero(input.parent_size.width, |v, b| tree.resolve_calc_value(v, b)),
                border: style.border().resolve_or_zero(input.parent_size.width, |v, b| tree.resolve_calc_value(v, b)),
                margin: Rect::ZERO,
                scrollbar_size: Size {
                    width: if child_overflow.y == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
                    height: if child_overflow.x == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
                },
                #[cfg(feature = "content_size")]
                scrollable_overflow_rect: child_output.scrollable_overflow_rect,
            }
        };
        drop(style);
        tree.set_unrounded_layout(child, &child_layout);
        #[cfg(feature = "content_size")]
        if !hidden {
            let contribution = super::common::scrollable_overflow::compute_scrollable_overflow_contribution(
                Point {
                    x: if rtl {
                        output.size.width - location.x - child_output.size.width - border.right
                    } else {
                        location.x - border.left
                    },
                    y: location.y - border.top,
                },
                child_output.size,
                child_output.scrollable_overflow_rect,
                child_overflow,
                child_contain,
                overflow.x.is_scroll_container() || overflow.y.is_scroll_container(),
            );
            output.scrollable_overflow_rect = output.scrollable_overflow_rect.union(contribution);
        }
        #[cfg(not(feature = "content_size"))]
        let _ = child_contain;
        child_output.oof_candidates.translate(location);
        output.oof_candidates.append(&mut child_output.oof_candidates);
    }
    output.oof_positioning_area = Some(crate::OofPositioningArea {
        size: (output.size - border.sum_axes() - gutter).map(|v| v.max(0.0)),
        offset: Point { x: border.left + if rtl { gutter.width } else { 0.0 }, y: border.top },
    });
    Ok(output)
}
