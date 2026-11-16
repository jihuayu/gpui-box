//! Dependency-checked container queries over fixed or size-contained extents.
use super::*;

type Rule<'a> = (
    Signal<LayoutStyle>,
    NodeId,
    LayoutQuery,
    &'a LayoutBreakpointStyles,
);

pub(super) trait QueryContext {
    fn tree(&self) -> &UiTree;
    fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error>;
    fn targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error>;
    fn validate_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        binding: reactive::BindingId,
    ) -> Result<(), Error>;
}

impl QueryContext for LogicalUi {
    fn tree(&self) -> &UiTree {
        self.reactive().tree()
    }
    fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error> {
        self.read(source)
    }
    fn targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error> {
        self.reactive().responsive_layout_targets(source)
    }
    fn validate_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        binding: reactive::BindingId,
    ) -> Result<(), Error> {
        self.reactive()
            .validate_responsive_writer(source, target, binding)
    }
}

impl QueryContext for reactive::responsive::Draft<'_> {
    fn tree(&self) -> &UiTree {
        self.tree()
    }
    fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error> {
        self.style(source)
    }
    fn targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error> {
        self.layout_targets(source)
    }
    fn validate_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        binding: reactive::BindingId,
    ) -> Result<(), Error> {
        self.validate_writer(source, target, binding)
    }
}

impl LayoutResponsiveBindings {
    pub(super) fn validate_layout_rules(
        &self,
        ui: &impl QueryContext,
        candidate: Option<Rule<'_>>,
    ) -> Result<Vec<Vec<usize>>, Error> {
        let tree = ui.tree();
        for binding in &self.bindings {
            ui.validate_writer(binding.source, binding.target, binding.binding)?;
        }
        let rules: Vec<_> = self
            .bindings
            .iter()
            .map(|binding| {
                (
                    binding.source,
                    binding.target,
                    binding.query,
                    &binding.styles,
                )
            })
            .chain(candidate)
            .collect();
        for (_, target, query, _) in &rules {
            tree.node(*target)?;
            let LayoutQuery::ContainerInline(host) = *query else {
                continue;
            };
            let host_node = tree.node(host)?;
            if *target == host || !Self::is_ancestor(tree, host, *target)? {
                return Err(Error::UnsafeContainerQuery);
            }
            validate_query_host(ui, &rules, host, &host_node.style)?;
            // A host may itself be controlled by an outer query/viewport rule.
            // Every branch and the source synchronized at bind time must keep
            // its inline size independent of descendant measurement.
            for (source, producer, _, styles) in &rules {
                if *producer == host {
                    validate_query_host(ui, &rules, host, &ui.style(*source)?.to_taffy()?)?;
                    for style in &styles.styles {
                        validate_query_host(ui, &rules, host, &style.to_taffy()?)?;
                    }
                }
            }
        }
        query_stages(ui, &rules)
    }
}

