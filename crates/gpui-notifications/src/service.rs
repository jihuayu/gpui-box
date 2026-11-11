use crate::{
    ActionPolicy, Capability, Configuration, ErrorCode, EventSender, EventSubscription,
    NotificationEvent, NotificationId, NotificationOperation, NotificationProvider,
    NotificationSubmission, PostOutcome, PostReceipt, PostRejectReason, ProgressPolicy,
    ReceiptPolicy, RecoveryGuidance, SubmissionState, SystemError, SystemServices, WithdrawOutcome,
};
use gpui_permissions::{
    PermissionKind, PermissionOperation, PermissionOutcome, PermissionRequest, PermissionService,
    PermissionSnapshot, PermissionStatus,
};
use gpui_system::{Availability, OperationContext, Request, Support, event_channel};
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
    thread::{self, ThreadId},
};

const EVENT_CAPACITY: usize = 32;

type Subscriber = (u64, Rc<Cell<bool>>, Rc<EventSender<NotificationEvent>>);

struct ServiceInner {
    provider: Rc<dyn NotificationProvider>,
    permissions: PermissionService,
    ui_thread: ThreadId,
    subscribers: RefCell<Vec<Subscriber>>,
    next_token: Cell<u64>,
}

/// UI-local notification service.
///
/// The handle is `Rc`-backed and cheap to clone. All submission operations check
/// the owning UI thread. Independent bounded subscribers receive the same events;
/// every subscriber list borrow is released before an event is sent, so a
/// synchronous [`std::task::Wake::wake`] callback may re-enter the service.
///
/// Authorization is delegated to [`PermissionService`] for
/// [`PermissionKind::Notifications`]. Missing identity, disabled channels and
/// unavailable providers remain distinct, actionable outcomes.
#[derive(Clone)]
pub struct NotificationService {
    inner: Rc<ServiceInner>,
}

impl fmt::Debug for NotificationService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotificationService")
            .field("ui_thread", &self.inner.ui_thread)
            .field("subscribers", &self.subscriber_count())
            .finish_non_exhaustive()
    }
}

enum Prepared {
    Ready {
        submission: NotificationSubmission,
        omitted_actions: bool,
        omitted_progress: bool,
        receipt_supported: bool,
    },
    Rejected {
        notification_id: NotificationId,
        reason: PostRejectReason,
    },
}

impl NotificationService {
    pub fn new(provider: Rc<dyn NotificationProvider>, permissions: PermissionService) -> Self {
        Self {
            inner: Rc::new(ServiceInner {
                provider,
                permissions,
                ui_thread: thread::current().id(),
                subscribers: RefCell::new(Vec::new()),
                next_token: Cell::new(0),
            }),
        }
    }

    pub fn from_provider(
        provider: impl NotificationProvider + 'static,
        permissions: PermissionService,
    ) -> Self {
        Self::new(Rc::new(provider), permissions)
    }

    pub fn provider(&self) -> &Rc<dyn NotificationProvider> {
        &self.inner.provider
    }

    pub fn permissions(&self) -> &PermissionService {
        &self.inner.permissions
    }

    /// Register this concrete wrapper in `SystemServices`. Install overrides
    /// before starting operations.
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn is_ui_thread(&self) -> bool {
        self.inner.ui_thread == thread::current().id()
    }

    pub fn require_ui_thread(&self) -> Result<(), SystemError> {
        if self.is_ui_thread() {
            Ok(())
        } else {
            Err(SystemError::new(
                ErrorCode::ContextExpired,
                "Notification service requires its owning UI thread",
            ))
        }
    }

