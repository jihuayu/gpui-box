//! Backend-independent custom parent measurement and child placement.
use super::*;

/// Content-box inputs after the host's normal sizing and inset rules.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentLayoutInput {
    pub known_size: LayoutSize<Option<f32>>,
    pub available: LayoutSize<layout::AvailableSpace>,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

/// A direct child's independent border-box constraints and percentage bases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildLayoutInput {
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}
impl ChildLayoutInput {
    pub fn new(constraints: LayoutConstraints) -> Self {
        Self {
            constraints,
            percentage_basis: LayoutSize::new(None, None),
        }
    }
    fn to_taffy(self) -> Result<taffy::LayoutInput, Error> {
        self.constraints.validate()?;
        if [self.percentage_basis.width, self.percentage_basis.height]
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidParentLayout);
        }
        let bounds = self.constraints.bounds.to_taffy()?;
        Ok(taffy::LayoutInput {
            size_bounds: bounds,
            known_dimensions: bounds
                .min
                .zip_map(bounds.max, |min, max| (min == max).then_some(min)),
            known_dimensions_are_definite: Size {
                width: true,
                height: true,
            },
            parent_size: to_size(self.percentage_basis),
            available_space: Size {
                width: self.constraints.width.to_taffy()?,
                height: self.constraints.height.to_taffy()?,
            },
            run_mode: taffy::RunMode::ComputeSize,
            sizing_mode: taffy::SizingMode::InherentSize,
            axis: taffy::RequestedAxis::Both,
            vertical_margins_are_collapsible: taffy::Line::FALSE,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChildLayoutResult {
    pub size: [f32; 2],
    pub baseline: Option<f32>,
}

/// Only direct children are addressable. Measurements never publish geometry.
pub trait ParentLayoutChildren {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// An independent planner's enforced inline assignment, if installed.
    /// Measurement and final placement use this width even if the caller's
    /// ChildLayoutInput requests another width. Block inputs remain explicit.
    fn inline_allocation(&self, _child: usize) -> Result<Option<ChildInlineAllocation>, Error> {
        Ok(None)
    }
    fn measure(
        &mut self,
        child: usize,
        input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error>;
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
    pub baseline: Option<f32>,
    /// Every child, including hidden nodes, must occur exactly once.
    pub placements: Vec<ParentPlacement>,
}

/// Pure owned snapshot, called repeatedly for sizing queries and allocation.
/// It must not read mutable application state or depend on invocation count.
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

/// Clone this handle to preserve snapshot identity. Create a new snapshot when
/// any algorithm input changes; pointer equality gives precise same-value skips.
#[derive(Clone, Debug)]
pub struct ParentLayoutSnapshot {
    layout: Arc<dyn ParentLayout>,
    inline: Option<Arc<dyn ParentInlineLayout>>,
}
impl ParentLayoutSnapshot {
    pub fn new(delegate: impl ParentLayout) -> Self {
        Self {
            layout: Arc::new(delegate),
            inline: None,
        }
    }
    /// Install a separate width-only phase. The framework enforces its tight
    /// assignments; this is not an unchecked declaration about layout().
    pub fn with_inline_layout(mut self, planner: impl ParentInlineLayout) -> Self {
        self.inline = Some(Arc::new(planner));
        self
    }
    pub fn has_inline_layout(&self) -> bool {
        self.inline.is_some()
    }
    pub(super) fn to_taffy(&self) -> Arc<dyn taffy::tree::CustomLayout> {
        Arc::new(Adapter(self.clone()))
    }
}
impl PartialEq for ParentLayoutSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.layout, &other.layout)
            && match (&self.inline, &other.inline) {
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
    fn inline_allocation(&self, child: usize) -> Result<Option<ChildInlineAllocation>, Error> {
        if child >= self.len() {
            return Err(Error::InvalidParentLayout);
        }
        Ok(self.inline.map(|slots| slots[child]))
    }
    fn measure(
        &mut self,
        child: usize,
        input: ChildLayoutInput,
    ) -> Result<ChildLayoutResult, Error> {
        let input = self
            .inline_allocation(child)?
            .map_or(input, |slot| slot.apply(input));
        let result = self.inner.measure(child, input.to_taffy()?)?;
        Ok(ChildLayoutResult {
            size: [result.size.width, result.size.height],
            baseline: result.baselines.first,
        })
    }
}
#[derive(Debug)]
struct Adapter(ParentLayoutSnapshot);
impl taffy::tree::CustomLayout for Adapter {
    fn layout(
        &self,
        input: taffy::tree::CustomLayoutInput,
        children: &mut dyn taffy::tree::CustomLayoutChildren,
    ) -> Result<taffy::tree::CustomLayoutOutput, taffy::TaffyError> {
        let input = ParentLayoutInput {
            known_size: from_size(input.known_size),
            available: from_size(input.available.map(space)),
            percentage_basis: from_size(input.percentage_basis),
        };
        let inline = self
            .0
            .inline
            .as_ref()
            .map(|planner| {
                let slots = planner.allocate(
                    ParentInlineInput {
                        known_width: input.known_size.width,
                        available: input.available.width,
                        percentage_basis: input.percentage_basis.width,
                    },
                    children.len(),
                )?;
                if slots.len() != children.len() {
                    return Err(Error::InvalidParentLayout);
                }
                for slot in &slots {
                    slot.validate()?;
                }
                Ok(slots)
            })
            .transpose()
            .map_err(|_: Error| taffy::TaffyError::InvalidCustomLayout)?;
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
            baselines: taffy::Baselines::from_first(output.baseline),
            placements: output
                .placements
                .into_iter()
                .map(|p| {
                    Ok(taffy::tree::CustomPlacement {
                        child: p.child,
                        input: inline
                            .as_ref()
                            .map_or(Ok(p.input), |slots| {
                                slots
                                    .get(p.child)
                                    .map(|slot| slot.apply(p.input))
                                    .ok_or(taffy::TaffyError::InvalidCustomLayout)
                            })?
                            .to_taffy()
                            .map_err(|_| taffy::TaffyError::InvalidCustomLayout)?,
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
mod inline_tests;
#[cfg(test)]
mod tests;
