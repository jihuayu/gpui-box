//! Low-level access to the layout algorithms themselves. For a higher-level API, see the [`TaffyTree`](crate::TaffyTree) struct.
//!
//! ### Layout functions
//!
//! The layout functions all take an [`&mut impl LayoutPartialTree`](crate::LayoutPartialTree) parameter, which represents a single container node and it's direct children.
//!
//! | Function                          | Purpose                                                                                                                                                                                            |
//! | ---                               | ---                                                                                                                                                                                                |
//! | [`compute_flexbox_layout`]        | Layout a Flexbox container and it's direct children                                                                                                                                                |
//! | [`compute_grid_layout`]           | Layout a CSS Grid container and it's direct children                                                                                                                                               |
//! | [`compute_block_layout`]          | Layout a Block container and it's direct children                                                                                                                                                  |
//! | [`compute_leaf_layout`]           | Applies common properties like padding/border/aspect-ratio to a node before deferring to a passed closure to determine it's size. Can be applied to nodes like text or image nodes.                |
//! | [`compute_root_layout`]           | Layout the root node of a tree (regardless of it's layout mode). This function is typically called once to begin a layout run.                                                                     |                                                                      |
//! | [`compute_hidden_layout`]         | Mark a node as hidden during layout (like `Display::None`)                                                                                                                                         |
//! | [`compute_cached_layout`]         | Attempts to find a cached layout for the specified node and layout inputs. Uses the provided closure to compute the layout (and then stores the result in the cache) if no cached layout is found. |
//!
//! ### Other functions
//!
//! | Function                          | Requires                                                                                                                                                                                           | Purpose                                                              |
//! | ---                               | ---                                                                                                                                                                                                | ---                                                                  |
//! | [`round_layout`]                  | [`RoundTree`]                                                                                                                                                                                      | Round a tree of float-valued layouts to integer pixels               |
//! | [`print_tree`](crate::print_tree) | [`PrintTree`](crate::PrintTree)                                                                                                                                                                    | Print a debug representation of a node tree and it's computed layout |
//!
pub(crate) mod common;
pub(crate) mod constraint_box;
#[cfg(feature = "taffy_tree")]
mod custom_layout;
#[cfg(feature = "taffy_tree")]
pub use custom_layout::compute_custom_layout;
pub(crate) mod leaf;
pub use constraint_box::compute_constraint_box_layout;
pub(crate) mod oof;
#[cfg(all(test, feature = "taffy_tree", feature = "flexbox", feature = "grid", feature = "block_layout"))]
mod unbounded_tests;

#[cfg(feature = "block_layout")]
pub(crate) mod block;

#[cfg(feature = "float_layout")]
pub(crate) mod float;

#[cfg(feature = "flexbox")]
pub(crate) mod flexbox;

#[cfg(feature = "grid")]
pub(crate) mod grid;

pub use leaf::compute_leaf_layout;
pub use oof::{compute_oof_layout, compute_oof_layout_for_area, resolve_static_offset, OofLayoutResult};

#[cfg(feature = "block_layout")]
pub use self::block::{compute_block_align_content_offset, compute_block_layout, BlockContext, BlockFormattingContext};

#[cfg(feature = "flexbox")]
pub use self::flexbox::compute_flexbox_layout;

#[cfg(feature = "grid")]
pub use self::grid::compute_grid_layout;

#[cfg(feature = "float_layout")]
pub use self::float::{BfcSlot, ContentSlot, FloatContext, FloatIntrinsicWidthCalculator};

use crate::geometry::{Line, Point, Size};
use crate::style::{AvailableSpace, ContainingBlockClaims, CoreStyle, Overflow, Position};
use crate::tree::{
    AxisStaticAlign, AxisStaticEdge, AxisStaticPosition, Layout, LayoutInput, LayoutOutput, LayoutPartialTree,
    LayoutPartialTreeExt, NodeId, OofCandidate, OofCandidates, RequestedAxis, RoundTree, RunMode, SizingMode,
};
use crate::util::debug::{debug_log, debug_log_node, debug_pop_node, debug_push_node};
use crate::util::sys::{round, Vec};
use crate::util::ResolveOrZero;
use crate::{CacheTree, MaybeMath, MaybeResolve};

