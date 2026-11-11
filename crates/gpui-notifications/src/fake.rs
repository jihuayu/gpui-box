use crate::{
    ActionId, ApplicationIdentity, Availability, Capability, Configuration, Constraint, Evidence,
    NotificationId, NotificationObservation, NotificationOperation, NotificationProgress,
    NotificationProvider, NotificationSubmission, PermissionState, PostReceipt, PostRejectReason,
    ProviderNotificationId, SubmissionState, Support, WithdrawOutcome, unsupported_capability,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fmt,
};

struct FakeDelivery {
    provider_id: ProviderNotificationId,
    title: String,
    body: String,
    actions: Vec<ActionId>,
    progress: Option<NotificationProgress>,
    revision: u64,
}

fn baseline_capability(operation: NotificationOperation) -> Capability<NotificationOperation> {
    let support = match operation {
        NotificationOperation::ActionButtons => Support::Conditional,
        NotificationOperation::QueryAuthorization
        | NotificationOperation::RequestAuthorization
        | NotificationOperation::PostImmediate
        | NotificationOperation::ReplaceById
        | NotificationOperation::WithdrawById
        | NotificationOperation::ProgressUpdates
        | NotificationOperation::DeliveryReceipt
        | NotificationOperation::ActivationRouting => Support::Native,
    };
    Capability {
        operation,
        support,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::<Constraint>::new(),
        observed_revision: 1,
    }
}

struct FakeState {
    capabilities: BTreeMap<NotificationOperation, Capability<NotificationOperation>>,
    identity: ApplicationIdentity,
    server_enabled: bool,
    channels: BTreeMap<String, bool>,
    submission_state: SubmissionState,
    auto_display: bool,
    revision: u64,
    next_provider_id: u64,
    delivered: BTreeMap<NotificationId, FakeDelivery>,
    submissions: Vec<NotificationSubmission>,
    replacements: Vec<NotificationId>,
    withdrawals: Vec<NotificationId>,
    observations: VecDeque<NotificationObservation>,
}

impl Default for FakeState {
    fn default() -> Self {
        Self {
            capabilities: NotificationOperation::ALL
                .into_iter()
                .map(|operation| (operation, baseline_capability(operation)))
                .collect(),
            identity: ApplicationIdentity::Ready,
            server_enabled: true,
            channels: BTreeMap::new(),
            submission_state: SubmissionState::PostedToCenter,
            auto_display: false,
            revision: 1,
            next_provider_id: 1,
            delivered: BTreeMap::new(),
            submissions: Vec::new(),
            replacements: Vec::new(),
            withdrawals: Vec::new(),
            observations: VecDeque::new(),
        }
    }
}

impl FakeState {
    fn bump_capability(&mut self, operation: NotificationOperation) {
        self.revision = self.revision.saturating_add(1);
        if let Some(capability) = self.capabilities.get_mut(&operation) {
            capability.observed_revision = self.revision;
        }
    }

    fn provider_id(&mut self) -> ProviderNotificationId {
        let value = format!("fake-{}", self.next_provider_id);
        self.next_provider_id = self.next_provider_id.saturating_add(1);
        ProviderNotificationId::new(value).expect("fake provider identity is valid")
    }

    fn deliver(
        &mut self,
        submission: &NotificationSubmission,
        provider_id: ProviderNotificationId,
        replaced_existing: bool,
    ) -> PostReceipt {
        let content = &submission.content;
        let actions = content
            .actions()
            .iter()
            .map(|action| action.id().clone())
            .collect();
        let revision = self.revision;
        self.delivered.insert(
            submission.notification_id.clone(),
            FakeDelivery {
                provider_id: provider_id.clone(),
                title: content.title().to_owned(),
                body: content.body().to_owned(),
                actions,
                progress: content.progress().cloned(),
                revision,
            },
        );
        self.submissions.push(submission.clone());
        if self.auto_display {
            self.observations
                .push_back(NotificationObservation::ObservedDisplayed {
                    notification_id: submission.notification_id.clone(),
                    provider_id: Some(provider_id.clone()),
                });
        }
        PostReceipt {
            notification_id: submission.notification_id.clone(),
            provider_id: Some(provider_id),
            submission: self.submission_state,
            replaced_existing: Some(replaced_existing),
            omitted_actions: false,
            omitted_progress: false,
        }
    }
}

/// Deterministic, UI-local notification provider for headless tests and samples.
///
/// It never contacts a native platform. Capabilities, identity, channel state,
/// display observations and activation transitions are explicit, so tests do not
/// sleep or depend on an OS notification center.
#[derive(Default)]
pub struct FakeNotificationProvider {
    state: RefCell<FakeState>,
}

