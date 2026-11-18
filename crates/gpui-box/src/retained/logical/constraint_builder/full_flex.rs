//! Two-axis incoming Flex assignments whose sizes do not read constructed
//! content. The kernel remains the only allocator; this is a dependency proof.
use super::*;

#[derive(PartialEq)]
pub(in crate::retained::logical) struct Dependencies(Vec<Parent>);
#[derive(PartialEq)]
struct Parent {
    node: NodeId,
    style: Arc<Style>,
    children: Vec<(NodeId, Arc<Style>)>,
}

/// Both parent axes must be externally sized. Numeric main bases plus explicit
/// main minima eliminate intrinsic/automatic main-size feedback for every item.
/// In nowrap, a stretched cross size comes from the parent's fixed line size;
/// wrapped lines need explicit item cross sizes because their height/width can
/// otherwise depend on constructed content. All percentages have definite bases.
pub(super) fn capture(tree: &UiTree, host: NodeId) -> Result<Option<Dependencies>, Error> {
    let mut path = vec![];
    let mut next = Some(host);
    while let Some(id) = next {
        let node = tree.node(id)?;
        if node.parent_layout.is_some()
            || node.style.display != taffy::Display::Flex
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
    let constraints = tree.layout_constraints;
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
    for &parent in path.iter().skip(1).rev() {
        let node = tree.node(parent)?;
        let row = matches!(
            node.style.flex_direction,
            taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
        );
        let mut children = vec![];
        for id in tree.ordered_children_for(parent, &node.children)? {
            let child = tree.node(id)?;
            let style = measure::effective_style(&child.style, child.measure.as_ref());
            if style.display != taffy::Display::None
                && !style.position.is_out_of_flow()
                && !independent_item(&style, &node.style, row)
            {
                return Ok(None);
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
    Ok(Some(Dependencies(records)))
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
mod tests;