/// Compute layout for the root node in the tree
///
/// The root's containing block is the initial containing block (ICB), which has the dimensions of the
/// viewport (`available_space`) and is anchored at the canvas origin. The root's `position` and `inset`
/// styles are resolved against it:
///
/// - A `position: absolute`/`fixed` root is laid out by the out-of-flow positioning routine used for
///   every other out-of-flow box (insets, shrink-to-fit sizing, auto margins, ...), with its static
///   position at the ICB origin. This requires a definite `available_space` in both axes: otherwise there
///   is no ICB to resolve against and the root is laid out as if it were in flow.
/// - A `position: relative` root is laid out in flow and then offset by its insets.
pub fn compute_root_layout(
    tree: &mut (impl crate::tree::LayoutContainingBlock + CacheTree),
    root: NodeId,
    available_space: Size<AvailableSpace>,
) {
    compute_root_layout_with_bounds(tree, root, available_space, crate::tree::SizeBounds::NONE)
}

/// Layout a root with independent hard border-box bounds. The caller must
/// provide nonnegative ordered bounds with finite minima and finite or +inf
/// maxima. Hidden nodes remain zero-sized regardless of bounds.
pub fn compute_root_layout_with_bounds(
    tree: &mut (impl crate::tree::LayoutContainingBlock + CacheTree),
    root: NodeId,
    available_space: Size<AvailableSpace>,
    bounds: crate::tree::SizeBounds,
) {
    let style = tree.get_core_container_style(root);
    let position = style.position();
    let direction = style.direction();
    let hidden = style.box_generation_mode() == crate::BoxGenerationMode::None;
    drop(style);
    if hidden {
        compute_hidden_layout(tree, root);
        return;
    }

    let icb_size = available_space.into_options();
    let oof_root_area = match (position.is_out_of_flow(), icb_size.width, icb_size.height) {
        (true, Some(width), Some(height)) => Some(Size { width, height }),
        _ => None,
    };

    // When the root's layout is served from the cache its layout algorithm does not run, so the
    // hoisted children it recorded on a previous run (including those added by the root
    // positioning pass below) are still in place and must not be re-added.
    let mut root_is_cached = false;

    let (layout, candidates) = if let Some(area_size) = oof_root_area {
        // The static position of the root places its margin box at the origin of the ICB
        let (start, end) = if direction.is_rtl() { (area_size.width, area_size.width) } else { (0.0, 0.0) };
        let candidate = OofCandidate {
            node: root,
            order: 0,
            position,
            static_position: Point {
                x: AxisStaticPosition {
                    area: Line { start, end },
                    align: AxisStaticAlign::from_keyword(if direction.is_rtl() {
                        AxisStaticEdge::End
                    } else {
                        AxisStaticEdge::Start
                    }),
                },
                y: AxisStaticPosition::from_edge(0.0, AxisStaticEdge::Start),
            },
        };
        let oof::OofBoxLayout { layout, surfaced, .. } = oof::layout_oof_box(
            tree,
            candidate,
            area_size,
            Point::ZERO,
            direction,
            // The initial containing block is not a scroll container
            Point { x: Overflow::Visible, y: Overflow::Visible },
            #[cfg(feature = "grid")]
            None,
            bounds,
            |tree, inputs| root_is_cached = tree.cache_get(root, inputs).is_some(),
        );
        (layout, surfaced)
    } else {
        compute_in_flow_root_layout(tree, root, available_space, bounds, &mut root_is_cached)
    };

    // Final positioning pass for out-of-flow boxes with no nearer containing block: the initial
    // containing block is the containing block for `position: fixed` boxes and for
    // `position: absolute` boxes with no positioned ancestor.
    if !candidates.is_empty() {
        let overflow = tree.get_core_container_style(root).overflow();

        // The initial containing block has the dimensions of the viewport (the available space) and is
        // anchored at the canvas origin. In an axis where the available space is indefinite, fall back
        // to the root's padding box.
        let area_inset = layout.border
            + crate::geometry::Rect {
                left: 0.0,
                right: layout.scrollbar_size.width,
                top: 0.0,
                bottom: layout.scrollbar_size.height,
            };
        let root_padding_box_size = (layout.size
            - Size { width: area_inset.horizontal_axis_sum(), height: area_inset.vertical_axis_sum() })
        .map(|value| value.max(0.0));
        let (area_width, area_x) = match icb_size.width {
            Some(width) => ((width - layout.scrollbar_size.width).max(0.0), -layout.location.x),
            None => (root_padding_box_size.width, area_inset.left),
        };
        let (area_height, area_y) = match icb_size.height {
            Some(height) => ((height - layout.scrollbar_size.height).max(0.0), -layout.location.y),
            None => (root_padding_box_size.height, area_inset.top),
        };
        let area_size = Size { width: area_width, height: area_height };
        let area_offset = Point { x: area_x, y: area_y };

        let mut hoisted: Vec<NodeId> = Vec::new();
        let mut unclaimed = OofCandidates::new();
        oof::perform_oof_layout(
            tree,
            root,
            candidates.as_slice(),
            area_size,
            area_offset,
            direction,
            // The root is the initial containing block and claims all remaining candidates
            ContainingBlockClaims::ALL,
            overflow,
            &mut hoisted,
            &mut unclaimed,
        );
        debug_assert!(unclaimed.is_empty(), "the root positioning pass must claim all remaining candidates");
        if !root_is_cached {
            tree.add_hoisted_children(root, &hoisted);
        }
    }
}

