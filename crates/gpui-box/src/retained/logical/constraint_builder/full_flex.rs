//! Two-axis incoming Flex/Grid assignments whose sizes do not read constructed
//! content. The kernel remains the only allocator; this is a dependency proof.
use super::*;
mod grid;

#[derive(PartialEq)]
pub(in crate::retained::logical) struct Dependencies {
    parents: Vec<Parent>,
    measured_siblings: Vec<ContentInput>,
}
#[derive(PartialEq)]
struct Parent {
    node: NodeId,
    style: Arc<Style>,
    children: Vec<(NodeId, Arc<Style>)>,
}

/// Only layout-relevant inputs. Paint/material changes do not invalidate an
/// independent measurement. No user code is invoked to compare these records.
#[derive(PartialEq)]
struct ContentInput {
    node: NodeId,
    order: i32,
    style: Arc<Style>,
    children: Vec<NodeId>,
    measure: Option<MeasureSnapshot>,
    parent_layout: Option<ParentLayoutSnapshot>,
    parent_data: Option<ParentData>,
}

fn capture_content(
    tree: &UiTree,
    root: NodeId,
    records: &mut Vec<ContentInput>,
) -> Result<(), Error> {
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let node = tree.node(id)?;
        pending.extend(node.children.iter().rev().copied());
        records.push(ContentInput {
            node: id,
            order: node.layout_order,
            style: measure::effective_style(&node.style, node.measure.as_ref()),
            children: node.children.clone(),
            measure: node.measure.clone(),
            parent_layout: node.parent_layout.clone(),
            parent_data: node.parent_data.clone(),
        });
    }
    Ok(())
}

/// Both parent axes must be externally sized. Numeric main bases plus explicit
/// main minima eliminate intrinsic/automatic feedback for each path item.
/// In nowrap, a stretched cross size comes from the parent's fixed line size;
/// wrapped lines need explicit item cross sizes because their height/width can
/// otherwise depend on constructed content. Siblings may depend on their own
/// measured content, whose complete layout inputs are observed separately.
/// Grid edges instead need independent tracks on both definite axes and an
/// independently sized/stretched path item. All path percentages have definite
/// bases; Grid sibling content cannot alter these tracks.
pub(super) fn capture(tree: &UiTree, host: NodeId) -> Result<Option<Dependencies>, Error> {
    capture_with_constraints(tree, host, tree.layout_constraints)
}

pub(super) fn capture_with_constraints(
    tree: &UiTree,
    host: NodeId,
    constraints: Option<LayoutConstraints>,
) -> Result<Option<Dependencies>, Error> {
    let mut path = vec![];
    let mut next = Some(host);
    while let Some(id) = next {
        let node = tree.node(id)?;
        if node.parent_layout.is_some()
            || !matches!(
                node.style.display,
                taffy::Display::Flex | taffy::Display::Grid
            )
            || node.style.position.is_out_of_flow()
            || !plain_box(&measure::effective_style(
                &node.style,
                node.measure.as_ref(),
            ))
        {
            return Ok(None);
        }
        path.push(id);
        next = node.parent;
    }
    let Some(&root) = path.last() else {
        return Ok(None);
    };
    let style = &tree.node(root)?.style;
    if !root_axis(
        style.size.width,
        constraints.map(|c| (c.width, c.bounds.min.width, c.bounds.max.width)),
    ) || !root_axis(
        style.size.height,
        constraints.map(|c| (c.height, c.bounds.min.height, c.bounds.max.height)),
    ) {
        return Ok(None);
    }
    let mut records = vec![];
    let mut measured_siblings = vec![];
    for edge in path.windows(2).rev() {
        let child_on_path = edge[0];
        let parent = edge[1];
        let node = tree.node(parent)?;
        let row = matches!(
            node.style.flex_direction,
            taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
        );
        let mut children = vec![];
        if node.style.display == taffy::Display::Grid && !grid::independent_parent(tree, parent)? {
            return Ok(None);
        }
        for id in tree.ordered_children_for(parent, &node.children)? {
            let child = tree.node(id)?;
            let style = measure::effective_style(&child.style, child.measure.as_ref());
            if node.style.display == taffy::Display::Grid {
                if id == child_on_path && !grid::independent_item(&style, &node.style) {
                    return Ok(None);
                }
                children.push((id, style));
                continue;
            }
            if style.display != taffy::Display::None
                && !style.position.is_out_of_flow()
                && !independent_item(&style, &node.style, row)
            {
                if id == child_on_path {
                    return Ok(None);
                }
                // An ordinary sibling may use its own intrinsic/content size.
                // Its complete immutable layout inputs must settle before this
                // callback and remain unchanged afterwards. It cannot contain
                // the path child in the accepted tree.
                capture_content(tree, id, &mut measured_siblings)?;
            }
            children.push((id, style));
        }
        records.push(Parent {
            node: parent,
            style: node.style.clone(),
            children,
        });
    }
    // A root builder observes the incoming external slot, but ordinary root
    // layout still needs the explicitly sized host to avoid content feedback.
    if host == root {
        records.push(Parent {
            node: root,
            style: style.clone(),
            children: vec![],
        });
    }
    Ok(Some(Dependencies {
        parents: records,
        measured_siblings,
    }))
}

