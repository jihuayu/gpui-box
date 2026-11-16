//! Owned parent algorithms. Kept outside Style so ordinary nodes stay compact.
use crate::util::sys::Vec;
use crate::{AvailableSpace, Baselines, LayoutInput, LayoutOutput, Point, Size, TaffyError};

/// Content-box inputs after the common host box model has resolved sizing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomLayoutInput {
    /// Assigned content dimensions. Returned size must preserve these values.
    pub known_size: Size<Option<f32>>,
    /// Soft content space, preserving intrinsic and normal indefinite modes.
    pub available: Size<AvailableSpace>,
    /// Established content-box percentage bases, independent of soft space.
    pub percentage_basis: Size<Option<f32>>,
}

/// Measure-only access to direct children; cannot reach an ancestor or write UI.
pub trait CustomLayoutChildren {
    /// Includes hidden children, whose measure/layout size remains zero.
    fn len(&self) -> usize;
    /// Whether there are no direct children.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The engine forces ComputeSize and disables collapsing margins.
    fn measure(&mut self, child: usize, input: LayoutInput) -> Result<LayoutOutput, TaffyError>;
}

/// One direct child's parent-owned border-box assignment and physical offset.
#[derive(Clone, Debug)]
pub struct CustomPlacement {
    /// Direct child index, in layout order.
    pub child: usize,
    /// Border-box inputs used for both measurement and final layout.
    pub input: LayoutInput,
    /// Physical content-box coordinates, with no implicit RTL mirroring.
    pub origin: Point<f32>,
}

/// A complete pure plan. Every direct child must occur exactly once.
#[derive(Clone, Debug)]
pub struct CustomLayoutOutput {
    /// Requested content size, before the host box model adds insets.
    pub size: Size<f32>,
    /// Optional content-box baseline. Validated before publication.
    pub baselines: Baselines,
    /// Placement order does not change retained paint or hit-test order.
    pub placements: Vec<CustomPlacement>,
}

/// Immutable, repeatable layout snapshot. The engine may invoke this multiple
/// times for intrinsic queries and final allocation, or skip it on cache hits.
pub trait CustomLayout: core::fmt::Debug + Send + Sync {
    /// Must depend only on owned snapshot data, inputs and child measurements.
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError>;
}
