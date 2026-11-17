//! Shared proofs that content block-size changes cannot alter inline allocation.
use super::*;

pub(super) trait InlineContext {
    fn tree(&self) -> &UiTree;
    /// All styles reachable during this plan, including current source values.
    fn styles(&self, node: NodeId) -> Result<Vec<Style>, Error>;
}

impl InlineContext for UiTree {
    fn tree(&self) -> &UiTree {
        self
    }
    fn styles(&self, node: NodeId) -> Result<Vec<Style>, Error> {
        Ok(vec![self.node(node)?.style.clone()])
    }
}

/// Custom width plans observe child count/order and owned snapshot data. A
/// structural builder cannot change those dependencies after consuming a slot,
/// even if its numeric allocation happens to remain equal in this execution.
#[derive(Clone)]
pub(super) struct InlineDependencies(Vec<(NodeId, ParentLayoutSnapshot, Vec<NodeId>)>);
impl InlineDependencies {
    pub(super) fn capture(tree: &UiTree, node: NodeId) -> Result<Self, Error> {
        let mut result = vec![];
        let mut parent = tree.node(node)?.parent;
        while let Some(id) = parent {
            let node = tree.node(id)?;
            if let Some(snapshot) = &node.parent_layout {
                result.push((
                    id,
                    snapshot.clone(),
                    tree.ordered_children_for(id, &node.children)?,
                ));
            }
            parent = node.parent;
        }
        Ok(Self(result))
    }
    pub(super) fn matches(&self, tree: &UiTree, node: NodeId) -> Result<bool, Error> {
        Ok(self.0 == Self::capture(tree, node)?.0)
    }
}

/// A column's stretched/definite inline size is independent of content height.
/// A Tight constraint-box edge also assigns inline size from its own allocation.
/// The proof walks every ancestor and every reachable authored alternative;
/// aspect feedback, OOF and intrinsic width limits reject rather than iterate.
pub(super) fn column_inline_chain(
    context: &impl InlineContext,
    node: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    prove(context, node, style, &mut Default::default(), false)
}

/// Incoming soft space and percentage basis depend on the entire ancestor
/// chain, even when the host's own CSS width is locked. A local fixed-width
/// boundary proves used content width, not these observable parent inputs.
pub(super) fn column_inline_allocation_chain(
    context: &impl InlineContext,
    node: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    prove(context, node, style, &mut Default::default(), true)
}

fn prove(
    context: &impl InlineContext,
    node: NodeId,
    style: &Style,
    memo: &mut std::collections::HashMap<NodeId, bool>,
    incoming: bool,
) -> Result<bool, Error> {
    use taffy::style::{ExpandedDimension as D, ExpandedLengthPercentageAuto as A};
    if style.position.is_out_of_flow()
        || !matches!(
            style.display,
            taffy::Display::Flex | taffy::Display::ConstraintBox { .. } | taffy::Display::None
        )
    {
        return Ok(false);
    }
    let parent = context.tree().node(node)?.parent;
    if let Some(parent) = parent
        && let Some(delegate) = &context.tree().node(parent)?.parent_layout
    {
        // The independent phase cannot see block dimensions/measurements. Its
        // tight child width overrides authored width limits/aspect coupling;
        // its independent percentage basis also fixes percentage insets.
        // The parent's own width must still be independent of content height.
        if !delegate.has_inline_layout() {
            return Ok(false);
        }
        return prove_alternatives(context, parent, memo, incoming);
    }
    if style.aspect_ratio.is_some()
        || style.min_size.width.is_sizing_keyword()
        || style.max_size.width.is_sizing_keyword()
    {
        return Ok(false);
    }
    let definite = matches!(
        style.size.width.expand(),
        D::Length(_) | D::Percent(_) | D::Stretch
    );
    let Some(parent) = parent else {
        return Ok(definite);
    };
    let parents = context.styles(parent)?;
    // CSS Flex cannot grow/shrink an item's exact min=max border-box width.
    // With numeric inline insets its content width is independent of parent
    // height/allocation, including a Row/wrap/aspect ancestor. A non-CSS box
    // edge can override authored width with hard bounds, so it cannot stop here.
    if !incoming
        && fixed_inline_content(style)
        && parents
            .iter()
            .all(|parent| matches!(parent.display, taffy::Display::Flex | taffy::Display::None))
    {
        return Ok(true);
    }
    for parent_style in &parents {
        if parent_style.aspect_ratio.is_some() {
            return Ok(false);
        }
        match parent_style.display {
            taffy::Display::ConstraintBox {
                width: taffy::ConstraintFit::Tight,
                ..
            } => {}
            taffy::Display::Flex | taffy::Display::None => {
                if !matches!(
                    parent_style.flex_direction,
                    taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse
                ) || parent_style.flex_wrap != taffy::FlexWrap::NoWrap
                {
                    return Ok(false);
                }
                let alignment = style.align_self.unwrap_or(parent_style.align_items);
                let stretched = style.size.width.is_auto()
                    && matches!(
                        alignment.keyword,
                        taffy::AlignItemsKeyword::Stretch | taffy::AlignItemsKeyword::Normal
                    )
                    && !matches!(style.margin.left.expand(), A::Auto)
                    && !matches!(style.margin.right.expand(), A::Auto);
                if !definite && !stretched {
                    return Ok(false);
                }
            }
            _ => return Ok(false),
        }
    }
    prove_alternatives(context, parent, memo, incoming)
}

fn prove_alternatives(
    context: &impl InlineContext,
    parent: NodeId,
    memo: &mut std::collections::HashMap<NodeId, bool>,
    incoming: bool,
) -> Result<bool, Error> {
    if let Some(valid) = memo.get(&parent) {
        return Ok(*valid);
    }
    for parent_style in &context.styles(parent)? {
        if !prove(context, parent, parent_style, memo, incoming)? {
            memo.insert(parent, false);
            return Ok(false);
        }
    }
    memo.insert(parent, true);
    Ok(true)
}

fn fixed_inline_content(style: &Style) -> bool {
    use taffy::style::{ExpandedDimension as D, ExpandedLengthPercentage as L};
    let locked = matches!((style.size.width.expand(), style.min_size.width.expand(), style.max_size.width.expand()),
        (D::Length(w), D::Length(min), D::Length(max)) if w == min && w == max && w.is_finite() && w >= 0.0);
    locked
        && style.box_sizing == taffy::BoxSizing::BorderBox
        && [
            style.padding.left,
            style.padding.right,
            style.border.left,
            style.border.right,
        ]
        .iter()
        .all(
            |inset| matches!(inset.expand(), L::Length(value) if value.is_finite() && value >= 0.0),
        )
}
