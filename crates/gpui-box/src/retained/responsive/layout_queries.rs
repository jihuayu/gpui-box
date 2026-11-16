//! Dependency-checked container queries over fixed inline extents.
use super::*;

type Rule<'a> = (
    Signal<LayoutStyle>,
    NodeId,
    LayoutQuery,
    &'a LayoutBreakpointStyles,
);

impl LayoutResponsiveBindings {
    pub(super) fn validate_layout_rules(
        &self,
        ui: &LogicalUi,
        candidate: Option<Rule<'_>>,
    ) -> Result<(), Error> {
        let tree = ui.reactive().tree();
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
            validate_inline_extent(&host_node.style)?;
            // A host may itself be controlled by an outer query/viewport rule.
            // Every branch and the source synchronized at bind time must keep
            // its inline size independent of descendant measurement.
            for (source, producer, _, styles) in &rules {
                if *producer == host {
                    validate_inline_extent(&ui.read(*source)?.to_taffy()?)?;
                    for style in &styles.styles {
                        validate_inline_extent(&style.to_taffy()?)?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn layout_query_stages(&self, tree: &UiTree) -> Result<Vec<Vec<usize>>, Error> {
        let queries = self
            .bindings
            .iter()
            .enumerate()
            .filter_map(|(i, binding)| {
                matches!(binding.query, LayoutQuery::ContainerInline(_)).then_some(i)
            })
            .collect::<Vec<_>>();
        let mut dependents = vec![vec![]; queries.len()];
        let mut indegrees = vec![0; queries.len()];
        for (consumer, &index) in queries.iter().enumerate() {
            let LayoutQuery::ContainerInline(host) = self.bindings[index].query else {
                unreachable!()
            };
            for (producer, &other) in queries.iter().enumerate() {
                if Self::is_ancestor(tree, self.bindings[other].target, host)? {
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
}

fn validate_inline_extent(style: &Style) -> Result<(), Error> {
    use taffy::style::{
        ExpandedDimension as D, ExpandedLengthPercentage as L, ExpandedLengthPercentageAuto as A,
    };
    let (D::Length(width), A::Length(min), A::Length(max)) = (
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
mod tests;
