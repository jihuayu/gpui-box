//! Explicit responsive stages inside the keyed edit publication boundary.
use super::*;
use layout_queries::QueryContext;
#[cfg(test)]
mod tests;

/// Private layout work performed by this explicit responsive stage. The final
/// edit FramePacket and constraint-builder previews are not included here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResponsiveEditStats {
    pub rules_examined: usize,
    pub changed_styles: usize,
    pub layout_runs: u64,
    pub layout_stats: LayoutStats,
}

struct Context<'a, 'edit, 'draft> {
    tree: &'a UiTree,
    edit: &'a reactive::ReactiveEdit<'edit, 'draft>,
}
impl QueryContext for Context<'_, '_, '_> {
    fn tree(&self) -> &UiTree {
        self.tree
    }
    fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error> {
        self.edit.read(source)
    }
    fn targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error> {
        Ok(self
            .edit
            .responsive_layout_targets(source)?
            .into_iter()
            .filter(|id| self.tree.node(*id).is_ok())
            .collect())
    }
    fn validate_writer(
        &self,
        source: Signal<LayoutStyle>,
        target: NodeId,
        binding: reactive::BindingId,
    ) -> Result<(), Error> {
        self.edit
            .validate_responsive_writer(source, target, binding)
    }
}

pub(in crate::retained) struct Observation {
    rules: LayoutResponsiveBindings,
    stages: Vec<Vec<usize>>,
    widths: Vec<(NodeId, f32)>,
    inline_dependencies: Vec<(NodeId, layout_dependencies::InlineDependencies)>,
    selected: Vec<(Signal<LayoutStyle>, LayoutStyle)>,
}

pub(in crate::retained) struct FinalObservation {
    observation: Observation,
    sources: Vec<(Signal<LayoutStyle>, LayoutStyle, Vec<NodeId>)>,
}
struct FinalContext<'a> {
    tree: &'a UiTree,
    sources: &'a [(Signal<LayoutStyle>, LayoutStyle, Vec<NodeId>)],
}
impl QueryContext for FinalContext<'_> {
    fn tree(&self) -> &UiTree {
        self.tree
    }
    fn style(&self, source: Signal<LayoutStyle>) -> Result<&LayoutStyle, Error> {
        self.sources
            .iter()
            .find(|(id, _, _)| *id == source)
            .map(|(_, style, _)| style)
            .ok_or(Error::InvalidSignal)
    }
    fn targets(&self, source: Signal<LayoutStyle>) -> Result<Vec<NodeId>, Error> {
        self.sources
            .iter()
            .find(|(id, _, _)| *id == source)
            .map(|(_, _, targets)| {
                targets
                    .iter()
                    .copied()
                    .filter(|id| self.tree.node(*id).is_ok())
                    .collect()
            })
            .ok_or(Error::InvalidSignal)
    }
    fn validate_writer(
        &self,
        _: Signal<LayoutStyle>,
        _: NodeId,
        _: reactive::BindingId,
    ) -> Result<(), Error> {
        // The final reactive draft checked exact writer ownership after all edits.
        Ok(())
    }
}

fn inline_size(tree: &UiTree, node: NodeId) -> Result<f32, Error> {
    layout_queries::observed_inline_size(
        tree,
        node,
        tree.layout_constraints.ok_or(Error::UnsafeContainerQuery)?,
    )
}

