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
    pub sizing_mode: LayoutSizingMode,
    /// Parent-selected border-box dimensions, independent of hard intervals.
    /// Flex's final known size is not represented by SizeBounds alone.
    pub known_size: LayoutSize<Option<f32>>,
    /// A known used size need not establish a percentage basis.
    pub known_size_is_definite: LayoutSize<bool>,
    pub constraints: LayoutConstraints,
    pub percentage_basis: LayoutSize<Option<f32>>,
}

/// Only the inline-axis assignment is observable. Block size may be derived
/// from the selected child content without becoming a structural dependency.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineConstraintBuilderInput {
    pub sizing_mode: LayoutSizingMode,
    /// Parent-selected border-box width, independently of tight/loose bounds.
    pub known_width: Option<f32>,
    pub known_width_is_definite: bool,
    /// Hard minimum border-box width of the observed assignment.
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
    InlineAllocation(
        NodeId,
        InlineConstraintBuilderInput,
        layout_dependencies::InlineDependencies,
    ),
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
                    && dependencies.input(tree, *node)? == *previous
            }
            Self::InlineAllocation(node, previous, dependencies) => {
                inline_allocation_input(tree, *node)? == *previous
                    && dependencies.matches(tree, *node)?
            }
        };
        if equal {
            Ok(())
        } else {
            Err(Error::UnsafeConstraintBuilder)
        }
    }
}

/// Independent allocation edges reach either the external root or a full-size
/// containment cut, or independently sized ordinary Flex/Grid ancestry.
/// Numeric equality cannot prove absence of
/// content -> parent -> builder feedback. Styles and ordered child lists remain
/// dependencies even when a replacement happens to return equal numbers.
/// Direct-child metadata/effective styles on independent allocators are inputs; host
/// descendants remain free to change because they cannot affect incoming slots.
#[derive(PartialEq)]
pub(super) enum AllocationDependencies {
    Independent(
        Vec<AllocationParentDependency>,
        Vec<(NodeId, Arc<Style>, Option<ParentData>)>,
    ),
    Ordinary(full_flex::Dependencies),
}
type AllocationParentDependency = (
    NodeId,
    Arc<Style>,
    Option<ParentLayoutSnapshot>,
    Vec<NodeId>,
);
impl AllocationDependencies {
    /// Scheduling observes proven Flex measurement dependencies, including those
    /// below content-independent Grid assignments.
    /// A not-yet-valid path may be established by an earlier ancestor task;
    /// ordinary builder validation still runs immediately before its callback.
    pub(super) fn measured_task_nodes(
        tree: &UiTree,
        host: NodeId,
        constraints: Option<LayoutConstraints>,
    ) -> Result<BTreeSet<NodeId>, Error> {
        match Self::capture_independent(tree, host) {
            Ok(_) => return Ok(BTreeSet::new()),
            Err(Error::UnsafeConstraintBuilder) => {}
            Err(error) => return Err(error),
        }
        Ok(
            full_flex::capture_with_constraints(tree, host, constraints)?
                .map(|proof| proof.measured_nodes().collect())
                .unwrap_or_default(),
        )
    }
    pub(super) fn capture(tree: &UiTree, host: NodeId) -> Result<Self, Error> {
        // Preserve the existing independent-parent/containment domain first:
        // it intentionally permits content-derived outer ancestors.
        Self::capture_independent(tree, host).or_else(|error| {
            if error != Error::UnsafeConstraintBuilder {
                return Err(error);
            }
            full_flex::capture(tree, host)?
                .map(Self::Ordinary)
                .ok_or(error)
        })
    }
    fn capture_independent(tree: &UiTree, host: NodeId) -> Result<Self, Error> {
        let mut records = vec![];
        let mut data = vec![];
        let mut next = Some(host);
        let mut cut = false;
        while let Some(id) = next {
            let node = tree.node(id)?;
            let style = &node.style;
            // Below the isolation cut intrinsic resolution can feed descendants
            // into a subsequent assignment. Above it, contained size/baseline/
            // overflow no longer depend on this builder's constructed content.
            if style.position.is_out_of_flow()
                || (cut && node.parent_layout.is_some())
                || !matches!(
                    style.display,
                    taffy::Display::Flex | taffy::Display::ConstraintBox { .. }
                )
                || (!cut
                    && (style.size.width.is_sizing_keyword()
                        || style.size.height.is_sizing_keyword()
                        || style.min_size.width.is_sizing_keyword()
                        || style.min_size.height.is_sizing_keyword()
                        || style.max_size.width.is_sizing_keyword()
                        || style.max_size.height.is_sizing_keyword()))
            {
                return Err(Error::UnsafeConstraintBuilder);
            }
            if !cut {
                if let Some(parent) = node.parent {
                    let parent = tree.node(parent)?;
                    if parent
                        .parent_layout
                        .as_ref()
                        .is_some_and(ParentLayoutSnapshot::has_allocation_layout)
                    {
                        // Enforced pre-measurement assignments preserve the cut.
                    } else if parent.parent_layout.is_none()
                        && matches!(parent.style.display, taffy::Display::ConstraintBox { .. })
                        && full_containment(&parent.style)
                    {
                        // Its final outgoing slot depends on its empty-content
                        // size, never on this child's constructed content.
                    } else if parent.parent_layout.is_none()
                        && matches!(parent.style.display, taffy::Display::Flex)
                        && full_containment(style)
                    {
                        cut = true;
                    } else {
                        return Err(Error::UnsafeConstraintBuilder);
                    }
                } else if id != tree.root || (id == host && !full_containment(style)) {
                    return Err(Error::UnsafeConstraintBuilder);
                }
            }
            let children = if id == host {
                vec![]
            } else {
                tree.ordered_children_for(id, &node.children)?
            };
            if node
                .parent_layout
                .as_ref()
                .is_some_and(ParentLayoutSnapshot::has_allocation_layout)
            {
                for &child in &children {
                    let node = tree.node(child)?;
                    data.push((
                        child,
                        measure::effective_style(&node.style, node.measure.as_ref()),
                        node.parent_data.clone(),
                    ));
                }
            }
            records.push((id, style.clone(), node.parent_layout.clone(), children));
            next = node.parent;
        }
        Ok(Self::Independent(records, data))
    }
    pub(super) fn tracks_input(&self) -> bool {
        matches!(self, Self::Ordinary(_))
    }
    pub(super) fn input(
        &self,
        tree: &UiTree,
        host: NodeId,
    ) -> Result<ConstraintBuilderInput, Error> {
        if self.tracks_input() {
            let input = tree
                .layout
                .tracked_layout_input(tree.node(host)?.layout)?
                .ok_or(Error::UnsafeConstraintBuilder)?;
            let input = builder_input(input);
            input.constraints.validate()?;
            Ok(input)
        } else {
            allocation_input(tree, host)
        }
    }
}

