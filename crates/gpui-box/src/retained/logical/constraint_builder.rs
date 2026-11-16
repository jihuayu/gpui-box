//! Parent-input construction, distinct from queries of used content size.
use super::*;

/// Inputs assigned to a constraint box's direct child slot. Bounds are hard;
/// available space preserves normal unbounded and explicit intrinsic modes.
/// A percentage basis is present only when the parent establishes it. None of
/// these fields is the child's used size, a paint viewport, or a CSS media query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstraintBuilderInput {
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

pub(super) fn validate_host(tree: &UiTree, host: NodeId) -> Result<(), Error> {
    let style = &tree.node(host)?.style;
    if !matches!(style.display, taffy::Display::ConstraintBox { .. })
        || !style
            .contain
            .contains(taffy::Contain::SIZE | taffy::Contain::LAYOUT)
    {
        return Err(Error::UnsafeConstraintBuilder);
    }
    Ok(())
}

pub(super) fn input(tree: &UiTree, host: NodeId) -> Result<ConstraintBuilderInput, Error> {
    validate_host(tree, host)?;
    let input = tree
        .layout
        .constraint_box_input(tree.node(host)?.layout)?
        .ok_or(Error::UnsafeConstraintBuilder)?;
    fn space(value: taffy::AvailableSpace) -> layout::AvailableSpace {
        match value {
            taffy::AvailableSpace::Definite(value) => layout::AvailableSpace::Definite(value),
            taffy::AvailableSpace::Unbounded => layout::AvailableSpace::Unbounded,
            taffy::AvailableSpace::MinContent => layout::AvailableSpace::MinContent,
            taffy::AvailableSpace::MaxContent => layout::AvailableSpace::MaxContent,
        }
    }
    let constraints = LayoutConstraints::new(
        space(input.available_space.width),
        space(input.available_space.height),
    )
    .with_bounds(BoxConstraints::new(
        [input.size_bounds.min.width, input.size_bounds.min.height],
        [input.size_bounds.max.width, input.size_bounds.max.height],
    ));
    constraints.validate()?;
    Ok(ConstraintBuilderInput {
        constraints,
        percentage_basis: LayoutSize::new(input.parent_size.width, input.parent_size.height),
    })
}

#[cfg(test)]
mod tests;