    /// A capability snapshot for one operation. Authorization operations reflect
    /// the installed permission provider rather than inventing a second prompt
    /// surface.
    pub fn capability(
        &self,
        operation: NotificationOperation,
    ) -> Capability<NotificationOperation> {
        if operation.is_authorization() {
            let permission_operation = match operation {
                NotificationOperation::QueryAuthorization => {
                    PermissionOperation::Query(PermissionKind::Notifications)
                }
                NotificationOperation::RequestAuthorization => {
                    PermissionOperation::Request(PermissionKind::Notifications)
                }
                _ => unreachable!("only authorization operations take this path"),
            };
            let capability = self.inner.permissions.capability(permission_operation);
            return Capability {
                operation,
                support: capability.support,
                availability: capability.availability,
                configuration: capability.configuration,
                permission: capability.permission,
                evidence: capability.evidence,
                constraints: capability.constraints,
                observed_revision: capability.observed_revision,
            };
        }
        self.inner.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<NotificationOperation>> {
        NotificationOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Fresh, side-effect-free authorization probe. Use this for a synchronous
    /// check before posting; no native prompt is shown.
    pub fn authorization_status(&self) -> Result<PermissionSnapshot, SystemError> {
        self.require_ui_thread()?;
        self.inner
            .permissions
            .provider()
            .query(PermissionKind::Notifications)
    }

    /// Delegate an authorization query to the shared permission service.
    pub fn query_authorization(&self, context: OperationContext) -> Request<PermissionSnapshot> {
        self.inner
            .permissions
            .query(PermissionKind::Notifications, context)
    }

    /// Delegate a contextual authorization request to the shared permission
    /// service. Only that service serializes native prompts.
    pub fn request_authorization(&self, context: OperationContext) -> Request<PermissionOutcome> {
        self.inner.permissions.request_permission(
            PermissionRequest::new(PermissionKind::Notifications),
            context,
        )
    }

    /// Create an independent bounded observation stream.
    ///
    /// Each UI entity or view may subscribe to the shared service. A stream's
    /// cleanup marks its slot dead; dead slots are pruned on the next
    /// subscription, emission or count.
    pub fn subscribe(&self) -> EventSubscription<NotificationEvent> {
        let token = self.inner.next_token.get();
        self.inner.next_token.set(token.wrapping_add(1));
        let active = Rc::new(Cell::new(true));
        let cleanup_active = Rc::clone(&active);
        let weak = Rc::downgrade(&self.inner);
        let (sender, subscription) = event_channel(EVENT_CAPACITY, move || {
            cleanup_active.set(false);
            if let Some(inner) = weak.upgrade() {
                inner
                    .subscribers
                    .borrow_mut()
                    .retain(|(id, active, _)| *id != token && active.get());
            }
        });
        self.inner
            .subscribers
            .borrow_mut()
            .push((token, active, Rc::new(sender)));
        subscription
    }

    pub fn subscriber_count(&self) -> usize {
        let mut subscribers = self.inner.subscribers.borrow_mut();
        subscribers.retain(|(_, active, _)| active.get());
        subscribers.len()
    }

    /// Submit a notification immediately.
    pub fn post(&self, submission: NotificationSubmission) -> Result<PostOutcome, SystemError> {
        self.submit(submission, false)
    }

    /// Replace the notification with the same stable identity where supported.
    pub fn replace(&self, submission: NotificationSubmission) -> Result<PostOutcome, SystemError> {
        self.submit(submission, true)
    }

    /// Withdraw a submitted notification where supported.
    pub fn withdraw(
        &self,
        notification_id: &NotificationId,
    ) -> Result<WithdrawOutcome, SystemError> {
        self.require_ui_thread()?;
        NotificationId::new(notification_id.as_str())?;
        if capability_problem(
            &self
                .inner
                .provider
                .capability(NotificationOperation::WithdrawById),
        )
        .is_some()
        {
            return Ok(WithdrawOutcome::Unsupported);
        }
        let outcome = match self.inner.provider.withdraw(notification_id) {
            Ok(outcome) => outcome,
            Err(PostRejectReason::Unsupported(_)) => WithdrawOutcome::Unsupported,
            Err(reason) => return Err(reason.into_error()),
        };
        if outcome == WithdrawOutcome::Withdrawn {
            self.publish(NotificationEvent::Withdrawn {
                notification_id: notification_id.clone(),
            });
        }
        let _ = self.pump_observations();
        Ok(outcome)
    }

    /// Drain provider observations and publish them. This is public so hosts that
    /// receive out-of-band display or dismissal callbacks can forward them without
    /// waiting for another submission.
    pub fn pump_observations(&self) -> Result<usize, SystemError> {
        self.require_ui_thread()?;
        let observations = self.inner.provider.take_observations();
        let count = observations.len();
        for observation in observations {
            self.publish(observation.into());
        }
        Ok(count)
    }

    /// Record a provider observation that the notification was displayed.
    pub fn record_displayed(
        &self,
        notification_id: &NotificationId,
        provider_id: Option<crate::ProviderNotificationId>,
    ) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::ObservedDisplayed {
            notification_id: notification_id.clone(),
            provider_id,
        });
        Ok(())
    }

