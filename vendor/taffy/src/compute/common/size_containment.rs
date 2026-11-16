//! Size containment uses the box's empty-content size without hiding children.
use crate::geometry::Size;
use crate::style::CoreStyle;
use crate::tree::{LayoutInput, RunMode};

/// Resolve only contained axes using the same box model as a zero-content leaf.
/// Uncontained axes remain unresolved so that content still participates there.
/// A known dimension supplied by the parent always wins. Final layout still
/// runs the normal container algorithm for children, overflow and out-of-flow.
pub(crate) fn contained_size(
    inputs: LayoutInput,
    style: &impl CoreStyle,
    calc: impl Fn(*const (), f32) -> f32,
) -> Size<Option<f32>> {
    let axes = style.contain().size_containment();
    if !axes.width && !axes.height {
        return Size::NONE;
    }
    let empty = crate::compute::compute_leaf_layout(
        LayoutInput { run_mode: RunMode::ComputeSize, ..inputs },
        style,
        calc,
        |_, _| Size::ZERO,
    );
    Size { width: axes.width.then_some(empty.size.width), height: axes.height.then_some(empty.size.height) }
}
