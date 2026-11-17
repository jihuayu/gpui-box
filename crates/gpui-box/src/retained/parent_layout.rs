//! Backend-independent custom parent measurement and child placement.
use super::*;

/// Whether this node's own preferred/min/max sizes participate. Descendant
/// styles, box-model insets, containment and parent hard inputs still apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayoutSizingMode {
    #[default]
    Inherent,
    Content,
}
impl LayoutSizingMode {
    fn to_taffy(self) -> taffy::SizingMode {
        match self {
            Self::Inherent => taffy::SizingMode::InherentSize,
            Self::Content => taffy::SizingMode::ContentSize,
        }
    }
    pub(super) fn from_taffy(mode: taffy::SizingMode) -> Self {
        match mode {
            taffy::SizingMode::InherentSize => Self::Inherent,
            taffy::SizingMode::ContentSize => Self::Content,
        }
    }
}

/// Immutable, typed metadata for a node's direct parent algorithm. It is not
/// inherited and ordinary Flex ignores it. Clone preserves identity; create a
/// new handle whenever its value changes, as for ParentLayoutSnapshot.
/// Interior mutation, mutable external state or invocation-dependent values
/// violate the layout purity contract even if Rust's type system permits them.
#[derive(Clone, Debug, PartialEq)]
pub struct ParentData(taffy::tree::CustomParentData);
impl ParentData {
    pub fn new<T: Send + Sync + 'static>(value: T) -> Self {
        Self(taffy::tree::CustomParentData::new(value))
    }
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
    pub(super) fn to_taffy(&self) -> taffy::tree::CustomParentData {
        self.0.clone()
    }
}

/// Independent allocation can read immutable direct-child metadata and item
/// style inputs, without measuring content or accessing mutable UI state.
pub trait ParentAllocationChildren {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Owned handle; borrowing a value through get() cannot outlive the handle.
    fn parent_data(&self, child: usize) -> Result<Option<ParentData>, Error>;
    /// Unresolved item inputs in layout order. Intrinsic keywords are data,
    /// not permission to measure content. Final builder observations include
    /// all siblings' effective styles, even if an allocator ignores a field.
    fn style(&self, _child: usize) -> Result<ChildLayoutStyle, Error> {
        Err(Error::InvalidParentLayout)
    }
}

/// Content-box inputs after the host's normal sizing and inset rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentLayoutInput {
    pub sizing_mode: LayoutSizingMode,
    pub known_size: LayoutSize<Option<f32>>,
    pub available: LayoutSize<layout::AvailableSpace>,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