/// Lay out an in-flow (or `position: relative`) root node against the available space, store its layout
/// and return it along with the out-of-flow candidates bubbled up from its subtree.
#[inline(always)]
fn compute_in_flow_root_layout(
    tree: &mut (impl crate::tree::LayoutContainingBlock + CacheTree),
    root: NodeId,
    available_space: Size<AvailableSpace>,
    bounds: crate::tree::SizeBounds,
    root_is_cached: &mut bool,
) -> (Layout, OofCandidates) {
    let mut known_dimensions = Size::NONE;

    // The available space passed to the root excludes the root's margins
    let root_available_space = {
        let style = tree.get_core_container_style(root);
        let margin =
            style.margin().resolve_or_zero(available_space.width.into_option(), |val, basis| tree.calc(val, basis));
        bounds.available(available_space.maybe_sub(margin.sum_axes()).maybe_max(Size::ZERO))
    };

    #[cfg(feature = "block_layout")]
    {
        use crate::BoxSizing;

        let parent_size = available_space.into_options();
        let style = tree.get_core_container_style(root);

        if style.is_block() {
            // Pull these out earlier to avoid borrowing issues
            let aspect_ratio = style.aspect_ratio();
            let padding = style.padding().resolve_or_zero(parent_size.width, |val, basis| tree.calc(val, basis));
            let border = style.border().resolve_or_zero(parent_size.width, |val, basis| tree.calc(val, basis));
            let padding_border_size = (padding + border).sum_axes();
            let box_sizing_adjustment =
                if style.box_sizing() == BoxSizing::ContentBox { padding_border_size } else { Size::ZERO };

            let min_size = style
                .min_size()
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_apply_aspect_ratio(aspect_ratio)
                .maybe_add(box_sizing_adjustment);
            let max_size = style
                .max_size()
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_apply_aspect_ratio(aspect_ratio)
                .maybe_add(box_sizing_adjustment);
            let clamped_style_size = style
                .size()
                .maybe_resolve(parent_size, |val, basis| tree.calc(val, basis))
                .maybe_apply_aspect_ratio(aspect_ratio)
                .maybe_add(box_sizing_adjustment)
                .maybe_clamp(min_size, max_size);

            // If both min and max in a given axis are set and max <= min then this determines the size in that axis
            let min_max_definite_size = min_size.zip_map(max_size, |min, max| match (min, max) {
                (Some(min), Some(max)) if max <= min => Some(min),
                _ => None,
            });

            // Block nodes automatically stretch fit their width to fit available space if available space is definite
            let available_space_based_size = Size {
                width: if style.size().width.is_sizing_keyword() {
                    None
                } else {
                    root_available_space.width.into_option()
                },
                height: None,
            };

            let styled_based_known_dimensions = known_dimensions
                .or(min_max_definite_size)
                .or(clamped_style_size)
                .or(available_space_based_size)
                .maybe_max(padding_border_size);

            known_dimensions = styled_based_known_dimensions;
        }
    }

    let tight_dimensions = bounds.min.zip_map(bounds.max, |min, max| (min == max).then_some(min));
    let mut inputs = LayoutInput {
        size_bounds: bounds,
        known_dimensions: tight_dimensions.or(known_dimensions),
        known_dimensions_are_definite: Size { width: true, height: true },
        parent_size: available_space.into_options(),
        available_space: root_available_space,
        sizing_mode: SizingMode::InherentSize,
        axis: RequestedAxis::Both,
        run_mode: RunMode::PerformLayout,
        vertical_margins_are_collapsible: Line::FALSE,
    };
    let style = tree.get_core_container_style(root);
    let min_style = style.min_size();
    let max_style = style.max_size();
    let pb = (style.padding().resolve_or_zero(inputs.parent_size.width, |v, b| tree.calc(v, b))
        + style.border().resolve_or_zero(inputs.parent_size.width, |v, b| tree.calc(v, b)))
    .sum_axes();
    let adjustment = if style.box_sizing() == crate::BoxSizing::ContentBox { pb } else { Size::ZERO };
    let numeric_min = min_style
        .maybe_resolve(inputs.parent_size, |v, b| tree.calc(v, b))
        .maybe_apply_aspect_ratio(style.aspect_ratio())
        .maybe_add(adjustment);
    let numeric_max = max_style
        .maybe_resolve(inputs.parent_size, |v, b| tree.calc(v, b))
        .maybe_apply_aspect_ratio(style.aspect_ratio())
        .maybe_add(adjustment);
    drop(style);
    let (intrinsic_min, intrinsic_max) =
        common::intrinsic_limits::resolve_intrinsic_limits(tree, root, min_style, max_style, inputs);
    let (min_size, max_size) = bounds.limits(intrinsic_min.or(numeric_min), intrinsic_max.or(numeric_max));
    inputs.known_dimensions = inputs.known_dimensions.maybe_clamp(min_size, max_size);
    // A root has no parent item algorithm to resolve its sizing keywords.
    // Measure the requested intrinsic axes using the same resolver as Flex,
    // Grid and out-of-flow items, then run normal layout with the used size.
    // ContentSize ignores the root's preferred size during this query.
    use common::sizing_keyword::{resolve_sizing_keyword, SizingKeywordResolution};
    let size_style = tree.get_core_container_style(root).size();
    if size_style.width.is_sizing_keyword() || size_style.height.is_sizing_keyword() {
        let style = tree.get_core_container_style(root);
        let aspect_ratio = style.aspect_ratio();
        let padding_border =
            (style.padding().resolve_or_zero(inputs.parent_size.width, |val, basis| tree.calc(val, basis))
                + style.border().resolve_or_zero(inputs.parent_size.width, |val, basis| tree.calc(val, basis)))
            .sum_axes();
        let adjustment = if style.box_sizing() == crate::BoxSizing::ContentBox { padding_border } else { Size::ZERO };
        let min_size = intrinsic_min.or(style
            .min_size()
            .maybe_resolve(inputs.parent_size, |val, basis| tree.calc(val, basis))
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_add(adjustment));
        let max_size = intrinsic_max.or(style
            .max_size()
            .maybe_resolve(inputs.parent_size, |val, basis| tree.calc(val, basis))
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_add(adjustment));
        let (min_size, max_size) = bounds.limits(min_size, max_size);
        inputs.known_dimensions = inputs.known_dimensions.or(size_style
            .maybe_resolve(inputs.parent_size, |val, basis| tree.calc(val, basis))
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_add(adjustment)
            .maybe_clamp(min_size, max_size));
        drop(style);
        let mut query_space = root_available_space;
        let mut measured_axes = Size { width: false, height: false };
        for (style, stretch, basis, known, query, measure) in [
            (
                size_style.width,
                root_available_space.width,
                available_space.width,
                &mut inputs.known_dimensions.width,
                &mut query_space.width,
                &mut measured_axes.width,
            ),
            (
                size_style.height,
                root_available_space.height,
                available_space.height,
                &mut inputs.known_dimensions.height,
                &mut query_space.height,
                &mut measured_axes.height,
            ),
        ] {
            if known.is_some() {
                continue;
            }
            match resolve_sizing_keyword(style, stretch.into_option(), basis.into_option(), |val, basis| {
                tree.calc(val, basis)
            }) {
                Some(SizingKeywordResolution::Exact(size)) => {
                    *known = Some(size);
                }
                Some(SizingKeywordResolution::Measure(space)) => {
                    *query = space;
                    *measure = true;
                }
                None => {}
            }
        }
        if measured_axes.width || measured_axes.height {
            // Keep bounds on same-node keyword queries so a constrained inline
            // size determines actual wrapped height. Descendant helpers reset
            // them explicitly. ContentSize still ignores authored limits.
            let measured = tree
                .compute_child_layout(
                    root,
                    LayoutInput {
                        available_space: query_space,
                        sizing_mode: SizingMode::ContentSize,
                        run_mode: RunMode::ComputeSize,
                        ..inputs
                    },
                )
                .size;
            inputs.known_dimensions = inputs
                .known_dimensions
                .zip_map(measured.zip_map(measured_axes, |size, used| used.then_some(size)), Option::or);
            // A content-derived block-axis size constrains final line breaks
            // without becoming a definite basis for percentage heights. Inline
            // percentages resolve against the final used width.
            if measured_axes.height {
                inputs.known_dimensions_are_definite.height = false;
            }
        }
        inputs.known_dimensions = inputs
            .known_dimensions
            .maybe_apply_aspect_ratio(aspect_ratio)
            .maybe_clamp(min_size, max_size)
            .maybe_max(padding_border);
    }
    // Tight dimensions are definite parent assignments, including an
    // independently unbounded available-space axis. Loose bounds constrain the
    // normal sizing algorithm without manufacturing a percentage basis.
    inputs.known_dimensions = inputs
        .known_dimensions
        .zip_map(bounds.min.zip_map(bounds.max, |min, max| (min == max).then_some(min)), |known, tight| {
            tight.or(known)
        });
    inputs.known_dimensions =
        inputs.known_dimensions.zip_map(bounds.min.zip_map(bounds.max, |min, max| (min, max)), |known, (min, max)| {
            known.map(|value| value.clamp(min, max))
        });
    inputs.known_dimensions_are_definite = inputs
        .known_dimensions_are_definite
        .zip_map(bounds.min.zip_map(bounds.max, |min, max| min == max), |definite, tight| definite || tight);
    *root_is_cached = tree.cache_get(root, &inputs).is_some();

    // Recursively compute node layout
    let mut output = tree.compute_child_layout(root, inputs);
    let style = tree.get_core_container_style(root);
    let padding =
        style.padding().resolve_or_zero(available_space.width.into_option(), |val, basis| tree.calc(val, basis));
    let border =
        style.border().resolve_or_zero(available_space.width.into_option(), |val, basis| tree.calc(val, basis));
    let margin =
        style.margin().resolve_or_zero(available_space.width.into_option(), |val, basis| tree.calc(val, basis));
    let scrollbar_size = Size {
        width: if style.overflow().y == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
        height: if style.overflow().x == Overflow::Scroll { style.scrollbar_width() } else { 0.0 },
    };
    let is_rtl = style.direction().is_rtl();

    // The root's margin box is placed at the origin of the initial containing block
    let mut location = Point {
        x: match (is_rtl, available_space.width.into_option()) {
            (true, Some(available_width)) => available_width - output.size.width - margin.right,
            _ => margin.left,
        },
        y: margin.top,
    };

    // A relatively positioned root is offset by its insets (resolved against the initial containing block)
    if style.position() == Position::Relative {
        let icb_size = available_space.into_options();
        let inset = style.inset();
        let left = inset.left.maybe_resolve(icb_size.width, |val, basis| tree.calc(val, basis));
        let right = inset.right.maybe_resolve(icb_size.width, |val, basis| tree.calc(val, basis));
        let top = inset.top.maybe_resolve(icb_size.height, |val, basis| tree.calc(val, basis));
        let bottom = inset.bottom.maybe_resolve(icb_size.height, |val, basis| tree.calc(val, basis));
        location.x += if is_rtl {
            right.map(|right| -right).or(left).unwrap_or(0.0)
        } else {
            left.or(right.map(|right| -right)).unwrap_or(0.0)
        };
        location.y += top.or(bottom.map(|bottom| -bottom)).unwrap_or(0.0);
    }
    drop(style);

    let layout = Layout {
        order: 0,
        location,
        size: output.size,
        baselines: output.baselines,
        #[cfg(feature = "content_size")]
        scrollable_overflow_rect: output.scrollable_overflow_rect,
        scrollbar_size,
        padding,
        border,
        // TODO: support auto margins for root node?
        margin,
    };
    tree.set_unrounded_layout(root, &layout);

    (layout, output.oof_candidates.take())
}