fn query_stages(ui: &impl QueryContext, rules: &[Rule<'_>]) -> Result<Vec<Vec<usize>>, Error> {
    let queries = rules
        .iter()
        .enumerate()
        .filter_map(|(i, (_, _, query, _))| {
            matches!(query, LayoutQuery::ContainerInline(_)).then_some(i)
        })
        .collect::<Vec<_>>();
    let mut dependents = vec![vec![]; queries.len()];
    let mut indegrees = vec![0; queries.len()];
    let targets = rules
        .iter()
        .map(|(source, target, _, _)| {
            let mut targets = ui.targets(*source)?;
            // Candidate registration validates before its direct sink is installed.
            if !targets.contains(target) {
                targets.push(*target);
            }
            Ok(targets)
        })
        .collect::<Result<Vec<_>, Error>>()?;
    for (consumer, &index) in queries.iter().enumerate() {
        let LayoutQuery::ContainerInline(host) = rules[index].2 else {
            unreachable!()
        };
        for (producer, &other) in queries.iter().enumerate() {
            let mut affects = false;
            for &target in &targets[other] {
                affects |= affects_query(ui, rules, target, host)?;
            }
            if affects {
                dependents[producer].push(consumer);
                indegrees[consumer] += 1;
            }
        }
    }
    let mut ready = indegrees
        .iter()
        .enumerate()
        .filter_map(|(index, &degree)| (degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut stages = vec![];
    let mut visited = 0;
    while !ready.is_empty() {
        visited += ready.len();
        let mut next = vec![];
        for &producer in &ready {
            for &consumer in &dependents[producer] {
                indegrees[consumer] -= 1;
                if indegrees[consumer] == 0 {
                    next.push(consumer);
                }
            }
        }
        stages.push(ready.into_iter().map(|index| queries[index]).collect());
        ready = next;
    }
    if visited != queries.len() {
        return Err(Error::UnsafeContainerQuery);
    }
    Ok(stages)
}

/// A fixed inline extent depends only on changes to its host/ancestors. A fluid
/// extent also depends on sibling allocation and content-dependent ancestors.
/// Walk the producer's ancestors to its first invariant full size/layout
/// boundary: changes inside that boundary cannot affect external allocation.
fn affects_query(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    target: NodeId,
    host: NodeId,
) -> Result<bool, Error> {
    let tree = ui.tree();
    if LayoutResponsiveBindings::is_ancestor(tree, target, host)? {
        return Ok(true);
    }
    let mut fluid = !external_inline_extent(ui, rules, host, &tree.node(host)?.style)?;
    for (source, _, _, styles) in rules.iter().filter(|(_, target, _, _)| *target == host) {
        fluid |= !external_inline_extent(ui, rules, host, &ui.style(*source)?.to_taffy()?)?;
        for style in &styles.styles {
            fluid |= !external_inline_extent(ui, rules, host, &style.to_taffy()?)?;
        }
    }
    if !fluid {
        return Ok(false);
    }
    let mut parent = tree.node(target)?.parent;
    while let Some(boundary) = parent {
        let mut invariant = full_size_boundary(&tree.node(boundary)?.style);
        for (source, _, _, styles) in rules.iter().filter(|(_, target, _, _)| *target == boundary) {
            invariant &= ui.style(*source)?.container_type == LayoutContainerType::Size
                && styles
                    .styles
                    .iter()
                    .all(|style| style.container_type == LayoutContainerType::Size);
        }
        if invariant {
            return Ok(
                boundary != host && LayoutResponsiveBindings::is_ancestor(tree, boundary, host)?
            );
        }
        parent = tree.node(boundary)?.parent;
    }
    Ok(true)
}

fn full_size_boundary(style: &Style) -> bool {
    style
        .contain
        .contains(taffy::Contain::SIZE | taffy::Contain::LAYOUT)
        && matches!(style.display, taffy::Display::Flex | taffy::Display::None)
}

fn fixed_inline_extent(style: &Style) -> bool {
    use taffy::style::ExpandedDimension as D;
    matches!((style.size.width.expand(),style.min_size.width.expand(),style.max_size.width.expand()),
        (D::Length(w),D::Length(min),D::Length(max)) if w == min && w == max)
        && style.box_sizing == taffy::BoxSizing::BorderBox
}

fn validate_query_host(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    host: NodeId,
    style: &Style,
) -> Result<(), Error> {
    if full_size_boundary(style) || external_inline_extent(ui, rules, host, style)? {
        return Ok(());
    }
    Err(Error::UnsafeContainerQuery)
}

fn external_inline_extent(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    host: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    if validate_inline_extent(style).is_ok() {
        return Ok(true);
    }
    if !style
        .contain
        .contains(taffy::Contain::INLINE_SIZE | taffy::Contain::LAYOUT)
    {
        return Ok(false);
    }
    // Inline containment cuts content-width contributions. The column chain
    // proves auto heights cannot feed a cross-axis width through an ancestor.
    column_inline_chain(ui, rules, host, style)
}

fn possible_styles(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    node: NodeId,
) -> Result<Vec<Style>, Error> {
    let mut result = vec![ui.tree().node(node)?.style.clone()];
    for (source, _, _, styles) in rules.iter().filter(|(_, target, _, _)| *target == node) {
        result.push(ui.style(*source)?.to_taffy()?);
        for style in &styles.styles {
            result.push(style.to_taffy()?);
        }
    }
    Ok(result)
}

fn column_inline_chain(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    node: NodeId,
    style: &Style,
) -> Result<bool, Error> {
    column_inline_style(ui, rules, node, style, &mut Default::default())
}

fn column_inline_style(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    node: NodeId,
    style: &Style,
    memo: &mut std::collections::HashMap<NodeId, bool>,
) -> Result<bool, Error> {
    use taffy::style::{ExpandedDimension as D, ExpandedLengthPercentageAuto as A};
    if style.aspect_ratio.is_some()
        || style.position.is_out_of_flow()
        || style.min_size.width.is_sizing_keyword()
        || style.max_size.width.is_sizing_keyword()
        || !matches!(style.display, taffy::Display::Flex | taffy::Display::None)
    {
        return Ok(false);
    }
    let definite_width = matches!(
        style.size.width.expand(),
        D::Length(_) | D::Percent(_) | D::Stretch
    );
    let Some(parent) = ui.tree().node(node)?.parent else {
        return Ok(definite_width);
    };
    let parents = possible_styles(ui, rules, parent)?;
    for parent_style in &parents {
        if !matches!(
            parent_style.flex_direction,
            taffy::FlexDirection::Column | taffy::FlexDirection::ColumnReverse
        ) || parent_style.flex_wrap != taffy::FlexWrap::NoWrap
            || parent_style.aspect_ratio.is_some()
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
        if !definite_width && !stretched && !fixed_inline_extent(style) {
            return Ok(false);
        }
    }
    // Every ancestor's style alternatives share the same ancestors above it.
    // Memoize that proof within this host query instead of expanding a product
    // of breakpoint alternatives over the entire depth of the tree.
    if let Some(valid) = memo.get(&parent) {
        return Ok(*valid);
    }
    for parent_style in &parents {
        if !column_inline_style(ui, rules, parent, parent_style, memo)? {
            memo.insert(parent, false);
            return Ok(false);
        }
    }
    memo.insert(parent, true);
    Ok(true)
}

fn validate_inline_extent(style: &Style) -> Result<(), Error> {
    use taffy::style::{ExpandedDimension as D, ExpandedLengthPercentage as L};
    let (D::Length(width), D::Length(min), D::Length(max)) = (
        style.size.width.expand(),
        style.min_size.width.expand(),
        style.max_size.width.expand(),
    ) else {
        return Err(Error::UnsafeContainerQuery);
    };
    if !width.is_finite()
        || width < 0.0
        || width != min
        || width != max
        || style.box_sizing != taffy::BoxSizing::BorderBox
        || !matches!(style.display, taffy::Display::Flex | taffy::Display::None)
        || !style.scrollbar_width.is_finite()
        || style.scrollbar_width < 0.0
    {
        return Err(Error::UnsafeContainerQuery);
    }
    let mut inset = if style.overflow.y == taffy::Overflow::Scroll {
        style.scrollbar_width
    } else {
        0.0
    };
    for value in [
        style.padding.left,
        style.padding.right,
        style.border.left,
        style.border.right,
    ] {
        let L::Length(value) = value.expand() else {
            return Err(Error::UnsafeContainerQuery);
        };
        if !value.is_finite() || value < 0.0 {
            return Err(Error::UnsafeContainerQuery);
        }
        inset += value;
    }
    if inset > width {
        return Err(Error::UnsafeContainerQuery);
    }
    Ok(())
}

#[cfg(test)]
mod atomic_tests;
#[cfg(test)]
mod tests;
