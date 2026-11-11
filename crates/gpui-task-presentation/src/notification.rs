//! Optional bridge from task progress to `gpui-notifications`.
//!
//! The mirror owns no task state. It maps a task identity to a stable
//! `NotificationId`, submits/replaces one ongoing notification and translates
//! `PostOutcome` values into presentation receipts without failing business work.

use crate::{
    AcknowledgmentPrecision, PresentationSurfaceKind, SurfacePresentationReceipt, TaskActionButton,
    TaskPresentationId, TaskPresentationOperation, TaskProgressState, TerminalNotificationPolicy,
};
use gpui_notifications::{
    ActionId as NotificationActionId, NotificationAction, NotificationContent, NotificationId,
    NotificationProgress, NotificationService, NotificationSubmission, PostOutcome,
    PostRejectReason, SubmissionState, WithdrawOutcome,
};
use gpui_system::{ErrorCode, SurfaceId, SystemError};

/// FNV-1a with a caller-selected seed. Stable across processes and platforms.
fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    let mut hash = seed;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Map a task identity to a stable notification identity.
///
/// Task identities allow `:` while notification identities do not, so the
/// identifier is represented by two independent 64-bit hashes. The result is a
/// valid, stable `NotificationId` that never embeds user content.
pub fn notification_id_for_task(task: &TaskPresentationId) -> Result<NotificationId, SystemError> {
    let first = fnv1a(task.as_str().as_bytes(), 0xcbf2_9ce4_8422_2325);
    let second = fnv1a(task.as_str().as_bytes(), 0x9e37_79b9_7f4a_7c15);
    NotificationId::new(format!("task-{first:016x}{second:016x}"))
}

/// Map a task action identity to a stable notification action identity.
pub fn notification_action_id_for_task(
    task: &TaskPresentationId,
    action: &crate::TaskActionId,
) -> Result<NotificationActionId, SystemError> {
    let mut bytes = Vec::with_capacity(task.as_str().len() + action.as_str().len() + 1);
    bytes.extend_from_slice(task.as_str().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(action.as_str().as_bytes());
    let first = fnv1a(&bytes, 0xcbf2_9ce4_8422_2325);
    let second = fnv1a(&bytes, 0x9e37_79b9_7f4a_7c15);
    NotificationActionId::new(format!("act-{first:016x}{second:016x}"))
}

/// Translate a notification submission state into presentation precision.
fn posted_precision(
    submission: SubmissionState,
    omitted_actions: bool,
    omitted_progress: bool,
) -> AcknowledgmentPrecision {
    let mut notes = Vec::new();
    if omitted_progress {
        notes.push("the provider omitted the progress bar");
    }
    if omitted_actions {
        notes.push("the provider omitted action buttons");
    }
    if !notes.is_empty() {
        return AcknowledgmentPrecision::DowngradedApplied {
            note: notes.join("; "),
        };
    }
    match submission {
        SubmissionState::PostedToCenter => AcknowledgmentPrecision::AppliedConfirmed,
        SubmissionState::AcceptedByProvider => AcknowledgmentPrecision::AcceptedByHost,
    }
}

fn rejected_precision(reason: &PostRejectReason) -> AcknowledgmentPrecision {
    match reason {
        PostRejectReason::PermissionDenied { .. } => {
            AcknowledgmentPrecision::SkippedPermissionDenied
        }
        PostRejectReason::ChannelOrServerDisabled
        | PostRejectReason::UnavailableProvider
        | PostRejectReason::Unsupported(_) => AcknowledgmentPrecision::SkippedUnavailable,
        PostRejectReason::MissingApplicationIdentity => AcknowledgmentPrecision::Failed {
            code: ErrorCode::InvalidConfiguration,
        },
    }
}

fn receipt(
    surface: SurfaceId,
    generation: u64,
    kind: PresentationSurfaceKind,
    precision: AcknowledgmentPrecision,
) -> SurfacePresentationReceipt {
    SurfacePresentationReceipt::new(
        surface,
        generation,
        kind,
        TaskPresentationOperation::MirrorProgressNotification,
        precision,
    )
}

fn progress_for(state: &TaskProgressState) -> Option<NotificationProgress> {
    match state {
        TaskProgressState::Determinate {
            completed_units,
            total_units,
            per_mille,
        } => {
            if total_units.get() <= 1_000_000
                && *completed_units <= u32::MAX as u64
                && total_units.get() <= u32::MAX as u64
            {
                NotificationProgress::new(*completed_units as u32, total_units.get() as u32).ok()
            } else {
                NotificationProgress::new(*per_mille as u32, 1000).ok()
            }
        }
        TaskProgressState::Paused {
            per_mille: Some(per_mille),
        }
        | TaskProgressState::Error {
            per_mille: Some(per_mille),
            ..
        } => NotificationProgress::new(*per_mille as u32, 1000).ok(),
        _ => None,
    }
}

/// A plain-text body derived from the state without floating-point math.
pub fn notification_body_for(state: &TaskProgressState) -> String {
    match state {
        TaskProgressState::Determinate {
            completed_units,
            total_units,
            per_mille,
        } => format!(
            "{completed_units} of {} units ({}%)",
            total_units.get(),
            (*per_mille as u32) / 10
        ),
        TaskProgressState::Indeterminate => "Working".to_owned(),
        TaskProgressState::Paused { .. } => "Paused".to_owned(),
        TaskProgressState::Error {
            message: Some(message),
            ..
        } => message.clone(),
        TaskProgressState::Error { message: None, .. } => "Failed".to_owned(),
        TaskProgressState::Completed => "Completed".to_owned(),
        TaskProgressState::Cancelled => "Cancelled".to_owned(),
    }
}

/// Optional notification mirror used by [`crate::TaskPresentationService`].
#[derive(Clone)]
pub struct NotificationTaskMirror {
    service: NotificationService,
    terminal_policy: TerminalNotificationPolicy,
}

impl std::fmt::Debug for NotificationTaskMirror {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NotificationTaskMirror")
            .field("terminal_policy", &self.terminal_policy)
            .finish_non_exhaustive()
    }
}

