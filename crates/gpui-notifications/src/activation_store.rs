use crate::{ActionId, NotificationActivationBinding, NotificationId, error};
use gpui_activation::{
    ActivationEvent, ActivationId, ActivationInbox, ActivationPayload, ActivationSource,
    EnqueueReceipt, EnqueueStatus, RouteRegistry,
};
use gpui_app_data::{AppData, DataLocation, DataScope, SettingsStore, WriteReceipt};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File, OpenOptions},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_NAMESPACE: usize = 64;
const MAX_CAPACITY: usize = 64;
const MAX_RETENTION: Duration = Duration::from_secs(30 * 86400);
const SETTINGS_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActivationConfig {
    pub capacity: usize,
    pub retention: Duration,
}

impl Default for NotificationActivationConfig {
    fn default() -> Self {
        Self {
            capacity: 32,
            retention: Duration::from_secs(7 * 86400),
        }
    }
}

/// One host notification callback. The host must reuse `activation_event_id` for
/// redelivery of the same native event; it must not derive the identity from the
/// route or notification content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationActivationCallback {
    activation_event_id: ActivationId,
    notification_id: NotificationId,
    action_id: Option<ActionId>,
    received_at: SystemTime,
}

impl NotificationActivationCallback {
    pub fn new(
        activation_event_id: ActivationId,
        notification_id: NotificationId,
        action_id: Option<ActionId>,
        received_at: SystemTime,
    ) -> Self {
        Self {
            activation_event_id,
            notification_id,
            action_id,
            received_at,
        }
    }

    pub fn activation_event_id(&self) -> &ActivationId {
        &self.activation_event_id
    }

    pub fn notification_id(&self) -> &NotificationId {
        &self.notification_id
    }

    pub fn action_id(&self) -> Option<&ActionId> {
        self.action_id.as_ref()
    }

    pub fn received_at(&self) -> SystemTime {
        self.received_at
    }

    pub const fn source(&self) -> ActivationSource {
        ActivationSource::Notification
    }
}

/// Result of resolving a notification callback and enqueueing its activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationRoute {
    Enqueued {
        notification_id: NotificationId,
        action_id: Option<ActionId>,
        target: gpui_activation::ActivationTarget,
        receipt: EnqueueReceipt,
    },
    /// The notification has a live binding, but the callback's action target was
    /// removed, typically by a stable-ID replacement. Nothing is enqueued.
    UnboundAction {
        notification_id: NotificationId,
        action_id: ActionId,
    },
    MissingBinding,
}

impl ActivationRoute {
    pub fn receipt(&self) -> Option<&EnqueueReceipt> {
        match self {
            Self::Enqueued { receipt, .. } => Some(receipt),
            Self::UnboundAction { .. } | Self::MissingBinding => None,
        }
    }

    pub fn is_duplicate(&self) -> bool {
        self.receipt()
            .is_some_and(|receipt| receipt.status == EnqueueStatus::Duplicate)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    notification_id: NotificationId,
    binding: NotificationActivationBinding,
    stored_ms: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    entries: Vec<Entry>,
}

struct Inner {
    store: SettingsStore,
    config: NotificationActivationConfig,
    routes: RouteRegistry,
    namespace: String,
    state: Mutex<State>,
    _namespace_lock: File,
}

/// Durable notification activation metadata.
///
/// The store persists only [`NotificationActivationBinding`] reconstruction
/// targets keyed by [`NotificationId`]. Title, body, action labels and channel
/// content are never written. All methods perform blocking file I/O and reject
/// UI-bound contexts, so they belong on a background worker.
///
/// A per-namespace OS file lock prevents two store objects sharing one AppData
/// owner from performing conflicting load/save cycles. Entry capacity and
/// retention are bounded; expired entries are pruned lazily and the oldest entry
/// is evicted only when a new identity is remembered at capacity.
#[derive(Clone)]
pub struct NotificationActivationStore {
    inner: Arc<Inner>,
}

impl fmt::Debug for NotificationActivationStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries = self
            .inner
            .state
            .try_lock()
            .map_or(0, |state| state.entries.len());
        formatter
            .debug_struct("NotificationActivationStore")
            .field("namespace", &self.inner.namespace)
            .field("capacity", &self.inner.config.capacity)
            .field("entries", &entries)
            .finish_non_exhaustive()
    }
}

