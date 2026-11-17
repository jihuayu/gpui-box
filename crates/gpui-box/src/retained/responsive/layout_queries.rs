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
    // Width phases address children in layout order. A sibling style sink can
    // change order and therefore which enforced slot reaches this host.
    let mut cursor = tree.node(host)?.parent;
    while let Some(parent) = cursor {
        let node = tree.node(parent)?;
        if node.parent_layout.is_some() && tree.node(target)?.parent == Some(parent) {
            return Ok(true);
        }
        cursor = node.parent;
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
        && matches!(
            style.display,
            taffy::Display::Flex | taffy::Display::ConstraintBox { .. } | taffy::Display::None
        )
}

fn validate_query_host(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    host: NodeId,
    style: &Style,
) -> Result<(), Error> {
    // Custom parent hard constraints override ordinary fixed-width limits.
    // Require the shared proof across independent width-allocation phases;
    // never trust containment/fixed CSS size alone below an arbitrary allocator.
    let mut ancestor = Some(host);
    while let Some(node) = ancestor {
        if ui.tree().node(node)?.parent_layout.is_some() {
            return if (column_inline_chain(ui, rules, host, style)?
                && (validate_inline_extent(style).is_ok()
                    || style
                        .contain
                        .contains(taffy::Contain::INLINE_SIZE | taffy::Contain::LAYOUT)))
                || inline_chain(ui, rules, host, style, true)?
            {
                Ok(())
            } else {
                Err(Error::UnsafeContainerQuery)
            };
        }
        ancestor = ui.tree().node(node)?.parent;
    }
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
        // Ordinary content-sized hosts can query descendants when the complete
        // incoming chain fixes inline allocation independently of their height.
        // Unlike a local used-width shortcut, this reaches the external root.
        return inline_chain(ui, rules, host, style, true);
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
    inline_chain(ui, rules, node, style, false)
}

fn inline_chain(
    ui: &impl QueryContext,
    rules: &[Rule<'_>],
    node: NodeId,
    style: &Style,
    incoming: bool,
) -> Result<bool, Error> {
    struct Context<'a, 'b, C> {
        ui: &'a C,
        rules: &'a [Rule<'b>],
    }
    impl<C: QueryContext> layout_dependencies::InlineContext for Context<'_, '_, C> {
        fn tree(&self) -> &UiTree {
            self.ui.tree()
        }
        fn styles(&self, node: NodeId) -> Result<Vec<Style>, Error> {
            possible_styles(self.ui, self.rules, node)
        }
    }
    let context = Context { ui, rules };
    if incoming {
        layout_dependencies::column_inline_allocation_chain(&context, node, style)
    } else {
        layout_dependencies::column_inline_chain(&context, node, style)
    }
}

/// Read actual content width without manufacturing an anchor from a previous
/// shrink-wrapped result. Fixed extents and explicit containment keep their
/// prior contracts; ordinary complete-chain hosts require live external input.
pub(in crate::retained) fn observed_inline_size(
    tree: &UiTree,
    node: NodeId,
    constraints: LayoutConstraints,
) -> Result<f32, Error> {
    let style = &tree.node(node)?.style;
    if validate_inline_extent(style).is_err()
        && !style
            .contain
            .contains(taffy::Contain::INLINE_SIZE | taffy::Contain::LAYOUT)
        && !layout_dependencies::has_external_inline_anchor(tree, constraints)?
    {
        return Err(Error::UnsafeContainerQuery);
    }
    let layout = tree.layout.layout(tree.node(node)?.layout)?;
    let width = (layout.size.width
        - layout.padding.left
        - layout.padding.right
        - layout.border.left
        - layout.border.right
        - layout.scrollbar_size.width)
        .max(0.0);
    if width.is_finite() {
        Ok(width)
    } else {
        Err(Error::InvalidFrame)
    }
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
        || !matches!(
            style.display,
            taffy::Display::Flex | taffy::Display::ConstraintBox { .. } | taffy::Display::None
        )
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
mod ordinary_tests;
#[cfg(test)]
mod tests;
