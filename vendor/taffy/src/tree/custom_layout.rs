//! Owned parent algorithms. Kept outside Style so ordinary nodes stay compact.
use crate::util::sys::Vec;
use crate::{AvailableSpace, Baselines, LayoutInput, LayoutOutput, Point, Size, TaffyError};
#[cfg(not(feature = "std"))]
use alloc::sync::Arc;
use core::any::Any;
#[cfg(feature = "std")]
use std::sync::Arc;

/// Opaque, owned metadata attached to a child for its direct parent algorithm.
/// Clone preserves identity; replacement must use a new handle. Values must be
/// immutable for the same reason as owned layout delegates.
#[derive(Clone)]
pub struct CustomParentData(Arc<dyn Any + Send + Sync>);
impl CustomParentData {
    /// Own a typed value without exposing layout-engine or application state.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value))
    }
    /// Read the value only when its concrete type matches.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}
impl core::fmt::Debug for CustomParentData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("CustomParentData").field(&(*self.0).type_id()).finish()
    }
}
impl PartialEq for CustomParentData {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Read-only direct-child metadata, without style or measurement capability.
pub trait CustomAllocationChildren {
    /// Direct children in layout order, including hidden children.
    fn len(&self) -> usize;
    /// Whether no children are present.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Metadata remains attached to a node when it moves to another parent.
    fn parent_data(&self, child: usize) -> Result<Option<&CustomParentData>, TaffyError>;
}

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
    /// Same immutable metadata seen by the independent allocation phase.
    fn parent_data(&self, _child: usize) -> Result<Option<&CustomParentData>, TaffyError> {
        Ok(None)
    }
    /// A separate phase's enforced full assignment, when installed.
    fn allocation(&self, _child: usize) -> Result<Option<LayoutInput>, TaffyError> {
        Ok(None)
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

/// Pure content measurement, without child placements or published geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CustomMeasureOutput {
    /// Content-box dimensions; preserve every known dimension exactly.
    pub size: Size<f32>,
    /// Content-box baselines, subject to the same validity as layout output.
    pub baselines: Baselines,
}

/// Immutable, repeatable layout snapshot. The engine may invoke this multiple
/// times for intrinsic queries and final allocation, or skip it on cache hits.
pub trait CustomLayout: core::fmt::Debug + Send + Sync {
    /// Optional size/baseline-only query. None falls back to the complete plan.
    /// Uses the same enforced slots, metadata and purity contract as layout.
    /// A final layout must agree with Some at the same final content input.
    fn measure(
        &self,
        _input: CustomLayoutInput,
        _children: &mut dyn CustomLayoutChildren,
    ) -> Result<Option<CustomMeasureOutput>, TaffyError> {
        Ok(None)
    }
    /// Independent assignments from the incoming border-box input, before host
    /// measurement/sizing. Only child order/count and immutable parent data are
    /// observable. No child styles or measurement capability is given.
    /// Slots are enforced throughout this invocation, including intrinsic
    /// probes and final placement. None preserves the measured-parent protocol.
    fn allocate(
        &self,
        _input: LayoutInput,
        _children: &dyn CustomAllocationChildren,
    ) -> Result<Option<Vec<LayoutInput>>, TaffyError> {
        Ok(None)
    }
    /// Must depend only on owned snapshot data, inputs and child measurements.
    fn layout(
        &self,
        input: CustomLayoutInput,
        children: &mut dyn CustomLayoutChildren,
    ) -> Result<CustomLayoutOutput, TaffyError>;
}
