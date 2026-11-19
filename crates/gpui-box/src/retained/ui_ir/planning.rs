//! Pure-style invalidation only. Normal graph, resource and geometry validation
//! remain authoritative, including when no style expression needs evaluation.
use super::style::StylePlan;

use super::*;

/// Work performed by this publication's source/style planner, excluding graph
/// payload validation, candidates, layout, extraction, raster and GPU work. A replay
/// performs none. Counts do not promise a wall-time or total-allocation bound.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiPlanningStats {
    pub topology_validations: usize,
    pub nodes_considered: usize,
    pub style_plans_built: usize,
    pub styles_evaluated: usize,
    pub effective_records_reused: usize,
}

#[derive(Clone, Default)]
pub(super) struct StyleCache {
    plans: BTreeMap<UiNodeId, Arc<StylePlan>>,
    /// Width, direction, reduced-motion, touch, pointer. Immutable membership
    /// snapshots share until a definition is replaced/created/removed.
    environment_users: [Arc<BTreeSet<UiNodeId>>; 5],
}
impl StyleCache {
    pub(super) fn initial(
        nodes: &NodeMap,
        environment: &LayoutEnvironment,
        limits: UiLimits,
    ) -> Result<(Self, NodeMap, UiPlanningStats), UiError> {
        let mut cache = Self::default();
        let mut resolved = NodeMap::new();
        let mut stats = UiPlanningStats::default();
        for (&id, node) in nodes {
            stats.nodes_considered += 1;
            cache.replace_plan(id, &node.styles, &mut stats);
            let effective = cache.evaluate(node, environment, limits, &mut stats)?;
            resolved.insert(id, effective);
        }
        Ok((cache, resolved, stats))
    }

    pub(super) fn update(
        &mut self,
        previous: (&NodeMap, &NodeMap, &LayoutEnvironment),
        nodes: &NodeMap,
        environment: &LayoutEnvironment,
        mut touched: BTreeSet<UiNodeId>,
        limits: UiLimits,
    ) -> Result<(NodeMap, UiPlanningStats), UiError> {
        environment.validate()?;
        let (authored, effective, before_environment) = previous;
        let mut stats = UiPlanningStats::default();
        let mut edits = BTreeMap::new();
        let changed = [
            before_environment.layout_viewport[0] != environment.layout_viewport[0],
            before_environment.direction != environment.direction,
            before_environment.reduced_motion != environment.reduced_motion,
            before_environment.touch != environment.touch,
            before_environment.pointer != environment.pointer,
        ];
        let mut environment_dirty = BTreeSet::new();
        for (index, changed) in changed.into_iter().enumerate() {
            if changed {
                environment_dirty.extend(self.environment_users[index].iter().copied());
            }
        }
        touched.extend(environment_dirty.iter().copied());
        for id in touched {
            stats.nodes_considered += 1;
            let Some(node) = nodes.get(&id) else {
                self.remove_plan(id);
                edits.insert(id, None);
                continue;
            };
            let old = authored.get(&id);
            let style_changed = old.is_none_or(|old| old.styles != node.styles);
            if style_changed {
                self.replace_plan(id, &node.styles, &mut stats);
            }
            if old.is_some_and(|old| Arc::ptr_eq(old, node) || old.as_ref() == node.as_ref())
                && !environment_dirty.contains(&id)
            {
                stats.effective_records_reused += 1;
                continue;
            }
            let needs_evaluation = old.is_none_or(|old| {
                style_changed
                    || old.layout != node.layout
                    || old.properties != node.properties
                    || self
                        .plans
                        .get(&id)
                        .is_some_and(|plan| plan.token_inputs_changed(&old.tokens, &node.tokens))
                    || self.plans.get(&id).is_some_and(|plan| {
                        plan.dependencies()
                            .state_changed(old.style_state, node.style_state)
                    })
            }) || environment_dirty.contains(&id);
            let next = if needs_evaluation {
                self.evaluate(node, environment, limits, &mut stats)?
            } else {
                // Content/topology/unread state still belongs to the effective
                // record, but must not reevaluate unaffected layout/properties.
                let mut next = node.as_ref().clone();
                next.layout = effective[&id].layout.clone();
                next.properties = effective[&id].properties.clone();
                next.styles.clear();
                Arc::new(next)
            };
            if effective
                .get(&id)
                .is_some_and(|old| old.as_ref() == next.as_ref())
            {
                stats.effective_records_reused += 1;
            } else {
                edits.insert(id, Some(next));
            }
        }
        Ok((apply_effective_edits(effective, edits), stats))
    }

