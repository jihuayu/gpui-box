//! Parent-input construction, distinct from queries of used content size.
use super::*;

/// Assigned parent inputs: constraint_builder reads a constraint box's outgoing
/// child slot; allocation_builder reads the host's incoming border-box slot.
/// Bounds are hard;
/// available space preserves normal unbounded and explicit intrinsic modes.
/// A percentage basis is present only when the parent establishes it. None of
/// these fields is the child's used size, a paint viewport, or a CSS media query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstraintBuilderInput {
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

/// Only the inline-axis assignment is observable. Block size may be derived
/// from the selected child content without becoming a structural dependency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineConstraintBuilderInput {
    /// Hard minimum border-box width of the direct child slot.
    pub min: f32,
    /// Hard maximum width, possibly positive infinity. This is not a used size.
    pub max: f32,
    /// Normal layout space or explicit intrinsic mode for this axis.
    pub available: layout::AvailableSpace,
    /// The parent's independently established inline percentage basis.
    pub percentage_basis: Option<f32>,
}

pub(super) enum Observation {
    Full(NodeId, ConstraintBuilderInput),
    Inline(
        NodeId,
        InlineConstraintBuilderInput,
        layout_dependencies::InlineDependencies,
    ),
    Allocation(NodeId, ConstraintBuilderInput, AllocationDependencies),
}
impl Observation {
    pub(super) fn validate(&self, tree: &UiTree) -> Result<(), Error> {
        let equal = match self {
            Self::Full(node, previous) => input(tree, *node)? == *previous,
            Self::Inline(node, previous, dependencies) => {
                inline_input(tree, *node)? == *previous && dependencies.matches(tree, *node)?
            }
            Self::Allocation(node, previous, dependencies) => {
                AllocationDependencies::capture(tree, *node)? == *dependencies
                    && allocation_input(tree, *node)? == *previous
            }
        };
        if equal {
            Ok(())
        } else {
            Err(Error::UnsafeConstraintBuilder)
        }
    }
}

/// The proof domain is deliberately structural: only independent allocation
/// edges from the root. Numeric preview equality alone cannot prove absence of
/// content -> parent -> builder feedback. Styles and ordered child lists remain
/// dependencies even when a replacement happens to return equal numbers.
#[derive(PartialEq)]
pub(super) struct AllocationDependencies(
    Vec<(NodeId, Style, Option<ParentLayoutSnapshot>, Vec<NodeId>)>,
);
impl AllocationDependencies {
    pub(super) fn capture(tree: &UiTree, host: NodeId) -> Result<Self, Error> {
        let mut records = vec![];
        let mut next = Some(host);
        while let Some(id) = next {
            let node = tree.node(id)?;
            let style = &node.style;
            // Preferred/min/max intrinsic resolution may measure descendants
            // before the parent-owned entry. No dependency is silently ignored.
            if style.position.is_out_of_flow()
                || style.size.width.is_sizing_keyword()
                || style.size.height.is_sizing_keyword()
                || style.min_size.width.is_sizing_keyword()
                || style.min_size.height.is_sizing_keyword()
                || style.max_size.width.is_sizing_keyword()
                || style.max_size.height.is_sizing_keyword()
                || !matches!(style.display, taffy::Display::Flex)
            {
                return Err(Error::UnsafeConstraintBuilder);
            }
            if let Some(parent) = node.parent {
                if !tree
                    .node(parent)?
                    .parent_layout
                    .as_ref()
                    .is_some_and(ParentLayoutSnapshot::has_allocation_layout)
                {
                    return Err(Error::UnsafeConstraintBuilder);
                }
            } else if id != tree.root || id == host {
                return Err(Error::UnsafeConstraintBuilder);
            }
            records.push((
                id,
                style.clone(),
                node.parent_layout.clone(),
                if id == host {
                    vec![]
                } else {
                    tree.ordered_children_for(id, &node.children)?
                },
            ));
            next = node.parent;
        }
        Ok(Self(records))
    }
}

pub(super) fn allocation_input(
    tree: &UiTree,
    host: NodeId,
) -> Result<ConstraintBuilderInput, Error> {
    let parent = tree
        .node(host)?
        .parent
        .ok_or(Error::UnsafeConstraintBuilder)?;
    let children = tree.ordered_children_for(parent, &tree.node(parent)?.children)?;
    let index = children
        .iter()
        .position(|child| *child == host)
        .ok_or(Error::UnsafeConstraintBuilder)?;
    let input = tree
        .layout
        .custom_child_input(tree.node(parent)?.layout, index)?
        .ok_or(Error::UnsafeConstraintBuilder)?;
    let input = parent_layout::ChildLayoutInput::from_taffy(input);
    Ok(ConstraintBuilderInput {
        constraints: input.constraints,
        percentage_basis: input.percentage_basis,
    })
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
    read_input(tree, host)
}

pub(super) fn validate_inline_host(tree: &UiTree, host: NodeId) -> Result<(), Error> {
    let style = &tree.node(host)?.style;
    if !matches!(style.display, taffy::Display::ConstraintBox { .. }) {
        return Err(Error::UnsafeConstraintBuilder);
    }
    if style
        .contain
        .contains(taffy::Contain::SIZE | taffy::Contain::LAYOUT)
    {
        return Ok(());
    }
    if !style
        .contain
        .contains(taffy::Contain::INLINE_SIZE | taffy::Contain::LAYOUT)
        || !layout_dependencies::column_inline_chain(tree, host, style)?
    {
        return Err(Error::UnsafeConstraintBuilder);
    }
    Ok(())
}

pub(super) fn inline_input(
    tree: &UiTree,
    host: NodeId,
) -> Result<InlineConstraintBuilderInput, Error> {
    validate_inline_host(tree, host)?;
    let input = read_input(tree, host)?;
    Ok(InlineConstraintBuilderInput {
        min: input.constraints.bounds.min.width,
        max: input.constraints.bounds.max.width,
        available: input.constraints.width,
        percentage_basis: input.percentage_basis.width,
    })
}

fn read_input(tree: &UiTree, host: NodeId) -> Result<ConstraintBuilderInput, Error> {
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
mod allocation_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod tests;