impl Observation {
    pub(in crate::retained) fn finish(
        self,
        edit: &reactive::ReactiveEdit<'_, '_>,
    ) -> Result<FinalObservation, Error> {
        let mut sources = vec![];
        for (source, selected) in &self.selected {
            if edit.read(*source)? != selected {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        for binding in &self.rules.bindings {
            edit.validate_responsive_writer(binding.source, binding.target, binding.binding)?;
            sources.push((
                binding.source,
                edit.read(binding.source)?.clone(),
                edit.responsive_layout_targets(binding.source)?,
            ));
        }
        Ok(FinalObservation {
            observation: self,
            sources,
        })
    }
}
impl FinalObservation {
    pub(in crate::retained) fn validate(&self, tree: &UiTree) -> Result<(), Error> {
        let context = FinalContext {
            tree,
            sources: &self.sources,
        };
        if self
            .observation
            .rules
            .validate_layout_rules(&context, None)?
            != self.observation.stages
        {
            return Err(Error::UnsafeContainerQuery);
        }
        for binding in &self.observation.rules.bindings {
            if *tree.node(binding.target)?.style != context.style(binding.source)?.to_taffy()? {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        for &(host, width) in &self.observation.widths {
            if inline_size(tree, host)? != width {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        for (host, dependencies) in &self.observation.inline_dependencies {
            if !dependencies.matches(tree, *host)? {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        Ok(())
    }
}

impl LayoutResponsiveBindings {
    pub(in crate::retained) fn validate_edit(
        &self,
        edit: &reactive::ReactiveEdit<'_, '_>,
        capabilities: BackendCapabilities,
        removed: &[NodeId],
        candidate: Option<(
            Signal<LayoutStyle>,
            NodeId,
            LayoutQuery,
            &LayoutBreakpointStyles,
        )>,
    ) -> Result<(), Error> {
        let preview = edit.preview(capabilities, removed)?;
        self.validate_layout_rules(
            &Context {
                tree: &preview,
                edit,
            },
            candidate,
        )?;
        Ok(())
    }

    pub(in crate::retained) fn resolve_edit(
        &self,
        edit: &mut reactive::ReactiveEdit<'_, '_>,
        environment: &LayoutEnvironment,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
        capabilities: BackendCapabilities,
        removed: &[NodeId],
    ) -> Result<(ResponsiveEditStats, Observation), Error> {
        environment.validate()?;
        let mut preview = edit.preview(capabilities, removed)?;
        let stages = self.validate_layout_rules(
            &Context {
                tree: &preview,
                edit,
            },
            None,
        )?;
        let originals = self
            .bindings
            .iter()
            .map(|b| Ok((b.source, edit.read(b.source)?.clone())))
            .collect::<Result<Vec<_>, Error>>()?;
        edit.responsive_values(
            self.bindings
                .iter()
                .filter(|b| b.query == LayoutQuery::ViewportInline)
                .map(|b| {
                    (
                        b.source,
                        b.styles
                            .select(environment.layout_viewport[0], environment.direction),
                    )
                })
                .collect(),
        )?;
        drop(preview);
        preview = edit.preview(capabilities, removed)?;
        if self.validate_layout_rules(
            &Context {
                tree: &preview,
                edit,
            },
            None,
        )? != stages
        {
            return Err(Error::UnsafeContainerQuery);
        }
        let mut stats = ResponsiveEditStats {
            rules_examined: self.len(),
            ..Default::default()
        };
        let mut widths = vec![];
        let mut inline_dependencies = vec![];
        for stage in &stages {
            preview.prepare_layout_environment(constraints, viewport)?;
            if preview.layout_dirty {
                let next = preview.compute_layout(constraints)?;
                stats.layout_runs += 1;
                stats.layout_stats.leaf_layouts += next.leaf_layouts;
                stats.layout_stats.measure_calls += next.measure_calls;
                stats.layout_stats.layout_writes += next.layout_writes;
                stats.layout_stats.changed_layout_writes += next.changed_layout_writes;
                stats.layout_stats.net_changed_nodes += next.net_changed_nodes;
                preview.layout_dirty = false;
            }
            let mut values = vec![];
            for &index in stage {
                let binding = &self.bindings[index];
                let LayoutQuery::ContainerInline(host) = binding.query else {
                    unreachable!()
                };
                let width = inline_size(&preview, host)?;
                widths.push((host, width));
                inline_dependencies.push((
                    host,
                    layout_dependencies::InlineDependencies::capture(&preview, host)?,
                ));
                values.push((
                    binding.source,
                    binding.styles.select(width, environment.direction),
                ));
            }
            edit.responsive_values(values)?;
            drop(preview);
            preview = edit.preview(capabilities, removed)?;
            if self.validate_layout_rules(
                &Context {
                    tree: &preview,
                    edit,
                },
                None,
            )? != stages
            {
                return Err(Error::UnsafeContainerQuery);
            }
        }
        let mut selected = vec![];
        for (source, original) in originals {
            let value = edit.read(source)?.clone();
            stats.changed_styles += usize::from(value != original);
            selected.push((source, value));
        }
        Ok((
            stats,
            Observation {
                rules: self.clone(),
                stages,
                widths,
                inline_dependencies,
                selected,
            },
        ))
    }
}