/// Attempts to find a cached layout for the specified node and layout inputs.
///
/// Uses the provided closure to compute the layout (and then stores the result in the cache) if no cached layout is found.
#[inline(always)]
pub fn compute_cached_layout<Tree: CacheTree + ?Sized, ComputeFunction>(
    tree: &mut Tree,
    node: NodeId,
    inputs: LayoutInput,
    compute_uncached: ComputeFunction,
) -> LayoutOutput
where
    ComputeFunction: FnOnce(&mut Tree, NodeId, LayoutInput) -> LayoutOutput,
{
    debug_push_node!(node);

    // First we check if we have a cached result for the given input
    let cache_entry = tree.cache_get(node, &inputs);
    if let Some(cached_size_and_baselines) = cache_entry {
        debug_log_node!(inputs);
        debug_log!("RESULT (CACHED)", dbg:cached_size_and_baselines.size);
        debug_pop_node!();
        return cached_size_and_baselines;
    }

    debug_log_node!(inputs);

    let computed_size_and_baselines = compute_uncached(tree, node, inputs);

    // Cache result
    tree.cache_store(node, &inputs, computed_size_and_baselines.clone());

    debug_log!("RESULT", dbg:computed_size_and_baselines.size);
    debug_pop_node!();

    computed_size_and_baselines
}

