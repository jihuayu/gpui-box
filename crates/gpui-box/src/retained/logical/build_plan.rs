//! Transaction-scoped, once-only layout construction scheduling. No user code
//! runs during graph sorting or inside kernel intrinsic measurement callbacks.
use super::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LayoutBuildTaskId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutBuildKind {
    Constraint,
    Allocation,
    InlineConstraint,
    InlineAllocation,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutBuildInput {
    Full(ConstraintBuilderInput),
    Inline(InlineConstraintBuilderInput),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutBuildLimits {
    pub max_tasks: usize,
    /// Deduplicated explicit plus ancestry edges in each current plan graph.
    pub max_dependency_edges: usize,
}
impl Default for LayoutBuildLimits {
    fn default() -> Self {
        Self {
            max_tasks: 1024,
            max_dependency_edges: 16384,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutBuildStats {
    pub tasks_executed: usize,
    pub planning_previews: usize,
    pub peak_dependency_edges: usize,
    pub peak_pending: usize,
}

type BuildCallback<'tasks> = Box<
    dyn for<'a, 'draft, 'tree> FnOnce(
            LayoutBuildInput,
            &mut LogicalEdit<'a, 'draft, 'tree>,
            &mut LayoutBuildQueue<'_, 'tasks>,
        ) -> Result<(), Error>
        + 'tasks,
>;
struct Task<'tasks> {
    host: NodeId,
    kind: LayoutBuildKind,
    after: BTreeSet<LayoutBuildTaskId>,
    callback: Option<BuildCallback<'tasks>>,
}
/// Owns one transaction's pending callbacks. IDs are caller-chosen within this
/// plan only. Render ancestry supplies automatic parent-before-child edges;
/// after supplies sibling/source prerequisites that ancestry cannot infer.
/// Callbacks may enqueue newly created descendant hosts before returning.
pub struct LayoutBuildPlan<'tasks> {
    tasks: BTreeMap<LayoutBuildTaskId, Task<'tasks>>,
    hosts: BTreeSet<NodeId>,
    limits: LayoutBuildLimits,
    invalid: bool,
}
/// Append-only callback capability. Pending/completed tasks cannot be removed,
/// replaced or replayed by taking/replacing the executing plan.
pub struct LayoutBuildQueue<'queue, 'tasks> {
    plan: &'queue mut LayoutBuildPlan<'tasks>,
}
impl<'tasks> LayoutBuildQueue<'_, 'tasks> {
    pub fn add(
        &mut self,
        id: LayoutBuildTaskId,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutBuildTaskId],
        build: impl for<'a, 'draft, 'tree> FnOnce(
            LayoutBuildInput,
            &mut LogicalEdit<'a, 'draft, 'tree>,
            &mut LayoutBuildQueue<'_, 'tasks>,
        ) -> Result<(), Error>
        + 'tasks,
    ) -> Result<(), Error> {
        self.plan.add(id, host, kind, after, build)
    }
}
/// No callbacks escape the transaction. Only final host ranks/prerequisites
/// survive until ordinary complete layout validation checks the executed order.
pub(super) struct BuildObservation {
    tasks: BTreeMap<LayoutBuildTaskId, (NodeId, usize, BTreeSet<LayoutBuildTaskId>)>,
}
impl BuildObservation {
    fn new(plan: &LayoutBuildPlan<'_>, completed: &BTreeMap<LayoutBuildTaskId, usize>) -> Self {
        Self {
            tasks: plan
                .tasks
                .iter()
                .map(|(&id, task)| (id, (task.host, completed[&id], task.after.clone())))
                .collect(),
        }
    }
    pub(super) fn validate(&self, tree: &UiTree) -> Result<(), Error> {
        let hosts: BTreeMap<_, _> = self
            .tasks
            .values()
            .map(|(host, rank, _)| (*host, *rank))
            .collect();
        for (host, rank, after) in self.tasks.values() {
            if after.iter().any(|id| {
                self.tasks
                    .get(id)
                    .is_none_or(|(_, before, _)| before >= rank)
            }) {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            let mut parent = tree.node(*host)?.parent;
            while let Some(node) = parent {
                if hosts.get(&node).is_some_and(|before| before >= rank) {
                    return Err(Error::InvalidLayoutBuildPlan);
                }
                parent = tree.node(node)?.parent;
            }
        }
        Ok(())
    }
}
impl Default for LayoutBuildPlan<'_> {
    fn default() -> Self {
        Self::new()
    }
}
impl<'tasks> LayoutBuildPlan<'tasks> {
    pub fn new() -> Self {
        Self::with_limits(LayoutBuildLimits::default())
    }
    pub fn with_limits(limits: LayoutBuildLimits) -> Self {
        Self {
            tasks: BTreeMap::new(),
            hosts: BTreeSet::new(),
            limits,
            invalid: false,
        }
    }
    pub fn len(&self) -> usize {
        self.tasks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    pub fn add(
        &mut self,
        id: LayoutBuildTaskId,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutBuildTaskId],
        build: impl for<'a, 'draft, 'tree> FnOnce(
            LayoutBuildInput,
            &mut LogicalEdit<'a, 'draft, 'tree>,
            &mut LayoutBuildQueue<'_, 'tasks>,
        ) -> Result<(), Error>
        + 'tasks,
    ) -> Result<(), Error> {
        let result = self.insert(id, host, kind, after, Box::new(build));
        self.invalid |= result.is_err();
        result
    }
    fn insert(
        &mut self,
        id: LayoutBuildTaskId,
        host: NodeId,
        kind: LayoutBuildKind,
        after: &[LayoutBuildTaskId],
        callback: BuildCallback<'tasks>,
    ) -> Result<(), Error> {
        if self.invalid
            || self.tasks.contains_key(&id)
            || self.hosts.contains(&host)
            || after.contains(&id)
        {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        if self.tasks.len() >= self.limits.max_tasks
            || after.len() > self.limits.max_dependency_edges
        {
            return Err(Error::LayoutBuildPlanLimit);
        }
        self.hosts.insert(host);
        self.tasks.insert(
            id,
            Task {
                host,
                kind,
                after: after.iter().copied().collect(),
                callback: Some(callback),
            },
        );
        Ok(())
    }
    fn order(
        &self,
        tree: &UiTree,
        completed: &BTreeMap<LayoutBuildTaskId, usize>,
    ) -> Result<(Vec<LayoutBuildTaskId>, usize), Error> {
        if self.invalid {
            return Err(Error::InvalidLayoutBuildPlan);
        }
        let owners: BTreeMap<_, _> = self
            .tasks
            .iter()
            .map(|(&id, task)| (task.host, id))
            .collect();
        let mut dependencies = BTreeMap::new();
        let mut edge_count = 0usize;
        for (&id, task) in &self.tasks {
            tree.node(task.host)?;
            if task.after.iter().any(|id| !self.tasks.contains_key(id)) {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            let mut after = task.after.clone();
            let mut parent = tree.node(task.host)?.parent;
            while let Some(node) = parent {
                if let Some(&dependency) = owners.get(&node) {
                    after.insert(dependency);
                }
                parent = tree.node(node)?.parent;
            }
            edge_count = edge_count
                .checked_add(after.len())
                .ok_or(Error::LayoutBuildPlanLimit)?;
            if edge_count > self.limits.max_dependency_edges {
                return Err(Error::LayoutBuildPlanLimit);
            }
            // A newly added ancestor or a reparent cannot retroactively precede
            // a task whose callback already consumed its input.
            if let Some(rank) = completed.get(&id)
                && after.iter().any(|dependency| {
                    completed
                        .get(dependency)
                        .is_none_or(|before| before >= rank)
                })
            {
                return Err(Error::InvalidLayoutBuildPlan);
            }
            dependencies.insert(id, after);
        }
        let mut done: BTreeSet<_> = completed.keys().copied().collect();
        let mut order = vec![];
        while done.len() < self.tasks.len() {
            let ready = dependencies
                .iter()
                .find(|(id, after)| !done.contains(id) && after.iter().all(|id| done.contains(id)))
                .map(|(&id, _)| id);
            let Some(id) = ready else {
                return Err(Error::CyclicLayoutBuildPlan);
            };
            done.insert(id);
            order.push(id);
        }
        Ok((order, edge_count))
    }
}
impl LogicalEdit<'_, '_, '_> {
    pub(super) fn run_layout_build_plan(
        &mut self,
        mut plan: LayoutBuildPlan<'_>,
    ) -> Result<LayoutBuildStats, Error> {
        let mut stats = LayoutBuildStats::default();
        let mut completed = BTreeMap::new();
        loop {
            let preview = self.build_plan_preview()?;
            stats.planning_previews += 1;
            let (order, edges) = plan.order(&preview, &completed)?;
            drop(preview);
            stats.peak_dependency_edges = stats.peak_dependency_edges.max(edges);
            stats.peak_pending = stats.peak_pending.max(order.len());
            let Some(&id) = order.first() else {
                self.observe_layout_build_order(BuildObservation::new(&plan, &completed));
                return Ok(stats);
            };
            let task = plan.tasks.get_mut(&id).unwrap();
            let host = task.host;
            let kind = task.kind;
            let callback = task.callback.take().ok_or(Error::InvalidLayoutBuildPlan)?;
            let mut queue = LayoutBuildQueue { plan: &mut plan };
            match kind {
                LayoutBuildKind::Constraint => self.constraint_builder(host, |input, edit| {
                    callback(LayoutBuildInput::Full(input), edit, &mut queue)
                })?,
                LayoutBuildKind::Allocation => self.allocation_builder(host, |input, edit| {
                    callback(LayoutBuildInput::Full(input), edit, &mut queue)
                })?,
                LayoutBuildKind::InlineConstraint => self
                    .inline_constraint_builder(host, |input, edit| {
                        callback(LayoutBuildInput::Inline(input), edit, &mut queue)
                    })?,
                LayoutBuildKind::InlineAllocation => self
                    .inline_allocation_builder(host, |input, edit| {
                        callback(LayoutBuildInput::Inline(input), edit, &mut queue)
                    })?,
            }
            completed.insert(id, stats.tasks_executed);
            stats.tasks_executed += 1;
        }
    }
}