impl fmt::Debug for FakeNotificationProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        formatter
            .debug_struct("FakeNotificationProvider")
            .field("identity", &state.identity)
            .field("server_enabled", &state.server_enabled)
            .field("auto_display", &state.auto_display)
            .field("delivered", &state.delivered.len())
            .field("submissions", &state.submissions.len())
            .field("replacements", &state.replacements.len())
            .field("withdrawals", &state.withdrawals.len())
            .field("observations", &state.observations.len())
            .finish()
    }
}

impl FakeNotificationProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_identity(self, identity: ApplicationIdentity) -> Self {
        self.set_application_identity(identity);
        self
    }

    pub fn with_server_enabled(self, enabled: bool) -> Self {
        self.set_server_enabled(enabled);
        self
    }

    pub fn with_channel_enabled(self, channel: impl Into<String>, enabled: bool) -> Self {
        self.set_channel_enabled(channel, enabled);
        self
    }

    pub fn with_auto_display(self, enabled: bool) -> Self {
        self.set_auto_display(enabled);
        self
    }

    pub fn with_submission_state(self, state: SubmissionState) -> Self {
        self.set_submission_state(state);
        self
    }

    pub fn with_support(self, operation: NotificationOperation, support: Support) -> Self {
        self.set_support(operation, support);
        self
    }

    pub fn set_application_identity(&self, identity: ApplicationIdentity) {
        self.state.borrow_mut().identity = identity;
    }

    pub fn set_server_enabled(&self, enabled: bool) {
        self.state.borrow_mut().server_enabled = enabled;
    }

    pub fn set_channel_enabled(&self, channel: impl Into<String>, enabled: bool) {
        self.state
            .borrow_mut()
            .channels
            .insert(channel.into(), enabled);
    }

    pub fn set_auto_display(&self, enabled: bool) {
        self.state.borrow_mut().auto_display = enabled;
    }

    /// Choose whether accepted submissions report provider acceptance or a
    /// center handoff. Neither state implies that the user saw the notification.
    pub fn set_submission_state(&self, state: SubmissionState) {
        self.state.borrow_mut().submission_state = state;
    }

    pub fn set_support(&self, operation: NotificationOperation, support: Support) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
        }
        state.bump_capability(operation);
    }

    pub fn set_availability(&self, operation: NotificationOperation, availability: Availability) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.availability = availability;
        }
        state.bump_capability(operation);
    }

    pub fn set_configuration(
        &self,
        operation: NotificationOperation,
        configuration: Configuration,
    ) {
        let mut state = self.state.borrow_mut();
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.configuration = configuration;
        }
        state.bump_capability(operation);
    }

    /// Simulate the platform reporting that the notification is on screen.
    pub fn mark_displayed(&self, notification_id: &NotificationId) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(delivery) = state.delivered.get(notification_id) else {
            return false;
        };
        let provider_id = delivery.provider_id.clone();
        state
            .observations
            .push_back(NotificationObservation::ObservedDisplayed {
                notification_id: notification_id.clone(),
                provider_id: Some(provider_id),
            });
        true
    }

    /// Simulate the user dismissing the notification without activating it.
    pub fn mark_dismissed_by_user(&self, notification_id: &NotificationId) -> bool {
        let mut state = self.state.borrow_mut();
        if state.delivered.remove(notification_id).is_none() {
            return false;
        }
        state
            .observations
            .push_back(NotificationObservation::DismissedByUser {
                notification_id: notification_id.clone(),
            });
        true
    }

    /// Simulate a user tap or action-button press.
    pub fn mark_activated(
        &self,
        notification_id: &NotificationId,
        action_id: Option<&ActionId>,
    ) -> bool {
        let mut state = self.state.borrow_mut();
        let Some(delivery) = state.delivered.get(notification_id) else {
            return false;
        };
        if action_id.is_some_and(|action| !delivery.actions.contains(action)) {
            return false;
        }
        state
            .observations
            .push_back(NotificationObservation::Activated {
                notification_id: notification_id.clone(),
                action_id: action_id.cloned(),
            });
        true
    }

    pub fn delivered_ids(&self) -> Vec<NotificationId> {
        self.state
            .borrow()
            .delivered
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn delivered_title(&self, notification_id: &NotificationId) -> Option<String> {
        self.state
            .borrow()
            .delivered
            .get(notification_id)
            .map(|delivery| delivery.title.clone())
    }

    pub fn delivered_body(&self, notification_id: &NotificationId) -> Option<String> {
        self.state
            .borrow()
            .delivered
            .get(notification_id)
            .map(|delivery| delivery.body.clone())
    }

    pub fn delivered_actions(&self, notification_id: &NotificationId) -> Option<Vec<ActionId>> {
        self.state
            .borrow()
            .delivered
            .get(notification_id)
            .map(|delivery| delivery.actions.clone())
    }

    pub fn delivered_progress(
        &self,
        notification_id: &NotificationId,
    ) -> Option<Option<NotificationProgress>> {
        self.state
            .borrow()
            .delivered
            .get(notification_id)
            .map(|delivery| delivery.progress.clone())
    }

    pub fn delivered_revision(&self, notification_id: &NotificationId) -> Option<u64> {
        self.state
            .borrow()
            .delivered
            .get(notification_id)
            .map(|delivery| delivery.revision)
    }

    pub fn submission_count(&self) -> usize {
        self.state.borrow().submissions.len()
    }

    /// The last submission exactly as the provider observed it after service
    /// policy gating. Providers receive a validated, internally consistent
    /// submission.
    pub fn last_submission(&self) -> Option<NotificationSubmission> {
        self.state.borrow().submissions.last().cloned()
    }

    pub fn replacement_ids(&self) -> Vec<NotificationId> {
        self.state.borrow().replacements.clone()
    }

    pub fn withdrawal_ids(&self) -> Vec<NotificationId> {
        self.state.borrow().withdrawals.clone()
    }

    pub fn observation_count(&self) -> usize {
        self.state.borrow().observations.len()
    }

    fn check_operation(
        state: &FakeState,
        operation: NotificationOperation,
    ) -> Result<(), PostRejectReason> {
        let capability = &state.capabilities[&operation];
        if matches!(
            capability.support,
            Support::Unsupported | Support::Unknown | Support::FallbackAvailable
        ) {
            return Err(PostRejectReason::Unsupported(operation));
        }
        if capability.configuration != Configuration::Ready {
            return Err(PostRejectReason::MissingApplicationIdentity);
        }
        if capability.availability != Availability::Available {
            return Err(PostRejectReason::UnavailableProvider);
        }
        Ok(())
    }

    fn check_common(state: &FakeState, channel_id: Option<&str>) -> Result<(), PostRejectReason> {
        if state.identity == ApplicationIdentity::Missing {
            return Err(PostRejectReason::MissingApplicationIdentity);
        }
        if !state.server_enabled
            || channel_id.is_some_and(|channel| state.channels.get(channel) == Some(&false))
        {
            return Err(PostRejectReason::ChannelOrServerDisabled);
        }
        Ok(())
    }
}