/// Rounds the calculated layout to exact pixel values
///
/// In order to ensure that no gaps in the layout are introduced we:
///   - Always round based on the cumulative x/y coordinates (relative to the viewport) rather than
///     parent-relative coordinates
///   - Compute width/height by first rounding the top/bottom/left/right and then computing the difference
///     rather than rounding the width/height directly
///
/// See <https://github.com/facebook/yoga/commit/aa5b296ac78f7a22e1aeaf4891243c6bb76488e2> for more context
///
/// In order to prevent innacuracies caused by rounding already-rounded values, we read from `unrounded_layout`
/// and write to `final_layout`.
pub fn round_layout(tree: &mut impl RoundTree, node_id: NodeId) {
    return round_layout_inner(tree, node_id, 0.0, 0.0);

    /// Recursive function to apply rounding to all descendents
    fn round_layout_inner(tree: &mut impl RoundTree, node_id: NodeId, cumulative_x: f32, cumulative_y: f32) {
        let unrounded_layout = tree.get_unrounded_layout(node_id);
        let mut layout = unrounded_layout;

        let parent_x = cumulative_x;
        let parent_y = cumulative_y;
        let cumulative_x = cumulative_x + unrounded_layout.location.x;
        let cumulative_y = cumulative_y + unrounded_layout.location.y;

        layout.location.x = round(cumulative_x) - round(parent_x);
        layout.location.y = round(cumulative_y) - round(parent_y);
        layout.size.width = round(cumulative_x + unrounded_layout.size.width) - round(cumulative_x);
        layout.size.height = round(cumulative_y + unrounded_layout.size.height) - round(cumulative_y);
        layout.baselines.first =
            unrounded_layout.baselines.first.map(|value| round(cumulative_y + value) - round(cumulative_y));
        layout.baselines.last =
            unrounded_layout.baselines.last.map(|value| round(cumulative_y + value) - round(cumulative_y));
        layout.scrollbar_size.width = round(unrounded_layout.scrollbar_size.width);
        layout.scrollbar_size.height = round(unrounded_layout.scrollbar_size.height);
        layout.border.left = round(cumulative_x + unrounded_layout.border.left) - round(cumulative_x);
        layout.border.right = round(cumulative_x + unrounded_layout.size.width)
            - round(cumulative_x + unrounded_layout.size.width - unrounded_layout.border.right);
        layout.border.top = round(cumulative_y + unrounded_layout.border.top) - round(cumulative_y);
        layout.border.bottom = round(cumulative_y + unrounded_layout.size.height)
            - round(cumulative_y + unrounded_layout.size.height - unrounded_layout.border.bottom);
        layout.padding.left = round(cumulative_x + unrounded_layout.padding.left) - round(cumulative_x);
        layout.padding.right = round(cumulative_x + unrounded_layout.size.width)
            - round(cumulative_x + unrounded_layout.size.width - unrounded_layout.padding.right);
        layout.padding.top = round(cumulative_y + unrounded_layout.padding.top) - round(cumulative_y);
        layout.padding.bottom = round(cumulative_y + unrounded_layout.size.height)
            - round(cumulative_y + unrounded_layout.size.height - unrounded_layout.padding.bottom);

        #[cfg(feature = "content_size")]
        round_scrollable_overflow_rect(
            &mut layout,
            unrounded_layout.scrollable_overflow_rect,
            cumulative_x,
            cumulative_y,
        );

        tree.set_final_layout(node_id, &layout);

        // Recurse into in-flow children. Out-of-flow (absolute/fixed) children are skipped here:
        // they are instead visited via their containing block's hoisted child list below, which
        // ensures each node is visited exactly once and that its cumulative offset is accumulated
        // relative to its containing block (which its `location` is relative to).
        let child_count = tree.child_count(node_id);
        for index in 0..child_count {
            let child = tree.get_child_id(node_id, index);
            if !tree.is_out_of_flow(child) {
                round_layout_inner(tree, child, cumulative_x, cumulative_y);
            }
        }

        // Recurse into out-of-flow boxes for which this node is the containing block
        let hoisted_count = tree.hoisted_child_count(node_id);
        for index in 0..hoisted_count {
            let child = tree.get_hoisted_child_id(node_id, index);
            round_layout_inner(tree, child, cumulative_x, cumulative_y);
        }
    }

    #[cfg(feature = "content_size")]
    #[inline(always)]
    /// Round the scrollable overflow rect.
    /// This is split into a separate function to make it easier to feature flag.
    fn round_scrollable_overflow_rect(
        layout: &mut Layout,
        unrounded_rect: crate::geometry::Rect<f32>,
        cumulative_x: f32,
        cumulative_y: f32,
    ) {
        layout.scrollable_overflow_rect.left = round(cumulative_x + unrounded_rect.left) - round(cumulative_x);
        layout.scrollable_overflow_rect.right = round(cumulative_x + unrounded_rect.right) - round(cumulative_x);
        layout.scrollable_overflow_rect.top = round(cumulative_y + unrounded_rect.top) - round(cumulative_y);
        layout.scrollable_overflow_rect.bottom = round(cumulative_y + unrounded_rect.bottom) - round(cumulative_y);
    }
}