    fn remove_plan(&mut self, id: UiNodeId) {
        if let Some(old) = self.plans.remove(&id) {
            for (index, used) in old
                .dependencies()
                .environment_inputs()
                .into_iter()
                .enumerate()
            {
                if used {
                    Arc::make_mut(&mut self.environment_users[index]).remove(&id);
                }
            }
        }
    }
    fn replace_plan(&mut self, id: UiNodeId, styles: &[StyleRef], stats: &mut UiPlanningStats) {
        self.remove_plan(id);
        if styles.is_empty() {
            return;
        }
        let plan = Arc::new(StylePlan::new(styles));
        stats.style_plans_built += 1;
        for (index, used) in plan
            .dependencies()
            .environment_inputs()
            .into_iter()
            .enumerate()
        {
            if used {
                Arc::make_mut(&mut self.environment_users[index]).insert(id);
            }
        }
        self.plans.insert(id, plan);
    }
    fn evaluate(
        &self,
        node: &Arc<UiNode>,
        environment: &LayoutEnvironment,
        limits: UiLimits,
        stats: &mut UiPlanningStats,
    ) -> Result<Arc<UiNode>, UiError> {
        let Some(plan) = self.plans.get(&node.id) else {
            return Ok(node.clone());
        };
        stats.styles_evaluated += 1;
        let output = plan
            .resolve_token_values(
                &node.layout,
                &node.properties,
                environment,
                node.style_state,
                &node.tokens,
            )
            .map_err(UiError::Style)?;
        let mut resolved = node.as_ref().clone();
        resolved.layout = output.layout;
        resolved.properties = output.properties;
        resolved.styles.clear();
        validate_payload_bounds(&resolved, limits)?;
        Ok(Arc::new(resolved))
    }
}

/// Sparse edits use ordinary logarithmic replacement. When their estimated
/// search work exceeds one full ordered scan, merge/rebuild sorted entries once.
/// This is a map-density decision, independent of UI primitive or frame rate.
pub(super) fn apply_effective_edits(
    previous: &NodeMap,
    edits: BTreeMap<UiNodeId, Option<Arc<UiNode>>>,
) -> NodeMap {
    let search_height = usize::BITS as usize - previous.len().max(1).leading_zeros() as usize;
    if edits.len().saturating_mul(search_height) <= previous.len() {
        let mut result = previous.clone();
        for (id, value) in edits {
            if let Some(value) = value {
                result.insert(id, value);
            } else {
                result.remove(&id);
            }
        }
        return result;
    }
    let mut entries = Vec::with_capacity(previous.len().saturating_add(edits.len()));
    let mut edits = edits.into_iter().peekable();
    for (&id, old) in previous {
        while edits.peek().is_some_and(|(key, _)| *key < id) {
            let (key, value) = edits.next().unwrap();
            if let Some(value) = value {
                entries.push((key, value));
            }
        }
        let value = if edits.peek().is_some_and(|(key, _)| *key == id) {
            Some(edits.next().unwrap().1)
        } else {
            None
        };
        match value {
            Some(Some(value)) => entries.push((id, value)),
            Some(None) => {}
            None => entries.push((id, old.clone())),
        }
    }
    entries.extend(edits.filter_map(|(id, value)| value.map(|value| (id, value))));
    entries.into_iter().collect()
}