impl NotificationActivationStore {
    pub fn open(
        app: &AppData,
        namespace: &str,
        config: NotificationActivationConfig,
        routes: RouteRegistry,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        if namespace.is_empty()
            || namespace.len() > MAX_NAMESPACE
            || !namespace
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || config.capacity == 0
            || config.capacity > MAX_CAPACITY
            || config.retention.is_zero()
            || config.retention > MAX_RETENTION
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Notification activation limits or namespace are invalid",
            ));
        }
        let directory = app
            .location(DataLocation::Config)
            .child("notification-locks")?;
        fs::create_dir_all(&directory).map_err(|_| {
            error(
                ErrorCode::PlatformFailure,
                "Notification activation namespace directory is unavailable",
            )
        })?;
        let namespace_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("activation-{namespace}.lock")))
            .map_err(|_| {
                error(
                    ErrorCode::PlatformFailure,
                    "Notification activation namespace lock is unavailable",
                )
            })?;
        namespace_lock.try_lock().map_err(|_| {
            error(
                ErrorCode::Busy,
                "Notification activation namespace already has an owner",
            )
        })?;
        let store = app.settings(
            DataScope::Application,
            &format!("notification-activation-{namespace}"),
            1,
            SETTINGS_MAX_BYTES,
        )?;
        let state: State = match store.load(context)? {
            Completion::Completed(state) => state.unwrap_or_default(),
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        if state.entries.len() > config.capacity {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Persisted notification activation metadata exceeds configured limits; original preserved",
            ));
        }
        let mut ids = BTreeSet::new();
        for entry in &state.entries {
            NotificationId::new(entry.notification_id.as_str())?;
            entry.binding.validate(&routes)?;
            if !ids.insert(entry.notification_id.clone()) {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Persisted notification activation metadata has duplicate identities; original preserved",
                ));
            }
        }
        Ok(Completion::Completed(Self {
            inner: Arc::new(Inner {
                store,
                config,
                routes,
                namespace: namespace.to_owned(),
                state: Mutex::new(state),
                _namespace_lock: namespace_lock,
            }),
        }))
    }

    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    pub fn config(&self) -> &NotificationActivationConfig {
        &self.inner.config
    }

    pub fn len(&self) -> usize {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Upsert reconstruction metadata for a notification.
    ///
    /// A new identity at capacity evicts the oldest entry. This is metadata, not
    /// an unhandled event queue; the application can always reconstruct from a
    /// newer notification. Expired entries are removed before the write.
    pub fn remember(
        &self,
        notification_id: &NotificationId,
        binding: NotificationActivationBinding,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<WriteReceipt> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        NotificationId::new(notification_id.as_str())?;
        binding.validate(&self.inner.routes)?;
        let now = timestamp(now)?;
        let mut state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        let mut next = state.clone();
        prune(&mut next, now, self.inner.config.retention);
        if let Some(existing) = next
            .entries
            .iter_mut()
            .find(|entry| &entry.notification_id == notification_id)
        {
            existing.binding = binding;
            existing.stored_ms = now;
        } else {
            if next.entries.len() == self.inner.config.capacity
                && let Some(index) = next
                    .entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(index, entry)| (entry.stored_ms, *index))
                    .map(|(index, _)| index)
            {
                next.entries.remove(index);
            }
            next.entries.push(Entry {
                notification_id: notification_id.clone(),
                binding,
                stored_ms: now,
            });
        }
        match self.inner.store.save(&next, context)? {
            Completion::Completed(write) => {
                *state = next;
                Ok(Completion::Completed(write))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    /// Forget reconstruction metadata, for example after a withdrawal or a
    /// notification the application will never accept again. Returns true when an
    /// entry was removed and persisted.
    pub fn forget(
        &self,
        notification_id: &NotificationId,
        context: &OperationContext,
    ) -> ServiceResult<bool> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let mut state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        let mut next = state.clone();
        let before = next.entries.len();
        next.entries
            .retain(|entry| &entry.notification_id != notification_id);
        if next.entries.len() == before {
            return Ok(Completion::Completed(false));
        }
        match self.inner.store.save(&next, context)? {
            Completion::Completed(_) => {
                *state = next;
                Ok(Completion::Completed(true))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    /// Look up live reconstruction metadata. Expired entries are not returned and
    /// are not deleted by this read-only method.
    pub fn lookup(
        &self,
        notification_id: &NotificationId,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<Option<NotificationActivationBinding>> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let now = timestamp(now)?;
        let state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        Ok(Completion::Completed(
            state
                .entries
                .iter()
                .find(|entry| {
                    &entry.notification_id == notification_id
                        && !expired(entry.stored_ms, now, self.inner.config.retention)
                })
                .map(|entry| entry.binding.clone()),
        ))
    }

    fn live_binding(
        &self,
        notification_id: &NotificationId,
        now: u64,
    ) -> Result<Option<NotificationActivationBinding>, SystemError> {
        let state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        Ok(state
            .entries
            .iter()
            .find(|entry| {
                &entry.notification_id == notification_id
                    && !expired(entry.stored_ms, now, self.inner.config.retention)
            })
            .map(|entry| entry.binding.clone()))
    }

    /// Resolve one host callback into a durable [`ActivationEvent`] using the
    /// persisted binding. No event is enqueued and no state is mutated.
    pub fn route_activation(
        &self,
        callback: &NotificationActivationCallback,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<Option<ActivationEvent>> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        NotificationId::new(callback.notification_id.as_str())?;
        let now = timestamp(now)?;
        let state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        let Some(entry) = state.entries.iter().find(|entry| {
            entry.notification_id == callback.notification_id
                && !expired(entry.stored_ms, now, self.inner.config.retention)
        }) else {
            return Ok(Completion::Completed(None));
        };
        let Some(target) = entry
            .binding
            .target_for(callback.action_id.as_ref())
            .cloned()
        else {
            // A replacement can remove an action while an earlier banner is still
            // on screen. That stale callback is an ordinary non-routing miss.
            return Ok(Completion::Completed(None));
        };
        let payload_notification_id = ActivationId::new(callback.notification_id.as_str())?;
        let event = ActivationEvent::new(
            callback.activation_event_id.clone(),
            ActivationSource::Notification,
            ActivationPayload::Notification {
                notification_id: payload_notification_id,
                target,
            },
            callback.received_at,
        )?;
        Ok(Completion::Completed(Some(event)))
    }

    /// Resolve a warm or cold-launch notification callback and enqueue it into the
    /// durable [`ActivationInbox`]. The same store works in a brand-new process:
    /// only metadata was persisted, never the notification content.
    ///
    /// A duplicate host callback reuses the same [`ActivationId`] and receives
    /// `EnqueueStatus::Duplicate`; the inbox's acknowledgment tombstone preserves
    /// that answer after the event was handled.
    ///
    /// A stale action callback for a replaced notification is reported as
    /// [`ActivationRoute::UnboundAction`] without enqueueing an event. A body tap
    /// still routes to the live default target.
    pub fn ingest_activation(
        &self,
        callback: NotificationActivationCallback,
        inbox: &ActivationInbox,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<ActivationRoute> {
        let notification_id = callback.notification_id.clone();
        let action_id = callback.action_id.clone();
        let event = match self.route_activation(&callback, now, context)? {
            Completion::Completed(Some(event)) => event,
            Completion::Completed(None) => {
                let now = timestamp(now)?;
                let route = match (self.live_binding(&notification_id, now)?, action_id) {
                    (Some(binding), Some(action_id))
                        if binding.target_for(Some(&action_id)).is_none() =>
                    {
                        ActivationRoute::UnboundAction {
                            notification_id,
                            action_id,
                        }
                    }
                    _ => ActivationRoute::MissingBinding,
                };
                return Ok(Completion::Completed(route));
            }
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        let target = match event.payload() {
            ActivationPayload::Notification { target, .. } => target.clone(),
            _ => {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Notification activation produced an unexpected payload",
                ));
            }
        };
        match inbox.enqueue(event, now, context)? {
            Completion::Completed(receipt) => {
                Ok(Completion::Completed(ActivationRoute::Enqueued {
                    notification_id,
                    action_id,
                    target,
                    receipt,
                }))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    /// Persist retention cleanup explicitly. Future timestamps survive backward
    /// wall-clock corrections; capacity is already enforced by `remember`.
    pub fn expire(&self, now: SystemTime, context: &OperationContext) -> ServiceResult<usize> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let now = timestamp(now)?;
        let mut state = self
            .inner
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Notification activation store is busy"))?;
        let mut next = state.clone();
        let before = next.entries.len();
        prune(&mut next, now, self.inner.config.retention);
        let removed = before - next.entries.len();
        if removed == 0 {
            return Ok(Completion::Completed(0));
        }
        match self.inner.store.save(&next, context)? {
            Completion::Completed(_) => {
                *state = next;
                Ok(Completion::Completed(removed))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }
}

fn check<T>(context: &OperationContext) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Notification activation metadata requires a background context without UI attachment",
        ));
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

fn timestamp(time: SystemTime) -> Result<u64, SystemError> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .ok_or_else(|| {
            error(
                ErrorCode::InvalidInput,
                "Notification activation timestamp is outside the supported range",
            )
        })
}

fn expired(stored_ms: u64, now: u64, retention: Duration) -> bool {
    now.checked_sub(stored_ms)
        .is_some_and(|age| u128::from(age) >= retention.as_millis())
}

fn prune(state: &mut State, now: u64, retention: Duration) {
    state
        .entries
        .retain(|entry| !expired(entry.stored_ms, now, retention));
}