/// Creates a layout for this node and its children, recursively.
/// Each hidden node has zero size and is placed at the origin
pub fn compute_hidden_layout(tree: &mut (impl LayoutPartialTree + CacheTree), node: NodeId) -> LayoutOutput {
    tree.record_constraint_box_input(node, None);
    tree.record_custom_child_inputs(node, None);
    // Clear cache and set zeroed-out layout for the node
    tree.cache_clear(node);
    tree.set_unrounded_layout(node, &Layout::with_order(0));

    // Perform hidden layout on all children
    for index in 0..tree.child_count(node) {
        let child_id = tree.get_child_id(node, index);
        tree.compute_child_layout(child_id, LayoutInput::HIDDEN);
    }

    LayoutOutput::HIDDEN
}

/// A module for unified re-exports of detailed layout info structs, used by low level API
pub mod detailed_info {
    #[cfg(feature = "grid")]
    pub use super::grid::{
        DetailedGridInfo, DetailedGridItemsInfo, DetailedGridTracksInfo, GridLineNames, GridLineNamesIter,
    };
}

#[cfg(all(test, feature = "taffy_tree", feature = "flexbox"))]
mod bounds_tests;

#[cfg(test)]
mod tests {
    use super::compute_hidden_layout;
    use crate::geometry::{Point, Size};
    use crate::style::{Display, Style};
    use crate::TaffyTree;

