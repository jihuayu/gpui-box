//! Typed task-action dispatch outcomes and stale-click rejection reasons.

use crate::{TaskActionId, TaskPresentationId};
use gpui_menus::{CommandId, CommandPayload};
use gpui_system::{Request, SystemError};
use std::fmt;

/// Why a thumbbar or notification action click was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionRejectionReason {
    UnknownTask,
    TaskTerminal,
    UnknownAction,
    ActionDisabled,
    SurfaceMismatch,
    GenerationMismatch,
    NoCommandService,
    CommandNotRegistered,
}

impl ActionRejectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownTask => "unknown_task",
            Self::TaskTerminal => "task_terminal",
            Self::UnknownAction => "unknown_action",
            Self::ActionDisabled => "action_disabled",
            Self::SurfaceMismatch => "surface_mismatch",
            Self::GenerationMismatch => "generation_mismatch",
            Self::NoCommandService => "no_command_service",
            Self::CommandNotRegistered => "command_not_registered",
        }
    }
}

impl fmt::Display for ActionRejectionReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A typed rejection carrying a public-safe system error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskActionRejection {
    pub reason: ActionRejectionReason,
    pub error: SystemError,
}

impl TaskActionRejection {
    pub fn new(reason: ActionRejectionReason, error: SystemError) -> Self {
        Self { reason, error }
    }

    pub const fn code(&self) -> gpui_system::ErrorCode {
        self.error.code
    }
}

impl fmt::Display for TaskActionRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.reason, self.error)
    }
}

impl std::error::Error for TaskActionRejection {}

/// The result of dispatching one task action click.
///
/// `Invoked` carries the `gpui-menus` request future for the caller's runtime to
/// await. A rejection never invokes a command.
pub enum TaskActionDispatchOutcome {
    Invoked {
        task: TaskPresentationId,
        action_id: TaskActionId,
        command_id: CommandId,
        request: Request<CommandPayload>,
    },
    Rejected {
        task: Option<TaskPresentationId>,
        action_id: String,
        rejection: TaskActionRejection,
    },
}

impl core::fmt::Debug for TaskActionDispatchOutcome {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Invoked {
                task,
                action_id,
                command_id,
                ..
            } => formatter
                .debug_struct("TaskActionDispatchOutcome::Invoked")
                .field("task", task)
                .field("action_id", action_id)
                .field("command_id", command_id)
                .finish_non_exhaustive(),
            Self::Rejected {
                task,
                action_id,
                rejection,
            } => formatter
                .debug_struct("TaskActionDispatchOutcome::Rejected")
                .field("task", task)
                .field("action_id", action_id)
                .field("rejection", rejection)
                .finish(),
        }
    }
}

impl TaskActionDispatchOutcome {
    pub const fn is_invoked(&self) -> bool {
        matches!(self, Self::Invoked { .. })
    }

    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }

    pub fn request(&self) -> Option<&Request<CommandPayload>> {
        match self {
            Self::Invoked { request, .. } => Some(request),
            Self::Rejected { .. } => None,
        }
    }

    pub fn rejection(&self) -> Option<&TaskActionRejection> {
        match self {
            Self::Invoked { .. } => None,
            Self::Rejected { rejection, .. } => Some(rejection),
        }
    }
}

/// Compatibility alias.
pub type TaskActionOutcome = TaskActionDispatchOutcome;