/// A direct child's border-box assignment, constraints and percentage bases.
/// Selected size does not imply a tight interval or percentage definiteness.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildLayoutInput {
    /// Content ignores this child's own preferred/min/max styles, not its
    /// descendants' styles or parent assignments. It is not Min/MaxContent space.
    pub sizing_mode: LayoutSizingMode,
    /// Parent-selected dimensions. None lets normal child sizing choose an axis.
    pub known_size: LayoutSize<Option<f32>>,
    /// Whether a selected axis establishes a definite size for the child's own
    /// percentage descendants. Ignored for None; a tight hard interval is definite.
    pub known_size_is_definite: LayoutSize<bool>,
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}
impl Default for ChildLayoutInput {
    fn default() -> Self {
        Self::new(LayoutConstraints::new(
            LayoutAvailableSpace::Unbounded,
            LayoutAvailableSpace::Unbounded,
        ))
    }
}
impl ChildLayoutInput {
    pub const fn new(constraints: LayoutConstraints) -> Self {
        Self {
            sizing_mode: LayoutSizingMode::Inherent,
            known_size: LayoutSize::new(None, None),
            known_size_is_definite: LayoutSize::new(true, true),
            constraints,
            percentage_basis: LayoutSize::new(None, None),
        }
    }
    pub const fn with_sizing_mode(mut self, mode: LayoutSizingMode) -> Self {
        self.sizing_mode = mode;
        self
    }
    pub const fn with_known_size(
        mut self,
        size: LayoutSize<Option<f32>>,
        definite: LayoutSize<bool>,
    ) -> Self {
        self.known_size = size;
        self.known_size_is_definite = definite;
        self
    }
    pub(super) fn from_taffy(input: taffy::LayoutInput) -> Self {
        Self {
            sizing_mode: LayoutSizingMode::from_taffy(input.sizing_mode),
            known_size: from_size(input.known_dimensions),
            known_size_is_definite: from_size(input.known_dimensions_are_definite),
            constraints: LayoutConstraints::new(
                space(input.available_space.width),
                space(input.available_space.height),
            )
            .with_bounds(BoxConstraints::new(
                [input.size_bounds.min.width, input.size_bounds.min.height],
                [input.size_bounds.max.width, input.size_bounds.max.height],
            )),
            percentage_basis: from_size(input.parent_size),
        }
    }
    pub(super) fn to_taffy(self) -> Result<taffy::LayoutInput, Error> {
        self.constraints.validate()?;
        if [self.percentage_basis.width, self.percentage_basis.height]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidParentLayout);
        }
        let bounds = self.constraints.bounds.to_taffy()?;
        let selected = to_size(self.known_size);
        let valid = selected.zip_map(
            bounds.min.zip_map(bounds.max, |min, max| (min, max)),
            |size, (min, max)| {
                size.is_none_or(|size| size.is_finite() && size >= min && size <= max)
            },
        );
        if !valid.width || !valid.height {
            return Err(Error::InvalidParentLayout);
        }
        let tight = bounds.min.zip_map(bounds.max, |min, max| min == max);
        Ok(taffy::LayoutInput {
            size_bounds: bounds,
            known_dimensions: bounds
                .min
                .zip_map(bounds.max, |min, max| (min == max).then_some(min))
                .or(selected),
            known_dimensions_are_definite: to_size(self.known_size_is_definite)
                .zip_map(selected, |definite, selected| {
                    definite || selected.is_none()
                })
                .zip_map(tight, |definite, tight| definite || tight),
            parent_size: to_size(self.percentage_basis),
            available_space: Size {
                width: self.constraints.width.to_taffy()?,
                height: self.constraints.height.to_taffy()?,
            },
            run_mode: taffy::RunMode::ComputeSize,
            sizing_mode: self.sizing_mode.to_taffy(),
            axis: taffy::RequestedAxis::Both,
            vertical_margins_are_collapsible: taffy::Line::FALSE,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildLayoutResult {
    pub size: [f32; 2],
    /// First horizontal baseline from the child's border-box top.
    pub baseline: Option<f32>,
    /// Last horizontal baseline from the child's border-box top.
    pub last_baseline: Option<f32>,
}

/// Owned, unresolved item-facing inputs of a direct child. These are effective
/// kernel inputs (including replaced-image classification), not final geometry.
/// Container internals such as its own gap or child alignment stay encapsulated;
/// use normal child measurement to obtain their content contribution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildLayoutStyle {
    pub hidden: bool,
    pub direction: LayoutDirection,
    pub position: LayoutPosition,
    pub inset: LayoutEdges,
    pub size: LayoutSize<LayoutDimension>,
    pub min_size: LayoutSize<LayoutDimension>,
    pub max_size: LayoutSize<LayoutDimension>,
    pub aspect_ratio: Option<f32>,
    pub margin: LayoutEdges,
    pub padding: LayoutEdges,
    pub border: LayoutEdges,
    pub box_sizing: LayoutBoxSizing,
    pub overflow: LayoutSize<LayoutOverflow>,
    pub scrollbar_width: f32,
    pub size_containment: LayoutSize<bool>,
    pub layout_containment: bool,
    pub paint_containment: bool,
    pub is_replaced: bool,
    pub is_compressible_replaced: bool,
    pub flex_basis: LayoutDimension,
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub align_self: LayoutAlignment<LayoutAlignSelf>,
}

/// Only direct children are addressable. Measurements never publish geometry.
pub trait ParentLayoutChildren {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn parent_data(&self, _child: usize) -> Result<Option<ParentData>, Error> {
        Ok(None)
    }
    /// Read direct-child style in current layout order, without measuring it.
    /// The default rejects unsupported external implementations explicitly.
    fn style(&self, _child: usize) -> Result<ChildLayoutStyle, Error> {
        Err(Error::InvalidParentLayout)
    }
    /// An independent planner's enforced inline assignment, if installed.
    /// Measurement and final placement use this width even if the caller's
    /// ChildLayoutInput requests another width. Block inputs remain explicit.
    fn inline_allocation(&self, _child: usize) -> Result<Option<ChildInlineAllocation>, Error> {
        Ok(None)
    }
    /// Complete independently allocated input, enforced for all measurements
    /// and final placements. None without ParentAllocationLayout.
    fn allocation(&self, _child: usize) -> Result<Option<ChildLayoutInput>, Error> {
        Ok(None)
    }
    /// Size-oriented query. Baselines are only those available from the size
    /// algorithm; use measure_baselines when complete final metrics are needed.
    fn measure(
        &mut self,
        child: usize,
        input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error>;
    /// Complete first/last baseline query for the same child allocation. This
    /// may run ordinary container placement arithmetic without publishing it.
    /// The default is unsupported for external measure-only implementations.
    fn measure_baselines(
        &mut self,
        _child: usize,
        _input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error> {
        Err(Error::InvalidParentLayout)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParentPlacement {
    pub child: usize,
    pub input: ChildLayoutInput,
    /// Physical content-box coordinates. Negative offsets permit overflow.
    pub origin: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParentLayoutOutput {
    pub size: [f32; 2],
    /// First horizontal baseline from the host's content-box top. Must be
    /// finite; overflowing/negatively placed content can put it outside size.
    pub baseline: Option<f32>,
    /// Last horizontal baseline from the host's content-box top; finite, with
    /// no clamp to the host's content height.
    pub last_baseline: Option<f32>,
    /// Every child, including hidden nodes, must occur exactly once.
    pub placements: Vec<ParentPlacement>,
}

/// Pure content metrics. A dry query owns no placement plan and publishes no
/// geometry. Known content dimensions and baseline validity match layout().
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentMeasureOutput {
    pub size: [f32; 2],
    /// Finite signed content-box offset, not limited by host height.
    pub baseline: Option<f32>,
    /// Finite signed content-box offset, not limited by host height.
    pub last_baseline: Option<f32>,
}

/// Pure owned snapshot, called repeatedly for sizing queries and allocation.
/// It must not read mutable application state or depend on invocation count.
pub trait ParentMeasure: std::fmt::Debug + Send + Sync + 'static {
    /// Read the same enforced slots and immutable metadata as the full layout.
    /// At the same final content input, metrics must agree exactly with layout.
    fn measure(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentMeasureOutput, Error>;
}

/// Pure complete parent plan; an optional separate ParentMeasure phase can
/// answer size queries without constructing the placement list.
pub trait ParentLayout: std::fmt::Debug + Send + Sync + 'static {
    fn layout(
        &self,
        input: ParentLayoutInput,
        children: &mut dyn ParentLayoutChildren,
    ) -> Result<ParentLayoutOutput, Error>;
}

/// Width-only input: no block dimensions, child styles or measurements are
/// observable. The host box model still owns its final outer allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentInlineInput {
    pub sizing_mode: LayoutSizingMode,
    pub known_width: Option<f32>,
    pub available: layout::AvailableSpace,
    pub percentage_basis: Option<f32>,
}

/// A finite tight border-box width, enforced for measurement and final layout.
/// Percentage definiteness is independent from the tight assigned width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildInlineAllocation {
    pub width: f32,
    pub percentage_basis: Option<f32>,
}
impl ChildInlineAllocation {
    fn validate(self) -> Result<(), Error> {
        if !self.width.is_finite()
            || self.width < 0.0
            || self
                .percentage_basis
                .is_some_and(|v| !v.is_finite() || v < 0.0)
        {
            return Err(Error::InvalidParentLayout);
        }
        Ok(())
    }
    fn apply(self, mut input: ChildLayoutInput) -> ChildLayoutInput {
        input.known_size.width = Some(self.width);
        input.known_size_is_definite.width = true;
        input.constraints.width = layout::AvailableSpace::Definite(self.width);
        input.constraints.bounds.min.width = self.width;
        input.constraints.bounds.max.width = self.width;
        input.percentage_basis.width = self.percentage_basis;
        input
    }
}

/// Pure child-width allocation, invoked before the measured parent algorithm.
/// Return one entry in direct-child layout order, including hidden children.
/// Owned data and these inputs must be its only dependencies; mutable captures,
/// invocation counts and external state violate the same purity contract as
/// ParentLayout. Intrinsic/unbounded inputs also require a finite allocation.
pub trait ParentInlineLayout: std::fmt::Debug + Send + Sync + 'static {
    fn allocate(
        &self,
        input: ParentInlineInput,
        children: usize,
    ) -> Result<Vec<ChildInlineAllocation>, Error>;
}

/// Incoming border-box inputs, before the host's sizing/measurement. The known
/// dimensions and their definiteness are separate from hard intervals and soft
/// space. Host content dimensions/insets are deliberately not inferred here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentAllocationInput {
    pub sizing_mode: LayoutSizingMode,
    pub known_size: LayoutSize<Option<f32>>,
    pub known_size_is_definite: LayoutSize<bool>,
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

/// Pure assignment from incoming constraints and owned immutable configuration.
/// Unresolved child item styles and metadata are readable; child measurement
/// and measured host dimensions are unavailable.
/// Override allocate_children to observe item styles and immutable parent data.
/// Return one complete input per direct child in layout order, including hidden
/// nodes. Loose/unbounded/intrinsic inputs remain distinct and supported.
pub trait ParentAllocationLayout: std::fmt::Debug + Send + Sync + 'static {
    /// Count-only compatibility entry. Override this or allocate_children;
    /// leaving both at their defaults fails with InvalidParentLayout.
    fn allocate(
        &self,
        _input: ParentAllocationInput,
        _children: usize,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        Err(Error::InvalidParentLayout)
    }
    fn allocate_children(
        &self,
        input: ParentAllocationInput,
        children: &dyn ParentAllocationChildren,
    ) -> Result<Vec<ChildLayoutInput>, Error> {
        self.allocate(input, children.len())
    }
}

/// Clone this handle to preserve snapshot identity. Create a new snapshot when
/// any algorithm input changes; pointer equality gives precise same-value skips.
#[derive(Clone, Debug)]
pub struct ParentLayoutSnapshot {
    layout: Arc<dyn ParentLayout>,
    measure: Option<Arc<dyn ParentMeasure>>,
    inline: Option<Arc<dyn ParentInlineLayout>>,
    allocation: Option<Arc<dyn ParentAllocationLayout>>,
}
impl ParentLayoutSnapshot {
    pub fn new(delegate: impl ParentLayout) -> Self {
        Self {
            layout: Arc::new(delegate),
            measure: None,
            inline: None,
            allocation: None,
        }
    }
    /// Install pure content measurement. Complete final plans are still
    /// validated against its metrics at the same final content input.
    pub fn with_measure(mut self, measure: impl ParentMeasure) -> Self {
        self.measure = Some(Arc::new(measure));
        self
    }
    pub fn has_measure(&self) -> bool {
        self.measure.is_some()
    }
    /// Install a separate width-only phase. The framework enforces its tight
    /// assignments; this is not an unchecked declaration about layout().
    pub fn with_inline_layout(mut self, planner: impl ParentInlineLayout) -> Self {
        self.inline = Some(Arc::new(planner));
        self.allocation = None;
        self
    }
    pub fn has_inline_layout(&self) -> bool {
        self.inline.is_some()
    }
    /// Install complete assignment from incoming border-box constraints. This
    /// replaces an inline phase; their enforcement domains would overlap.
    pub fn with_allocation_layout(mut self, planner: impl ParentAllocationLayout) -> Self {
        self.allocation = Some(Arc::new(planner));
        self.inline = None;
        self
    }
    pub fn has_allocation_layout(&self) -> bool {
        self.allocation.is_some()
    }
    pub(super) fn to_taffy(&self) -> Arc<dyn taffy::tree::CustomLayout> {
        Arc::new(Adapter(self.clone()))
    }
}
impl PartialEq for ParentLayoutSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.layout, &other.layout)
            && match (&self.measure, &other.measure) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && match (&self.inline, &other.inline) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && match (&self.allocation, &other.allocation) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

