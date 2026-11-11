//! Deterministic aggregation of concurrent task presentations.
//!
//! The winner is the highest priority, then the highest state severity, then the
//! earliest registration sequence. Under [`AggregateMode::WeightedDeterminateSum`]
//! only the determinate contribution is summed; `Error` and `Paused` continue to
//! override determinate progress immediately.

use crate::{
    AggregateMode, AttentionRequest, BadgeValue, TaskActionButton, TaskPresentationId,
    TaskPresentationOperation, TaskPriority, TaskProgressState, TaskStateKind,
};
use gpui_system::SurfaceId;
use std::num::NonZeroU64;

/// One active task's aggregation input.
#[derive(Debug, Clone, Copy)]
pub struct AggregateInput<'a> {
    pub id: &'a TaskPresentationId,
    pub sequence: u64,
    pub priority: TaskPriority,
    pub state: &'a TaskProgressState,
}

/// One attached live surface's aggregated presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatedSurfacePresentation {
    pub surface: SurfaceId,
    pub generation: u64,
    pub lead_task: Option<TaskPresentationId>,
    pub lead_priority: Option<TaskPriority>,
    pub progress: Option<TaskProgressState>,
    pub active_task_count: usize,
    pub determinate_task_count: usize,
    pub action_buttons: Vec<TaskActionButton>,
    pub badge: Option<BadgeValue>,
    pub attention: Vec<AttentionRequest>,
}

impl AggregatedSurfacePresentation {
    pub fn empty(surface: SurfaceId, generation: u64) -> Self {
        Self {
            surface,
            generation,
            lead_task: None,
            lead_priority: None,
            progress: None,
            active_task_count: 0,
            determinate_task_count: 0,
            action_buttons: Vec::new(),
            badge: None,
            attention: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.progress.is_none()
            && self.badge.is_none()
            && self.attention.is_empty()
            && self.action_buttons.is_empty()
    }

    /// The discrete part of the presentation. Continuous determinate ticks keep
    /// the same signature and may be coalesced.
    pub fn discrete_signature(&self) -> SurfaceDiscreteSignature {
        SurfaceDiscreteSignature {
            lead_task: self.lead_task.clone(),
            lead_priority: self.lead_priority,
            state_kind: self.progress.as_ref().map(TaskProgressState::kind),
            badge: self.badge.clone(),
            attention: self.attention.len(),
            actions: self.action_buttons.len(),
        }
    }
}

/// A comparable signature used by the throttler to classify a transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceDiscreteSignature {
    pub lead_task: Option<TaskPresentationId>,
    pub lead_priority: Option<TaskPriority>,
    pub state_kind: Option<TaskStateKind>,
    pub badge: Option<BadgeValue>,
    pub attention: usize,
    pub actions: usize,
}

/// The application-wide aggregated presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregatedAppPresentation {
    pub lead_task: Option<TaskPresentationId>,
    pub lead_priority: Option<TaskPriority>,
    pub progress: Option<TaskProgressState>,
    pub active_task_count: usize,
    pub determinate_task_count: usize,
    pub action_buttons: Vec<TaskActionButton>,
    pub badge: Option<BadgeValue>,
    pub surfaces: Vec<AggregatedSurfacePresentation>,
}

impl AggregatedAppPresentation {
    pub fn empty() -> Self {
        Self {
            lead_task: None,
            lead_priority: None,
            progress: None,
            active_task_count: 0,
            determinate_task_count: 0,
            action_buttons: Vec::new(),
            badge: None,
            surfaces: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.progress.is_none()
            && self.badge.is_none()
            && self.action_buttons.is_empty()
            && self.active_task_count == 0
    }
}

fn severity(state: &TaskProgressState) -> u8 {
    state.severity_rank().unwrap_or(0)
}

/// Pick the winning active task. Terminal states are ignored.
pub fn select_lead<'a>(
    inputs: impl IntoIterator<Item = AggregateInput<'a>>,
) -> Option<AggregateInput<'a>> {
    let mut winner: Option<AggregateInput<'a>> = None;
    for input in inputs {
        if input.state.is_terminal() {
            continue;
        }
        let replace = match winner {
            None => true,
            Some(current) => {
                input.priority > current.priority
                    || (input.priority == current.priority
                        && severity(input.state) > severity(current.state))
                    || (input.priority == current.priority
                        && severity(input.state) == severity(current.state)
                        && input.sequence < current.sequence)
            }
        };
        if replace {
            winner = Some(input);
        }
    }
    winner
}

/// Aggregate progress for one surface or the application.
pub fn aggregate_progress<'a>(
    inputs: impl IntoIterator<Item = AggregateInput<'a>> + Clone,
    mode: AggregateMode,
) -> Option<TaskProgressState> {
    let lead = select_lead(inputs.clone())?;
    match mode {
        AggregateMode::HighestPriorityLead => Some(lead.state.clone()),
        AggregateMode::WeightedDeterminateSum => {
            if lead.state.kind() != TaskStateKind::Determinate {
                return Some(lead.state.clone());
            }
            let determinate = inputs
                .into_iter()
                .filter(|input| {
                    !input.state.is_terminal()
                        && matches!(input.state, TaskProgressState::Determinate { .. })
                })
                .collect::<Vec<_>>();
            Some(weighted_determinate(&determinate))
        }
    }
}

fn weighted_determinate(inputs: &[AggregateInput<'_>]) -> TaskProgressState {
    let mut completed = 0u128;
    let mut total = 0u128;
    for input in inputs {
        if let TaskProgressState::Determinate {
            completed_units,
            total_units,
            ..
        } = input.state
        {
            completed = completed.saturating_add(*completed_units as u128);
            total = total.saturating_add(total_units.get() as u128);
        }
    }
    if total == 0 {
        return TaskProgressState::Indeterminate;
    }
    let per_mille = ((completed.saturating_mul(1000)) / total).min(1000) as u16;
    let total_units = NonZeroU64::new(total.min(u64::MAX as u128) as u64)
        .expect("a non-zero aggregate total remains non-zero after clamping");
    let completed_units = completed.min(total_units.get() as u128) as u64;
    TaskProgressState::Determinate {
        completed_units,
        total_units,
        per_mille,
    }
}

/// Aggregate the action buttons contributed by every active task, ordered by
/// lead priority and then stable registration sequence.
pub fn aggregate_actions<'a>(
    inputs: impl IntoIterator<Item = (&'a AggregateInput<'a>, &'a [TaskActionButton])>,
) -> Vec<TaskActionButton> {
    let mut ordered = inputs
        .into_iter()
        .filter(|(input, _)| !input.state.is_terminal())
        .collect::<Vec<_>>();
    ordered.sort_by(|(left, _), (right, _)| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.sequence.cmp(&right.sequence))
    });
    let mut actions = Vec::new();
    for (input, buttons) in ordered {
        for button in buttons {
            if !button.is_enabled_in(input.state) {
                continue;
            }
            if !actions
                .iter()
                .any(|existing: &TaskActionButton| existing.action_id == button.action_id)
            {
                actions.push(button.clone());
            }
        }
    }
    actions
}

/// The operation that a task state maps to for capability inspection.
pub const fn progress_operation() -> TaskPresentationOperation {
    TaskPresentationOperation::PresentTaskProgress
}
