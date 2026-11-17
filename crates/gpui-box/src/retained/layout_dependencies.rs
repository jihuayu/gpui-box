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

/// Custom width plans and Row allocation observe child count/order and owned
/// algorithm/item inputs. A builder cannot change them after consuming a slot,
/// even if its numeric allocation happens to remain equal in this execution.
#[derive(Clone)]
pub(super) struct InlineDependencies(Vec<InlineParentDependency>);
#[derive(Clone, PartialEq)]
struct InlineParentDependency {
    node: NodeId,
    algorithm: Option<ParentLayoutSnapshot>,
    children: Vec<NodeId>,
    item_styles: Vec<(NodeId, Style)>,
}
impl InlineDependencies {
    pub(super) fn capture(tree: &UiTree, node: NodeId) -> Result<Self, Error> {
        let mut result = vec![];
        let mut parent = tree.node(node)?.parent;
        while let Some(id) = parent {
            let node = tree.node(id)?;
            let row = matches!(node.style.display, taffy::Display::Flex)
                && matches!(
                    node.style.flex_direction,
                    taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
                );
            if node.parent_layout.is_some() || row {
                let children = tree.ordered_children_for(id, &node.children)?;
                let styles = if row {
                    children
                        .iter()
                        .map(|id| {
                            let child = tree.node(*id)?;
                            Ok((
                                *id,
                                measure::effective_style(&child.style, child.measure.as_ref()),
                            ))
                        })
                        .collect::<Result<Vec<_>, Error>>()?
                } else {
                    vec![]
                };
                result.push(InlineParentDependency {
                    node: id,
                    algorithm: node.parent_layout.clone(),
                    children,
                    item_styles: styles,
                });
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
/// Rows qualify when every item has a content-independent numeric main basis
/// and explicit numeric minimum, including wrapping and reversed rows. Incoming
/// proofs can also resolve percentage inputs along a definite ordinary-Flex
/// chain, conditional on the caller's live external inline anchor.
/// A Tight constraint-box edge also assigns inline size from its own allocation.
/// The proof walks every ancestor and every reachable authored alternative;
/// aspect feedback, OOF and intrinsic width limits reject rather than iterate.
pub(super) fn independent_inline_chain(
    context: &impl InlineContext,
    node: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    prove(context, node, style, &mut Default::default(), false)
}

/// Incoming soft space and percentage basis depend on the entire ancestor
/// chain, even when the host's own CSS width is locked. A local fixed-width
/// boundary proves used content width, not these observable parent inputs.
pub(super) fn independent_inline_allocation_chain(
    context: &impl InlineContext,
    node: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    prove(context, node, style, &mut Default::default(), true)
}

/// A syntactic percentage/stretch root needs an actual external width before
/// content construction or uncontained content queries may rely on the chain.
pub(super) fn has_external_inline_anchor(
    tree: &UiTree,
    constraints: LayoutConstraints,
) -> Result<bool, Error> {
    use taffy::style::ExpandedDimension as D;
    Ok(matches!(
        tree.node(tree.root)?.style.size.width.expand(),
        D::Length(_)
    ) || matches!(constraints.width, layout::AvailableSpace::Definite(_))
        || constraints.bounds.min.width == constraints.bounds.max.width)
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
            taffy::Display::Flex
                | taffy::Display::Grid
                | taffy::Display::ConstraintBox { .. }
                | taffy::Display::None
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
        D::Length(_) | D::Percent(_) | D::Affine(_, _) | D::Stretch
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
                if matches!(
                    parent_style.flex_direction,
                    taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
                ) {
                    if !independent_row_items(context, parent, incoming)? {
                        return Ok(false);
                    }
                    continue;
                }
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

/// Numeric bases/limits remove content from Row allocation. Percentages also
/// qualify along a definite ordinary-Flex chain, but only the incoming proof
/// carries the caller's live external-anchor requirement. The used-width proof
/// remains valid without that requirement for existing containment consumers.
fn independent_row_items(
    context: &impl InlineContext,
    parent: NodeId,
    incoming: bool,
) -> Result<bool, Error> {
    use taffy::style::ExpandedDimension as D;
    let mut percentage_proof = None;
    for &child in &context.tree().node(parent)?.children {
        let node = context.tree().node(child)?;
        for style in context.styles(child)? {
            let style = measure::effective_style(&style, node.measure.as_ref());
            if style.display == taffy::Display::None || style.position.is_out_of_flow() {
                continue;
            }
            let needs_percentage = [
                style.flex_basis,
                style.min_size.width,
                style.max_size.width,
                if style.flex_basis.is_auto() {
                    style.size.width
                } else {
                    taffy::Dimension::auto()
                },
            ]
            .iter()
            .any(|v| matches!(v.expand(), D::Percent(_) | D::Affine(_, _)));
            if incoming && needs_percentage && percentage_proof.is_none() {
                percentage_proof = Some(definite_flex_inline_chain(
                    context,
                    parent,
                    &mut Default::default(),
                )?);
            }
            let percentages = percentage_proof == Some(true);
            let numeric = |v: taffy::Dimension| match v.expand() {
                D::Length(x) => x.is_finite() && x >= 0.0,
                D::Percent(x) => percentages && x.is_finite() && x >= 0.0,
                D::Affine(px, percent) => percentages && px.is_finite() && percent.is_finite(),
                _ => false,
            };
            let basis = numeric(style.flex_basis)
                || (style.flex_basis.is_auto() && numeric(style.size.width));
            if !basis
                || !numeric(style.min_size.width)
                || !(style.max_size.width.is_auto() || numeric(style.max_size.width))
                || style.size.width.is_sizing_keyword()
                || style.aspect_ratio.is_some()
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Conditional on a live definite external root anchor, ordinary Flex assigns
/// a definite inline size down these edges. This is separate from independence:
/// a Row item's post-flexing width is definite when the Row width is definite,
/// even when its basis is content-dependent. `prove` checks that dependency.
/// Custom/box protocols are deliberately excluded: a custom selected width can
/// be independent yet explicitly not establish a percentage basis.
fn definite_flex_inline_chain(
    context: &impl InlineContext,
    node: NodeId,
    memo: &mut std::collections::HashMap<NodeId, bool>,
) -> Result<bool, Error> {
    use taffy::style::{ExpandedDimension as D, ExpandedLengthPercentageAuto as A};
    if let Some(valid) = memo.get(&node) {
        return Ok(*valid);
    }
    let current = context.tree().node(node)?;
    let parent = current.parent;
    let mut valid = current.parent_layout.is_none();
    if let Some(parent) = parent {
        valid &= definite_flex_inline_chain(context, parent, memo)?;
    }
    for style in context.styles(node)? {
        if !valid || style.display != taffy::Display::Flex || style.position.is_out_of_flow() {
            valid = false;
            break;
        }
        let numeric = matches!(
            style.size.width.expand(),
            D::Length(_) | D::Percent(_) | D::Affine(_, _)
        );
        let Some(parent) = parent else {
            valid &= numeric || style.size.width.is_stretch();
            continue;
        };
        for parent_style in context.styles(parent)? {
            if matches!(
                parent_style.flex_direction,
                taffy::FlexDirection::Row | taffy::FlexDirection::RowReverse
            ) {
                // Definite parent main size makes post-flexing main size definite.
                continue;
            }
            let align = style.align_self.unwrap_or(parent_style.align_items);
            let stretched = (style.size.width.is_auto()
                && matches!(
                    align.keyword,
                    taffy::AlignItemsKeyword::Stretch | taffy::AlignItemsKeyword::Normal
                )
                || style.size.width.is_stretch())
                && !matches!(style.margin.left.expand(), A::Auto)
                && !matches!(style.margin.right.expand(), A::Auto);
            valid &= numeric || stretched;
        }
    }
    memo.insert(node, valid);
    Ok(valid)
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

#[cfg(test)]
mod percentage_tests;
#[cfg(test)]
mod row_tests;