    /// Record a user dismissal without activation.
    pub fn record_dismissed(&self, notification_id: &NotificationId) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::DismissedByUser {
            notification_id: notification_id.clone(),
        });
        Ok(())
    }

    /// Record a user tap or action-button press.
    pub fn record_activated(
        &self,
        notification_id: &NotificationId,
        action_id: Option<crate::ActionId>,
    ) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::Activated {
            notification_id: notification_id.clone(),
            action_id,
        });
        Ok(())
    }

    /// Record a submission that was suppressed before reaching the provider.
    pub fn record_suppressed(
        &self,
        notification_id: &NotificationId,
        reason: &PostRejectReason,
    ) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::Suppressed {
            notification_id: notification_id.clone(),
            reason: reason.suppression_reason(),
        });
        Ok(())
    }

    /// Publish a submission receipt produced by a direct host bridge. This does
    /// not perform a second submission.
    pub fn record_submitted(&self, receipt: &PostReceipt) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::Submitted {
            notification_id: receipt.notification_id.clone(),
            submission: receipt.submission,
            replaced_existing: receipt.replaced_existing,
        });
        Ok(())
    }

    /// Publish a withdrawal performed by a direct host bridge.
    pub fn record_withdrawn(&self, notification_id: &NotificationId) -> Result<(), SystemError> {
        self.require_ui_thread()?;
        self.publish(NotificationEvent::Withdrawn {
            notification_id: notification_id.clone(),
        });
        Ok(())
    }

    fn submit(
        &self,
        submission: NotificationSubmission,
        replace: bool,
    ) -> Result<PostOutcome, SystemError> {
        self.require_ui_thread()?;
        submission.validate()?;
        let prepared = self.prepare(submission, replace)?;
        let (submission, omitted_actions, omitted_progress, receipt_supported) = match prepared {
            Prepared::Ready {
                submission,
                omitted_actions,
                omitted_progress,
                receipt_supported,
            } => (
                submission,
                omitted_actions,
                omitted_progress,
                receipt_supported,
            ),
            Prepared::Rejected {
                notification_id,
                reason,
            } => {
                self.publish(NotificationEvent::Suppressed {
                    notification_id,
                    reason: reason.suppression_reason(),
                });
                return Ok(PostOutcome::Rejected(reason));
            }
        };
        let notification_id = submission.notification_id.clone();
        let result = if replace {
            self.inner.provider.replace(&submission)
        } else {
            self.inner.provider.post(&submission)
        };
        let outcome = match result {
            Ok(mut receipt) => {
                if receipt.notification_id != submission.notification_id {
                    return Err(SystemError::new(
                        ErrorCode::IntegrityFailure,
                        "Notification provider returned a mismatched receipt",
                    ));
                }
                receipt.omitted_actions |= omitted_actions;
                receipt.omitted_progress |= omitted_progress;
                if !receipt_supported {
                    receipt.provider_id = None;
                    receipt.submission = SubmissionState::AcceptedByProvider;
                }
                self.publish(NotificationEvent::Submitted {
                    notification_id: notification_id.clone(),
                    submission: receipt.submission,
                    replaced_existing: receipt.replaced_existing,
                });
                PostOutcome::Posted(receipt)
            }
            Err(reason) => {
                self.publish(NotificationEvent::Suppressed {
                    notification_id,
                    reason: reason.suppression_reason(),
                });
                PostOutcome::Rejected(reason)
            }
        };
        let _ = self.pump_observations();
        Ok(outcome)
    }

    fn prepare(
        &self,
        submission: NotificationSubmission,
        replace: bool,
    ) -> Result<Prepared, SystemError> {
        let provider = &self.inner.provider;
        let notification_id = submission.notification_id.clone();
        if let Some(reason) =
            capability_problem(&provider.capability(NotificationOperation::PostImmediate))
        {
            return Ok(Prepared::Rejected {
                notification_id,
                reason,
            });
        }
        if replace
            && let Some(reason) =
                capability_problem(&provider.capability(NotificationOperation::ReplaceById))
        {
            return Ok(Prepared::Rejected {
                notification_id,
                reason,
            });
        }
        if provider.application_identity() == crate::ApplicationIdentity::Missing {
            return Ok(Prepared::Rejected {
                notification_id,
                reason: PostRejectReason::MissingApplicationIdentity,
            });
        }
        if !provider.channel_enabled(submission.content.channel_id()) {
            return Ok(Prepared::Rejected {
                notification_id,
                reason: PostRejectReason::ChannelOrServerDisabled,
            });
        }
        let snapshot = match self
            .inner
            .permissions
            .provider()
            .query(PermissionKind::Notifications)
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Ok(Prepared::Rejected {
                    notification_id,
                    reason: permission_rejection(error),
                });
            }
        };
        if !snapshot.status.allows_use() {
            let reason = permission_status_rejection(snapshot.status);
            return Ok(Prepared::Rejected {
                notification_id,
                reason,
            });
        }

        let mut normalized = submission.clone();
        let mut omitted_actions = false;
        if !normalized.content.actions().is_empty()
            && let Some(reason) =
                capability_problem(&provider.capability(NotificationOperation::ActionButtons))
        {
            match submission.action_policy {
                ActionPolicy::RequireSupported => {
                    return Ok(Prepared::Rejected {
                        notification_id,
                        reason,
                    });
                }
                ActionPolicy::OmitIfUnsupported => {
                    normalized.content = normalized.content.without_actions();
                    omitted_actions = true;
                }
            }
        }
        let mut omitted_progress = false;
        if normalized.content.progress().is_some()
            && let Some(reason) =
                capability_problem(&provider.capability(NotificationOperation::ProgressUpdates))
        {
            match submission.progress_policy {
                ProgressPolicy::RequireSupported => {
                    return Ok(Prepared::Rejected {
                        notification_id,
                        reason,
                    });
                }
                ProgressPolicy::OmitIfUnsupported => {
                    normalized.content = normalized.content.without_progress();
                    omitted_progress = true;
                }
            }
        }
        let receipt_supported = match capability_problem(
            &provider.capability(NotificationOperation::DeliveryReceipt),
        ) {
            None => true,
            Some(reason) => {
                if submission.receipt_policy == ReceiptPolicy::RequireSupported {
                    return Ok(Prepared::Rejected {
                        notification_id,
                        reason,
                    });
                }
                false
            }
        };
        // Feature omission must leave a submission that is still internally
        // consistent. In particular, dropping actions also drops their activation
        // targets while preserving the default body-tap target.
        normalized.validate()?;
        Ok(Prepared::Ready {
            submission: normalized,
            omitted_actions,
            omitted_progress,
            receipt_supported,
        })
    }

    fn publish(&self, event: NotificationEvent) {
        let snapshot = self.inner.subscribers.borrow().clone();
        for (_, active, sender) in &snapshot {
            if active.get() && !sender.send(event.clone()) {
                active.set(false);
            }
        }
        self.inner
            .subscribers
            .borrow_mut()
            .retain(|(_, active, _)| active.get());
    }
}

