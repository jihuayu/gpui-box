use crate::{
    ApplicationIdentity, Capability, NotificationId, NotificationObservation,
    NotificationOperation, NotificationSubmission, PostReceipt, PostRejectReason, WithdrawOutcome,
    unsupported_capability,
};

/// A local notification provider.
///
/// Implementations are UI-local and must not block: submission, replacement and
/// withdrawal are immediate handoffs. A provider reports submission evidence in
/// [`PostReceipt`] and enqueues display, dismissal and activation observations for
/// the service to drain. A provider never claims that a notification was displayed
/// merely because it accepted a submission.
pub trait NotificationProvider {
    /// Describe one operation without presenting UI, posting a notification or
    /// requesting authorization.
    fn capability(&self, operation: NotificationOperation) -> Capability<NotificationOperation>;

    /// Whether the packaged application identity required by the platform is
    /// available. A missing identity is a configuration failure, not a user denial.
    fn application_identity(&self) -> ApplicationIdentity;

    /// Whether the notification center/server and the optional channel are
    /// enabled. Disabled state is distinct from authorization.
    fn channel_enabled(&self, channel_id: Option<&str>) -> bool;

    /// Submit a new notification immediately.
    fn post(&self, submission: &NotificationSubmission) -> Result<PostReceipt, PostRejectReason>;

    /// Replace the notification with the same stable identity where supported.
    fn replace(&self, submission: &NotificationSubmission)
    -> Result<PostReceipt, PostRejectReason>;

    /// Withdraw a submitted notification where supported.
    fn withdraw(
        &self,
        notification_id: &NotificationId,
    ) -> Result<WithdrawOutcome, PostRejectReason>;

    /// Drain provider observations. The service calls this after every operation
    /// and publishes the results only after all provider state borrows are gone.
    fn take_observations(&self) -> Vec<NotificationObservation> {
        Vec::new()
    }
}

/// A conservative provider that installs nothing and reports every operation as
/// unsupported or unavailable.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableNotificationProvider;

impl NotificationProvider for UnavailableNotificationProvider {
    fn capability(&self, operation: NotificationOperation) -> Capability<NotificationOperation> {
        let mut capability = unsupported_capability(operation);
        // The contract would support the operation, but no provider is installed
        // in this build. Reporting both facts keeps the outcome actionable as
        // `UnavailableProvider` rather than a generic unsupported operation.
        capability.support = crate::Support::Conditional;
        capability.availability = crate::Availability::Unavailable;
        capability.configuration = crate::Configuration::Ready;
        capability
    }

    fn application_identity(&self) -> ApplicationIdentity {
        ApplicationIdentity::Missing
    }

    fn channel_enabled(&self, _channel_id: Option<&str>) -> bool {
        false
    }

    fn post(&self, _submission: &NotificationSubmission) -> Result<PostReceipt, PostRejectReason> {
        Err(PostRejectReason::UnavailableProvider)
    }

    fn replace(
        &self,
        _submission: &NotificationSubmission,
    ) -> Result<PostReceipt, PostRejectReason> {
        Err(PostRejectReason::UnavailableProvider)
    }

    fn withdraw(
        &self,
        _notification_id: &NotificationId,
    ) -> Result<WithdrawOutcome, PostRejectReason> {
        Ok(WithdrawOutcome::Unsupported)
    }
}