pub(super) fn allocation_input(
    tree: &UiTree,
    host: NodeId,
) -> Result<ConstraintBuilderInput, Error> {
    let node = tree.node(host)?;
    let Some(parent) = node.parent else {
        return contained_input(tree, host);
    };
    let parent_node = tree.node(parent)?;
    if parent_node.parent_layout.is_none() {
        if matches!(
            parent_node.style.display,
            taffy::Display::ConstraintBox { .. }
        ) && full_containment(&parent_node.style)
        {
            return read_input(tree, parent);
        }
        return contained_input(tree, host);
    }
    let children = tree.ordered_children_for(parent, &tree.node(parent)?.children)?;
    let index = children
        .iter()
        .position(|child| *child == host)
        .ok_or(Error::UnsafeConstraintBuilder)?;
    let input = tree
        .layout
        .custom_child_input(tree.node(parent)?.layout, index)?
        .ok_or(Error::UnsafeConstraintBuilder)?;
    Ok(builder_input(input))
}

fn full_containment(style: &Style) -> bool {
    style
        .contain
        .contains(taffy::Contain::SIZE | taffy::Contain::LAYOUT)
}

fn contained_input(tree: &UiTree, host: NodeId) -> Result<ConstraintBuilderInput, Error> {
    if !full_containment(&tree.node(host)?.style) {
        return Err(Error::UnsafeConstraintBuilder);
    }
    let input = tree
        .layout
        .contained_layout_input(tree.node(host)?.layout)?
        .ok_or(Error::UnsafeConstraintBuilder)?;
    Ok(builder_input(input))
}

fn builder_input(input: taffy::LayoutInput) -> ConstraintBuilderInput {
    let child = parent_layout::ChildLayoutInput::from_taffy(input);
    ConstraintBuilderInput {
        sizing_mode: child.sizing_mode,
        known_size: LayoutSize::new(input.known_dimensions.width, input.known_dimensions.height),
        known_size_is_definite: LayoutSize::new(
            input.known_dimensions_are_definite.width,
            input.known_dimensions_are_definite.height,
        ),
        constraints: child.constraints,
        percentage_basis: child.percentage_basis,
    }
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
        || !layout_dependencies::independent_inline_chain(tree, host, style)?
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
        sizing_mode: input.sizing_mode,
        known_width: input.known_size.width,
        known_width_is_definite: input.known_size_is_definite.width,
        min: input.constraints.bounds.min.width,
        max: input.constraints.bounds.max.width,
        available: input.constraints.width,
        percentage_basis: input.percentage_basis.width,
    })
}

pub(super) fn validate_inline_allocation_host(tree: &UiTree, host: NodeId) -> Result<(), Error> {
    let node = tree.node(host)?;
    if !matches!(
        node.style.display,
        taffy::Display::Flex | taffy::Display::Grid | taffy::Display::ConstraintBox { .. }
    ) || !layout_dependencies::independent_inline_allocation_chain(tree, host, &node.style)?
    {
        return Err(Error::UnsafeConstraintBuilder);
    }
    Ok(())
}

pub(super) fn inline_allocation_input(
    tree: &UiTree,
    host: NodeId,
) -> Result<InlineConstraintBuilderInput, Error> {
    validate_inline_allocation_host(tree, host)?;
    // A percentage/stretch root in indefinite or intrinsic space can still
    // shrink-wrap descendants. The authored Column proof alone cannot turn
    // that unresolved root width into an independent external allocation.
    let constraints = tree
        .layout_constraints
        .ok_or(Error::UnsafeConstraintBuilder)?;
    if !layout_dependencies::has_external_inline_anchor(tree, constraints)? {
        return Err(Error::UnsafeConstraintBuilder);
    }
    let input = tree
        .layout
        .tracked_layout_input(tree.node(host)?.layout)?
        .ok_or(Error::UnsafeConstraintBuilder)?;
    let input = builder_input(input);
    input.constraints.validate()?;
    Ok(InlineConstraintBuilderInput {
        sizing_mode: input.sizing_mode,
        known_width: input.known_size.width,
        known_width_is_definite: input.known_size_is_definite.width,
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
    let input = builder_input(input);
    input.constraints.validate()?;
    Ok(input)
}

#[cfg(test)]
mod allocation_tests;
mod full_flex;
#[cfg(test)]
mod inline_allocation_tests;
#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod mixed_allocation_tests;
#[cfg(test)]
mod tests;