fn capability_problem(capability: &Capability<NotificationOperation>) -> Option<PostRejectReason> {
    if matches!(
        capability.support,
        Support::Unsupported | Support::Unknown | Support::FallbackAvailable
    ) {
        return Some(PostRejectReason::Unsupported(capability.operation));
    }
    if capability.configuration != Configuration::Ready {
        return Some(PostRejectReason::MissingApplicationIdentity);
    }
    if capability.availability != Availability::Available {
        return Some(PostRejectReason::UnavailableProvider);
    }
    None
}

fn permission_rejection(error: SystemError) -> PostRejectReason {
    match error.code {
        ErrorCode::InvalidConfiguration => PostRejectReason::MissingApplicationIdentity,
        ErrorCode::Unavailable => PostRejectReason::UnavailableProvider,
        _ => PostRejectReason::UnavailableProvider,
    }
}

pub(crate) fn permission_status_rejection(status: PermissionStatus) -> PostRejectReason {
    match status {
        PermissionStatus::Denied { can_request_again } => PostRejectReason::PermissionDenied {
            can_request_again,
            recovery: RecoveryGuidance::notification_settings(),
        },
        PermissionStatus::NotDetermined => PostRejectReason::PermissionDenied {
            can_request_again: true,
            recovery: RecoveryGuidance::notification_settings(),
        },
        PermissionStatus::Restricted => PostRejectReason::PermissionDenied {
            can_request_again: false,
            recovery: RecoveryGuidance::notification_settings(),
        },
        PermissionStatus::Unavailable => PostRejectReason::UnavailableProvider,
        PermissionStatus::NotRequired | PermissionStatus::Granted | PermissionStatus::Limited => {
            PostRejectReason::UnavailableProvider
        }
    }
}
