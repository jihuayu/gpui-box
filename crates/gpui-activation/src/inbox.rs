use crate::{ActivationEvent, ActivationId, RouteRegistry, UrlPolicy, error, timestamp};
use gpui_app_data::{AppData, DataLocation, DataScope, SettingsStore, WriteReceipt};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug, Clone)]
pub struct InboxConfig {
    pub capacity: usize,
    pub dedup_capacity: usize,
    pub retention: Duration,
}
impl Default for InboxConfig {
    fn default() -> Self {
        Self {
            capacity: 32,
            dedup_capacity: 128,
            retention: Duration::from_secs(86400),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    event: ActivationEvent,
    accepted_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handled {
    id: ActivationId,
    acknowledged_ms: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    pending: Vec<Entry>,
    handled: Vec<Handled>,
}
struct Inner {
    store: SettingsStore,
    config: InboxConfig,
    urls: UrlPolicy,
    routes: RouteRegistry,
    state: Mutex<State>,
    ready: AtomicBool,
    _namespace_lock: File,
}
/// Clone this authoritative namespace owner. All methods performing I/O belong on
/// a background worker. The ready flag is process-local; the queue is durable.
#[derive(Clone)]
pub struct ActivationInbox(Arc<Inner>);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueStatus {
    Queued,
    Duplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnqueueReceipt {
    pub status: EnqueueStatus,
    pub write: Option<WriteReceipt>,
}
impl ActivationInbox {
    pub fn open(
        app: &AppData,
        namespace: &str,
        config: InboxConfig,
        urls: UrlPolicy,
        routes: RouteRegistry,
        context: &OperationContext,
    ) -> ServiceResult<Self> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        if namespace.is_empty()
            || namespace.len() > 64
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || config.capacity == 0
            || config.capacity > 64
            || config.dedup_capacity == 0
            || config.dedup_capacity > 256
            || config.retention.is_zero()
            || config.retention > Duration::from_secs(30 * 86400)
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Activation inbox limits or namespace are invalid",
            ));
        }
        let directory = app
            .location(DataLocation::Config)
            .child("activation-locks")?;
        fs::create_dir_all(&directory).map_err(|_| {
            error(
                ErrorCode::PlatformFailure,
                "Activation namespace directory is unavailable",
            )
        })?;
        let namespace_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join(format!("inbox-{namespace}.lock")))
            .map_err(|_| {
                error(
                    ErrorCode::PlatformFailure,
                    "Activation namespace lock is unavailable",
                )
            })?;
        namespace_lock.try_lock().map_err(|_| {
            error(
                ErrorCode::Busy,
                "Activation inbox namespace already has an owner",
            )
        })?;
        let store = app.settings(
            DataScope::Application,
            &format!("activation-{namespace}"),
            1,
            2 * 1024 * 1024,
        )?;
        let state: State = match store.load(context)? {
            Completion::Completed(state) => state.unwrap_or_default(),
            Completion::Cancelled(reason) => return Ok(Completion::Cancelled(reason)),
        };
        if state.pending.len() > config.capacity || state.handled.len() > config.dedup_capacity {
            return Err(error(
                ErrorCode::IntegrityFailure,
                "Persisted inbox exceeds configured limits; original preserved",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &state.pending {
            entry.event.validate(&urls, &routes)?;
            if !ids.insert(entry.event.id().clone()) {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Persisted inbox has duplicate identities; original preserved",
                ));
            }
        }
        for handled in &state.handled {
            ActivationId::new(handled.id.as_str())?;
            if !ids.insert(handled.id.clone()) {
                return Err(error(
                    ErrorCode::IntegrityFailure,
                    "Persisted inbox has duplicate identities; original preserved",
                ));
            }
        }
        Ok(Completion::Completed(Self(Arc::new(Inner {
            store,
            config,
            urls,
            routes,
            state: Mutex::new(state),
            ready: AtomicBool::new(false),
            _namespace_lock: namespace_lock,
        }))))
    }
    /// Application-owned ready gate. Reset before a host/model recreation.
    pub fn set_ready(&self, ready: bool) {
        self.0.ready.store(ready, Ordering::Release);
    }
    pub fn is_ready(&self) -> bool {
        self.0.ready.load(Ordering::Acquire)
    }
    pub fn enqueue(
        &self,
        event: ActivationEvent,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<EnqueueReceipt> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        event.validate(&self.0.urls, &self.0.routes)?;
        let now = timestamp(now)?;
        let mut state = self
            .0
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Activation inbox is busy"))?;
        let mut next = state.clone();
        prune(&mut next, now, self.0.config.retention);
        if next
            .pending
            .iter()
            .any(|entry| entry.event.id() == event.id())
            || next.handled.iter().any(|entry| &entry.id == event.id())
        {
            return Ok(Completion::Completed(EnqueueReceipt {
                status: EnqueueStatus::Duplicate,
                write: None,
            }));
        }
        if next.pending.len() == self.0.config.capacity {
            return Err(error(
                ErrorCode::Busy,
                "Activation inbox is full; retry after acknowledgment",
            ));
        }
        next.pending.push(Entry {
            event,
            accepted_ms: now,
        });
        match self.0.store.save(&next, context)? {
            Completion::Completed(write) => {
                *state = next;
                Ok(Completion::Completed(EnqueueReceipt {
                    status: EnqueueStatus::Queued,
                    write: Some(write),
                }))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }
    /// At-least-once snapshot, never destructive. Repeated calls can redeliver the
    /// same identity until acknowledgment. Call business handlers after return.
    pub fn pending(
        &self,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<Vec<ActivationEvent>> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        if !self.is_ready() {
            return Ok(Completion::Completed(Vec::new()));
        }
        let now = timestamp(now)?;
        let state = self
            .0
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Activation inbox is busy"))?;
        Ok(Completion::Completed(
            state
                .pending
                .iter()
                .filter(|entry| !expired(entry.accepted_ms, now, self.0.config.retention))
                .map(|entry| entry.event.clone())
                .collect(),
        ))
    }
    /// Ack only after the application handled the event. A crash between handling
    /// and ack redelivers; consumers must make their own business operation idempotent.
    pub fn acknowledge(
        &self,
        id: &ActivationId,
        now: SystemTime,
        context: &OperationContext,
    ) -> ServiceResult<Option<WriteReceipt>> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let now = timestamp(now)?;
        let mut state = self
            .0
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Activation inbox is busy"))?;
        let mut next = state.clone();
        let Some(index) = next.pending.iter().position(|entry| entry.event.id() == id) else {
            return Ok(Completion::Completed(None));
        };
        next.pending.remove(index);
        prune(&mut next, now, self.0.config.retention);
        if next.handled.len() == self.0.config.dedup_capacity {
            next.handled.remove(0);
        }
        next.handled.push(Handled {
            id: id.clone(),
            acknowledged_ms: now,
        });
        match self.0.store.save(&next, context)? {
            Completion::Completed(write) => {
                *state = next;
                Ok(Completion::Completed(Some(write)))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }
    /// Persist retention cleanup explicitly. Future timestamps survive backward
    /// wall-clock corrections; the queue remains bounded by configured capacity.
    pub fn expire(&self, now: SystemTime, context: &OperationContext) -> ServiceResult<usize> {
        if let Some(cancelled) = check(context)? {
            return Ok(cancelled);
        }
        let now = timestamp(now)?;
        let mut state = self
            .0
            .state
            .try_lock()
            .map_err(|_| error(ErrorCode::Busy, "Activation inbox is busy"))?;
        let mut next = state.clone();
        let before = next.pending.len() + next.handled.len();
        prune(&mut next, now, self.0.config.retention);
        let removed = before - next.pending.len() - next.handled.len();
        if removed == 0 {
            return Ok(Completion::Completed(0));
        }
        match self.0.store.save(&next, context)? {
            Completion::Completed(_) => {
                *state = next;
                Ok(Completion::Completed(removed))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }
}
fn expired(at: u64, now: u64, retention: Duration) -> bool {
    now.checked_sub(at)
        .is_some_and(|age| u128::from(age) >= retention.as_millis())
}
fn prune(state: &mut State, now: u64, retention: Duration) {
    state
        .pending
        .retain(|entry| !expired(entry.accepted_ms, now, retention));
    state
        .handled
        .retain(|entry| !expired(entry.acknowledged_ms, now, retention));
}
fn check<T>(context: &OperationContext) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Durable inbox requires a background context without UI attachment",
        ));
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}