pub(super) fn validate_node(style: &Style, measured: bool, custom: bool) -> Result<(), Error> {
    if custom && (measured || !matches!(style.display, taffy::Display::Flex | taffy::Display::None))
    {
        return Err(Error::InvalidParentLayout);
    }
    Ok(())
}

fn to_size<T>(size: LayoutSize<T>) -> Size<T> {
    Size {
        width: size.width,
        height: size.height,
    }
}
fn from_size<T>(size: Size<T>) -> LayoutSize<T> {
    LayoutSize::new(size.width, size.height)
}
fn space(value: taffy::AvailableSpace) -> layout::AvailableSpace {
    match value {
        taffy::AvailableSpace::Definite(v) => layout::AvailableSpace::Definite(v),
        taffy::AvailableSpace::Unbounded => layout::AvailableSpace::Unbounded,
        taffy::AvailableSpace::MinContent => layout::AvailableSpace::MinContent,
        taffy::AvailableSpace::MaxContent => layout::AvailableSpace::MaxContent,
    }
}
struct Children<'a> {
    inner: &'a mut dyn taffy::tree::CustomLayoutChildren,
    inline: Option<&'a [ChildInlineAllocation]>,
}
impl ParentLayoutChildren for Children<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
    fn parent_data(&self, child: usize) -> Result<Option<ParentData>, Error> {
        Ok(self.inner.parent_data(child)?.cloned().map(ParentData))
    }
    fn style(&self, child: usize) -> Result<ChildLayoutStyle, Error> {
        ChildLayoutStyle::from_taffy(self.inner.style(child)?)
    }
    fn inline_allocation(&self, child: usize) -> Result<Option<ChildInlineAllocation>, Error> {
        if child >= self.len() {
            return Err(Error::InvalidParentLayout);
        }
        Ok(self.inline.map(|slots| slots[child]))
    }
    fn allocation(&self, child: usize) -> Result<Option<ChildLayoutInput>, Error> {
        Ok(self
            .inner
            .allocation(child)?
            .map(ChildLayoutInput::from_taffy))
    }
    fn measure(
        &mut self,
        child: usize,
        input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error> {
        let input = self.allocation(child)?.unwrap_or(
            self.inline_allocation(child)?
                .map_or(input, |slot| slot.apply(input)),
        );
        let result = self.inner.measure(child, input.to_taffy()?)?;
        Ok(ChildLayoutResult {
            size: [result.size.width, result.size.height],
            baseline: result.baselines.first,
            last_baseline: result.baselines.last,
        })
    }
    fn measure_baselines(
        &mut self,
        child: usize,
        input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error> {
        let input = self.allocation(child)?.unwrap_or(
            self.inline_allocation(child)?
                .map_or(input, |slot| slot.apply(input)),
        );
        let result = self.inner.measure_baselines(child, input.to_taffy()?)?;
        Ok(ChildLayoutResult {
            size: [result.size.width, result.size.height],
            baseline: result.baselines.first,
            last_baseline: result.baselines.last,
        })
    }
}
#[derive(Debug)]
struct Adapter(ParentLayoutSnapshot);
impl Adapter {
    fn inline_slots(
        &self,
        input: ParentLayoutInput,
        count: usize,
    ) -> Result<Option<Vec<ChildInlineAllocation>>, taffy::TaffyError> {
        self.0
            .inline
            .as_ref()
            .map(|planner| {
                let slots = planner.allocate(
                    ParentInlineInput {
                        sizing_mode: input.sizing_mode,
                        known_width: input.known_size.width,
                        available: input.available.width,
                        percentage_basis: input.percentage_basis.width,
                    },
                    count,
                )?;
                if slots.len() != count {
                    return Err(Error::InvalidParentLayout);
                }
                for slot in &slots {
                    slot.validate()?;
                }
                Ok(slots)
            })
            .transpose()
            .map_err(|_: Error| taffy::TaffyError::InvalidCustomLayout)
    }
}
struct AllocationChildren<'a>(&'a dyn taffy::tree::CustomAllocationChildren);
impl ParentAllocationChildren for AllocationChildren<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn parent_data(&self, child: usize) -> Result<Option<ParentData>, Error> {
        Ok(self.0.parent_data(child)?.cloned().map(ParentData))
    }
    fn style(&self, child: usize) -> Result<ChildLayoutStyle, Error> {
        ChildLayoutStyle::from_taffy(self.0.style(child)?)
    }
}
impl taffy::tree::CustomLayout for Adapter {
    fn measure(
        &self,
        input: taffy::tree::CustomLayoutInput,
        children: &mut dyn taffy::tree::CustomLayoutChildren,
    ) -> Result<Option<taffy::tree::CustomMeasureOutput>, taffy::TaffyError> {
        let Some(measure) = &self.0.measure else {
            return Ok(None);
        };
        let input = ParentLayoutInput {
            sizing_mode: LayoutSizingMode::from_taffy(input.sizing_mode),
            known_size: from_size(input.known_size),
            available: from_size(input.available.map(space)),
            percentage_basis: from_size(input.percentage_basis),
        };
        let inline = self.inline_slots(input, children.len())?;
        measure
            .measure(
                input,
                &mut Children {
                    inner: children,
                    inline: inline.as_deref(),
                },
            )
            .map(|output| {
                Some(taffy::tree::CustomMeasureOutput {
                    size: Size {
                        width: output.size[0],
                        height: output.size[1],
                    },
                    baselines: taffy::Baselines {
                        first: output.baseline,
                        last: output.last_baseline,
                    },
                })
            })
            .map_err(|_| taffy::TaffyError::InvalidCustomLayout)
    }
    fn allocate(
        &self,
        input: taffy::LayoutInput,
        children: &dyn taffy::tree::CustomAllocationChildren,
    ) -> Result<Option<Vec<taffy::LayoutInput>>, taffy::TaffyError> {
        let Some(planner) = &self.0.allocation else {
            return Ok(None);
        };
        let child = ChildLayoutInput::from_taffy(input);
        let result = (|| -> Result<Vec<taffy::LayoutInput>, Error> {
            let slots = planner.allocate_children(
                ParentAllocationInput {
                    sizing_mode: child.sizing_mode,
                    known_size: from_size(input.known_dimensions),
                    known_size_is_definite: from_size(input.known_dimensions_are_definite),
                    constraints: child.constraints,
                    percentage_basis: child.percentage_basis,
                },
                &AllocationChildren(children),
            )?;
            if slots.len() != children.len() {
                return Err(Error::InvalidParentLayout);
            }
            slots.into_iter().map(ChildLayoutInput::to_taffy).collect()
        })();
        result
            .map(Some)
            .map_err(|_| taffy::TaffyError::InvalidCustomLayout)
    }
    fn layout(
        &self,
        input: taffy::tree::CustomLayoutInput,
        children: &mut dyn taffy::tree::CustomLayoutChildren,
    ) -> Result<taffy::tree::CustomLayoutOutput, taffy::TaffyError> {
        let input = ParentLayoutInput {
            sizing_mode: LayoutSizingMode::from_taffy(input.sizing_mode),
            known_size: from_size(input.known_size),
            available: from_size(input.available.map(space)),
            percentage_basis: from_size(input.percentage_basis),
        };
        let inline = self.inline_slots(input, children.len())?;
        let output = self
            .0
            .layout
            .layout(
                input,
                &mut Children {
                    inner: children,
                    inline: inline.as_deref(),
                },
            )
            .map_err(|_| taffy::TaffyError::InvalidCustomLayout)?;
        Ok(taffy::tree::CustomLayoutOutput {
            size: Size {
                width: output.size[0],
                height: output.size[1],
            },
            baselines: taffy::Baselines {
                first: output.baseline,
                last: output.last_baseline,
            },
            placements: output
                .placements
                .into_iter()
                .map(|p| {
                    Ok(taffy::tree::CustomPlacement {
                        child: p.child,
                        input: if let Some(slot) = children.allocation(p.child)? {
                            slot
                        } else {
                            inline
                                .as_ref()
                                .map_or(Ok(p.input), |slots| {
                                    slots
                                        .get(p.child)
                                        .map(|slot| slot.apply(p.input))
                                        .ok_or(taffy::TaffyError::InvalidCustomLayout)
                                })?
                                .to_taffy()
                                .map_err(|_| taffy::TaffyError::InvalidCustomLayout)?
                        },
                        origin: taffy::Point {
                            x: p.origin[0],
                            y: p.origin[1],
                        },
                    })
                })
                .collect::<Result<_, _>>()?,
        })
    }
}

#[cfg(test)]
mod allocation_style_tests;
#[cfg(test)]
mod assigned_size_tests;
mod child_style;
#[cfg(test)]
mod child_style_tests;
#[cfg(test)]
mod content_sizing_tests;
#[cfg(test)]
mod dry_baseline_tests;
#[cfg(test)]
mod dry_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod overflow_tests;
#[cfg(test)]
mod parent_data_tests;
#[cfg(test)]
mod tests;