impl NotificationProvider for FakeNotificationProvider {
    fn capability(&self, operation: NotificationOperation) -> Capability<NotificationOperation> {
        self.state
            .borrow()
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| unsupported_capability(operation))
    }

    fn application_identity(&self) -> ApplicationIdentity {
        self.state.borrow().identity
    }

    fn channel_enabled(&self, channel_id: Option<&str>) -> bool {
        let state = self.state.borrow();
        state.server_enabled
            && channel_id.is_none_or(|channel| state.channels.get(channel) != Some(&false))
    }

    fn post(&self, submission: &NotificationSubmission) -> Result<PostReceipt, PostRejectReason> {
        let mut state = self.state.borrow_mut();
        Self::check_operation(&state, NotificationOperation::PostImmediate)?;
        Self::check_common(&state, submission.content.channel_id())?;
        let existed = state.delivered.contains_key(&submission.notification_id);
        let provider_id = state.provider_id();
        Ok(state.deliver(submission, provider_id, existed))
    }

    fn replace(
        &self,
        submission: &NotificationSubmission,
    ) -> Result<PostReceipt, PostRejectReason> {
        let mut state = self.state.borrow_mut();
        Self::check_operation(&state, NotificationOperation::PostImmediate)?;
        Self::check_operation(&state, NotificationOperation::ReplaceById)?;
        Self::check_common(&state, submission.content.channel_id())?;
        state.replacements.push(submission.notification_id.clone());
        let existed = state.delivered.contains_key(&submission.notification_id);
        let provider_id = state.provider_id();
        Ok(state.deliver(submission, provider_id, existed))
    }

    fn withdraw(
        &self,
        notification_id: &NotificationId,
    ) -> Result<WithdrawOutcome, PostRejectReason> {
        let mut state = self.state.borrow_mut();
        state.withdrawals.push(notification_id.clone());
        if matches!(
            state.capabilities[&NotificationOperation::WithdrawById].support,
            Support::Unsupported | Support::Unknown | Support::FallbackAvailable
        ) || state.capabilities[&NotificationOperation::WithdrawById].availability
            != Availability::Available
        {
            return Ok(WithdrawOutcome::Unsupported);
        }
        if state.delivered.remove(notification_id).is_none() {
            return Ok(WithdrawOutcome::NotFoundOrAlreadyDismissed);
        }
        // The service publishes Withdrawn from the authoritative outcome; the
        // provider must not enqueue a duplicate observation.
        Ok(WithdrawOutcome::Withdrawn)
    }

    fn take_observations(&self) -> Vec<NotificationObservation> {
        self.state.borrow_mut().observations.drain(..).collect()
    }
}