impl Dependencies {
    pub(super) fn measured_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.measured_siblings.iter().map(|record| record.node)
    }
}

fn numeric(value: taffy::Dimension) -> bool {
    use taffy::style::ExpandedDimension as D;
    match value.expand() {
        D::Length(v) | D::Percent(v) => v.is_finite() && v >= 0.0,
        D::Affine(px, percent) => px.is_finite() && percent.is_finite(),
        _ => false,
    }
}
fn plain_box(style: &Style) -> bool {
    style.aspect_ratio.is_none()
        && [style.size.width, style.size.height]
            .into_iter()
            .all(|v| v.is_auto() || v.is_stretch() || numeric(v))
        && [
            style.min_size.width,
            style.min_size.height,
            style.max_size.width,
            style.max_size.height,
        ]
        .into_iter()
        .all(|v| v.is_auto() || numeric(v))
}
fn root_axis(value: taffy::Dimension, input: Option<(LayoutAvailableSpace, f32, f32)>) -> bool {
    use taffy::style::ExpandedDimension as D;
    let external = input.is_some_and(|(space, min, max)| {
        matches!(space, LayoutAvailableSpace::Definite(_)) || min == max
    });
    match value.expand() {
        D::Length(v) => v.is_finite() && v >= 0.0,
        D::Percent(_) | D::Affine(_, _) | D::Stretch => external,
        _ => false,
    }
}
fn independent_item(style: &Style, parent: &Style, row: bool) -> bool {
    use taffy::style::ExpandedLengthPercentageAuto as A;
    if !plain_box(style) {
        return false;
    }
    let (main, min, cross) = if row {
        (style.size.width, style.min_size.width, style.size.height)
    } else {
        (style.size.height, style.min_size.height, style.size.width)
    };
    let basis = numeric(style.flex_basis) || (style.flex_basis.is_auto() && numeric(main));
    let align = style.align_self.unwrap_or(parent.align_items).keyword;
    // Wrapped baseline-sharing groups can enlarge a line even when all border
    // boxes have fixed cross sizes: their ascent/descent comes from content.
    if parent.flex_wrap != taffy::FlexWrap::NoWrap
        && matches!(
            align,
            taffy::AlignItemsKeyword::Baseline | taffy::AlignItemsKeyword::LastBaseline
        )
    {
        return false;
    }
    let margins = if row {
        [style.margin.top, style.margin.bottom]
    } else {
        [style.margin.left, style.margin.right]
    };
    let stretch = (cross.is_stretch()
        || cross.is_auto()
            && matches!(
                align,
                taffy::AlignItemsKeyword::Normal | taffy::AlignItemsKeyword::Stretch
            ))
        && !margins.into_iter().any(|v| matches!(v.expand(), A::Auto));
    basis
        && numeric(min)
        && (numeric(cross) || parent.flex_wrap == taffy::FlexWrap::NoWrap && stretch)
}

#[cfg(test)]
mod sibling_tests;
#[cfg(test)]
mod tests;