impl NotificationTaskMirror {
    pub fn new(service: NotificationService) -> Self {
        Self {
            service,
            terminal_policy: TerminalNotificationPolicy::Withdraw,
        }
    }

    pub fn with_terminal_policy(mut self, policy: TerminalNotificationPolicy) -> Self {
        self.terminal_policy = policy;
        self
    }

    pub fn terminal_policy(&self) -> TerminalNotificationPolicy {
        self.terminal_policy
    }

    pub fn set_terminal_policy(&mut self, policy: TerminalNotificationPolicy) {
        self.terminal_policy = policy;
    }

    pub fn service(&self) -> &NotificationService {
        &self.service
    }

    /// Submit or replace one ongoing progress notification.
    ///
    /// Returns a receipt and whether a new notification was posted. A stale
    /// `replace` rejection falls back to `post` with the same stable identity.
    #[allow(clippy::too_many_arguments)]
    pub fn submit(
        &self,
        task: &TaskPresentationId,
        surface: SurfaceId,
        generation: u64,
        title: Option<&str>,
        body: Option<&str>,
        channel_id: Option<&str>,
        state: &TaskProgressState,
        actions: &[TaskActionButton],
        replace: bool,
    ) -> (SurfacePresentationReceipt, bool) {
        let notification_id = match notification_id_for_task(task) {
            Ok(id) => id,
            Err(error) => {
                return (
                    receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        AcknowledgmentPrecision::Failed { code: error.code },
                    )
                    .with_error(error),
                    false,
                );
            }
        };
        let title = title
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Task {}", task.as_str()));
        let body = body
            .map(str::to_owned)
            .unwrap_or_else(|| notification_body_for(state));
        let mut content = match NotificationContent::new(title, body) {
            Ok(content) => content,
            Err(error) => {
                return (
                    receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        AcknowledgmentPrecision::Failed { code: error.code },
                    )
                    .with_error(error),
                    false,
                );
            }
        };
        if let Some(channel) = channel_id {
            match content.with_channel_id(channel) {
                Ok(updated) => content = updated,
                Err(error) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error),
                        false,
                    );
                }
            }
        }
        let mut notification_actions = Vec::new();
        for action in actions
            .iter()
            .filter(|action| action.is_enabled_in(state))
            .take(8)
        {
            let action_id = match notification_action_id_for_task(task, &action.action_id) {
                Ok(id) => id,
                Err(error) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error),
                        false,
                    );
                }
            };
            match NotificationAction::new(action_id, action.label.as_str()) {
                Ok(action) => notification_actions.push(action),
                Err(error) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error),
                        false,
                    );
                }
            }
        }
        match content.with_actions(notification_actions) {
            Ok(updated) => content = updated,
            Err(error) => {
                return (
                    receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        AcknowledgmentPrecision::Failed { code: error.code },
                    )
                    .with_error(error),
                    false,
                );
            }
        }
        if let Some(progress) = progress_for(state) {
            content = content.with_progress(progress);
        }
        let submission = NotificationSubmission::new(notification_id, content);
        if replace {
            match self.service.replace(submission.clone()) {
                Ok(PostOutcome::Posted(receipt_outcome)) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            posted_precision(
                                receipt_outcome.submission,
                                receipt_outcome.omitted_actions,
                                receipt_outcome.omitted_progress,
                            ),
                        ),
                        true,
                    );
                }
                Ok(PostOutcome::Rejected(reason @ PostRejectReason::Unsupported(_))) => {
                    let _ = reason;
                    // The provider cannot replace by identity; posting the same
                    // stable identity is the compatible fallback.
                }
                Ok(PostOutcome::Rejected(reason)) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            rejected_precision(&reason),
                        )
                        .with_error(reason.into_error()),
                        false,
                    );
                }
                Err(error) => {
                    return (
                        receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error),
                        false,
                    );
                }
            }
        }
        match self.service.post(submission) {
            Ok(PostOutcome::Posted(receipt_outcome)) => (
                receipt(
                    surface,
                    generation,
                    PresentationSurfaceKind::NotificationMirror,
                    posted_precision(
                        receipt_outcome.submission,
                        receipt_outcome.omitted_actions,
                        receipt_outcome.omitted_progress,
                    ),
                ),
                true,
            ),
            Ok(PostOutcome::Rejected(reason)) => (
                receipt(
                    surface,
                    generation,
                    PresentationSurfaceKind::NotificationMirror,
                    rejected_precision(&reason),
                )
                .with_error(reason.clone().into_error()),
                false,
            ),
            Err(error) => (
                receipt(
                    surface,
                    generation,
                    PresentationSurfaceKind::NotificationMirror,
                    AcknowledgmentPrecision::Failed { code: error.code },
                )
                .with_error(error),
                false,
            ),
        }
    }

    /// Withdraw one ongoing notification.
    pub fn withdraw(
        &self,
        task: &TaskPresentationId,
        surface: SurfaceId,
        generation: u64,
    ) -> SurfacePresentationReceipt {
        let notification_id = match notification_id_for_task(task) {
            Ok(id) => id,
            Err(error) => {
                return receipt(
                    surface,
                    generation,
                    PresentationSurfaceKind::NotificationMirror,
                    AcknowledgmentPrecision::Failed { code: error.code },
                )
                .with_error(error);
            }
        };
        match self.service.withdraw(&notification_id) {
            Ok(WithdrawOutcome::Withdrawn) => receipt(
                surface,
                generation,
                PresentationSurfaceKind::NotificationMirror,
                AcknowledgmentPrecision::AppliedConfirmed,
            ),
            Ok(WithdrawOutcome::NotFoundOrAlreadyDismissed) => receipt(
                surface,
                generation,
                PresentationSurfaceKind::NotificationMirror,
                AcknowledgmentPrecision::AppliedConfirmed,
            ),
            Ok(WithdrawOutcome::Unsupported) => receipt(
                surface,
                generation,
                PresentationSurfaceKind::NotificationMirror,
                AcknowledgmentPrecision::SkippedUnavailable,
            ),
            Err(error) => receipt(
                surface,
                generation,
                PresentationSurfaceKind::NotificationMirror,
                AcknowledgmentPrecision::Failed { code: error.code },
            )
            .with_error(error),
        }
    }

    /// Terminal cleanup according to [`TerminalNotificationPolicy`].
    pub fn terminate(
        &self,
        task: &TaskPresentationId,
        surface: SurfaceId,
        generation: u64,
        state: &TaskProgressState,
    ) -> SurfacePresentationReceipt {
        match self.terminal_policy {
            TerminalNotificationPolicy::Withdraw => self.withdraw(task, surface, generation),
            TerminalNotificationPolicy::ReplaceWithSummary => {
                let notification_id = match notification_id_for_task(task) {
                    Ok(id) => id,
                    Err(error) => {
                        return receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error);
                    }
                };
                let title = match state {
                    TaskProgressState::Completed => "Task completed",
                    TaskProgressState::Cancelled => "Task cancelled",
                    _ => "Task finished",
                };
                let body = notification_body_for(state);
                let content = match NotificationContent::new(title, body) {
                    Ok(content) => content,
                    Err(error) => {
                        return receipt(
                            surface,
                            generation,
                            PresentationSurfaceKind::NotificationMirror,
                            AcknowledgmentPrecision::Failed { code: error.code },
                        )
                        .with_error(error);
                    }
                };
                let submission = NotificationSubmission::new(notification_id, content);
                match self.service.replace(submission.clone()) {
                    Ok(PostOutcome::Posted(receipt_outcome)) => receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        posted_precision(
                            receipt_outcome.submission,
                            receipt_outcome.omitted_actions,
                            receipt_outcome.omitted_progress,
                        ),
                    ),
                    Ok(PostOutcome::Rejected(PostRejectReason::Unsupported(_))) => {
                        match self.service.post(submission) {
                            Ok(PostOutcome::Posted(receipt_outcome)) => receipt(
                                surface,
                                generation,
                                PresentationSurfaceKind::NotificationMirror,
                                posted_precision(
                                    receipt_outcome.submission,
                                    receipt_outcome.omitted_actions,
                                    receipt_outcome.omitted_progress,
                                ),
                            ),
                            Ok(PostOutcome::Rejected(reason)) => receipt(
                                surface,
                                generation,
                                PresentationSurfaceKind::NotificationMirror,
                                rejected_precision(&reason),
                            )
                            .with_error(reason.into_error()),
                            Err(error) => receipt(
                                surface,
                                generation,
                                PresentationSurfaceKind::NotificationMirror,
                                AcknowledgmentPrecision::Failed { code: error.code },
                            )
                            .with_error(error),
                        }
                    }
                    Ok(PostOutcome::Rejected(reason)) => receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        rejected_precision(&reason),
                    )
                    .with_error(reason.into_error()),
                    Err(error) => receipt(
                        surface,
                        generation,
                        PresentationSurfaceKind::NotificationMirror,
                        AcknowledgmentPrecision::Failed { code: error.code },
                    )
                    .with_error(error),
                }
            }
        }
    }
}