    #[test]
    fn hidden_layout_should_hide_recursively() {
        let mut taffy: TaffyTree<()> = TaffyTree::new();

        let style: Style = Style { display: Display::Flex, size: Size::from_lengths(50.0, 50.0), ..Default::default() };

        let grandchild_00 = taffy.new_leaf(style.clone()).unwrap();
        let grandchild_01 = taffy.new_leaf(style.clone()).unwrap();
        let child_00 = taffy.new_with_children(style.clone(), &[grandchild_00, grandchild_01]).unwrap();

        let grandchild_02 = taffy.new_leaf(style.clone()).unwrap();
        let child_01 = taffy.new_with_children(style.clone(), &[grandchild_02]).unwrap();

        let root = taffy
            .new_with_children(
                Style { display: Display::None, size: Size::from_lengths(50.0, 50.0), ..Default::default() },
                &[child_00, child_01],
            )
            .unwrap();

        compute_hidden_layout(&mut taffy.as_layout_tree(), root);

        // Whatever size and display-mode the nodes had previously,
        // all layouts should resolve to ZERO due to the root's DISPLAY::NONE

        for node in [root, child_00, child_01, grandchild_00, grandchild_01, grandchild_02] {
            let layout = taffy.layout(node).unwrap();
            assert_eq!(layout.size, Size::zero());
            assert_eq!(layout.location, Point::zero());
        }
    }
}
