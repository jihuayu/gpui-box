//! Complete Grid assignments from definite tracks, composed with ordinary Flex.
use super::*;

pub(super) fn independent_parent(tree: &UiTree, parent: NodeId) -> Result<bool, Error> {
    let node = tree.node(parent)?;
    let style = &node.style;
    if !layout_dependencies::independent_grid_axis(
        &style.grid_template_columns,
        &style.grid_auto_columns,
    ) || !layout_dependencies::independent_grid_axis(
        &style.grid_template_rows,
        &style.grid_auto_rows,
    ) || style.grid_template_areas.is_some()
        || !style.grid_template_column_names.is_empty()
        || !style.grid_template_row_names.is_empty()
    {
        return Ok(false);
    }
    for &id in &node.children {
        let child = tree.node(id)?;
        let item = measure::effective_style(&child.style, child.measure.as_ref());
        if item.display == taffy::Display::None || item.position.is_out_of_flow() {
            continue;
        }
        if !layout_dependencies::numeric_grid_placement(&item)
            || [
                item.align_self.unwrap_or(style.align_items),
                item.justify_self.unwrap_or(style.justify_items),
            ]
            .iter()
            .any(|align| {
                matches!(
                    align.keyword,
                    taffy::AlignItemsKeyword::Baseline | taffy::AlignItemsKeyword::LastBaseline
                )
            })
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn independent_item(item: &Style, parent: &Style) -> bool {
    use taffy::style::ExpandedLengthPercentageAuto as A;
    if !plain_box(item) || item.display == taffy::Display::None || item.position.is_out_of_flow() {
        return false;
    }
    let stretched = |size: taffy::Dimension,
                     align: taffy::AlignItems,
                     margins: [taffy::LengthPercentageAuto; 2]| {
        size.is_stretch()
            || size.is_auto()
                && (align == taffy::AlignItems::STRETCH
                    || align.keyword == taffy::AlignItemsKeyword::Normal
                        && !item.item_is_replaced
                        && !item.item_is_compressible_replaced)
                && !margins
                    .into_iter()
                    .any(|margin| matches!(margin.expand(), A::Auto))
    };
    (numeric(item.size.width)
        || stretched(
            item.size.width,
            item.justify_self.unwrap_or(parent.justify_items),
            [item.margin.left, item.margin.right],
        ))
        && (numeric(item.size.height)
            || stretched(
                item.size.height,
                item.align_self.unwrap_or(parent.align_items),
                [item.margin.top, item.margin.bottom],
            ))
}

#[cfg(test)]
mod tests;
