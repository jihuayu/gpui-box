//! UI-local file service: picker serialization, grants and observations.

use crate::error::{MAX_GRANT_BYTES, completed};
use crate::model::{
    FileOperation, FileTypeFilter, OpenFileOptions, OpenFilesOptions, PickDirectoryOptions,
    PickerOutcome, SaveFileOptions,
};
use crate::provider::{FilePickerRequest, FileProvider, PickerCompletion};
use crate::resource::{FileResource, GrantRestoreOutcome, PersistedGrant};
use gpui_system::{
    CancelReason, Capability, Completion, ErrorCode, EventSender, EventSubscription,
    OperationContext, Request, SurfaceId, SystemError, SystemServices, event_channel,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Instant,
};

const EVENT_CAPACITY: usize = 16;

type Subscriber = (Rc<Cell<bool>>, EventSender<FileEvent>);

/// A safe, path-free lifecycle observation. Picker outcomes report counts, not
/// names or locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEvent {
    PickerStarted {
        operation: FileOperation,
        surface: SurfaceId,
    },
    PickerSelected {
        operation: FileOperation,
        surface: SurfaceId,
        resources: usize,
    },
    PickerCancelled {
        operation: FileOperation,
        surface: SurfaceId,
    },
    PickerFailed {
        operation: FileOperation,
        surface: SurfaceId,
        code: ErrorCode,
    },
    ResourceReleased {
        resource: crate::ResourceId,
    },
    GrantPersisted {
        resource: crate::ResourceId,
    },
    GrantRestored {
        resource: crate::ResourceId,
    },
}

#[derive(Default)]
struct ServiceState {
    active: HashMap<SurfaceId, ActivePicker>,
    subscribers: Vec<Subscriber>,
    next_picker: u64,
}

struct ActivePicker {
    token: u64,
    operation: FileOperation,
    context: OperationContext,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickerFinish {
    Selected(usize),
    Cancelled,
    Failed(ErrorCode),
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn FileProvider>,
    state: RefCell<ServiceState>,
}

fn collect_deliveries(
    state: &mut ServiceState,
    event: FileEvent,
) -> Vec<(EventSender<FileEvent>, FileEvent)> {
    state.subscribers.retain(|(live, _)| live.get());
    state
        .subscribers
        .iter()
        .map(|(_, sender)| (sender.clone(), event))
        .collect()
}

impl ServiceInner {
    fn emit(&self, event: FileEvent) {
        let deliveries = {
            let mut state = self.state.borrow_mut();
            collect_deliveries(&mut state, event)
        };
        for (sender, event) in deliveries {
            sender.send(event);
        }
    }

    /// Accept exactly one authoritative picker completion. The lane is always
    /// released, but a stale owner/generation cannot deliver success.
    pub(crate) fn finish_picker(
        &self,
        token: u64,
        operation: FileOperation,
        surface: SurfaceId,
        finish: PickerFinish,
    ) -> bool {
        let (accepted, deliveries) = {
            let mut state = self.state.borrow_mut();
            let Some(active) = state.active.get(&surface) else {
                return false;
            };
            if active.token != token || active.operation != operation {
                return false;
            }
            let active = state
                .active
                .remove(&surface)
                .expect("active picker was just observed");
            let accepted = active.context.cancellation.reason().is_none()
                && active.context.check_at(Instant::now()).is_ok();
            if !accepted {
                (false, Vec::new())
            } else {
                let event = match finish {
                    PickerFinish::Selected(resources) => FileEvent::PickerSelected {
                        operation,
                        surface,
                        resources,
                    },
                    PickerFinish::Cancelled => FileEvent::PickerCancelled { operation, surface },
                    PickerFinish::Failed(code) => FileEvent::PickerFailed {
                        operation,
                        surface,
                        code,
                    },
                };
                (true, collect_deliveries(&mut state, event))
            }
        };
        for (sender, event) in deliveries {
            sender.send(event);
        }
        accepted
    }

    /// Release lanes whose owner generation, disposal or cancellation advanced.
    /// The context checks run only while the service lock is held; sends occur
    /// after the borrow ends so a waker may safely re-enter the service.
    fn clear_stale_pickers(&self) {
        let deliveries = {
            let mut state = self.state.borrow_mut();
            let now = Instant::now();
            let stale: Vec<SurfaceId> = state
                .active
                .iter()
                .filter(|(_, active)| {
                    active.context.cancellation.reason().is_some()
                        || active.context.check_at(now).is_err()
                })
                .map(|(surface, _)| *surface)
                .collect();
            let mut deliveries = Vec::new();
            for surface in stale {
                if let Some(active) = state.active.remove(&surface) {
                    deliveries.extend(collect_deliveries(
                        &mut state,
                        FileEvent::PickerCancelled {
                            operation: active.operation,
                            surface,
                        },
                    ));
                }
            }
            deliveries
        };
        for (sender, event) in deliveries {
            sender.send(event);
        }
    }

    fn begin_picker(
        &self,
        operation: FileOperation,
        context: &OperationContext,
    ) -> Result<(u64, SurfaceId), SystemError> {
        let ui = context.require_ui()?;
        context.check_at(Instant::now())?;
        self.clear_stale_pickers();
        let mut state = self.state.borrow_mut();
        if state.active.contains_key(&ui.surface) {
            return Err(crate::error::busy(
                "A file picker is already active for this surface",
            ));
        }
        let token = state.next_picker;
        state.next_picker = state.next_picker.saturating_add(1);
        state.active.insert(
            ui.surface,
            ActivePicker {
                token,
                operation,
                context: context.clone(),
            },
        );
        Ok((token, ui.surface))
    }
}

impl Drop for ServiceInner {
    fn drop(&mut self) {
        for active in self.state.get_mut().active.values() {
            active.context.cancellation.cancel(CancelReason::Caller);
        }
        self.state.get_mut().active.clear();
    }
}

/// UI-local authorized file service. Cloning shares the provider, picker lanes
/// and observation subscribers.
#[derive(Clone)]
pub struct FileService(Rc<ServiceInner>);

impl FileService {
    pub fn new(provider: Rc<dyn FileProvider>) -> Self {
        Self(Rc::new(ServiceInner {
            provider,
            state: RefCell::new(ServiceState::default()),
        }))
    }

    pub fn from_provider(provider: impl FileProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn provider(&self) -> &Rc<dyn FileProvider> {
        &self.0.provider
    }

    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn capability(&self, operation: FileOperation) -> Capability<FileOperation> {
        self.0.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<FileOperation>> {
        FileOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    /// Create an independent bounded observation stream.
    pub fn subscribe(&self) -> EventSubscription<FileEvent> {
        let live = Rc::new(Cell::new(true));
        let cleanup = Rc::clone(&live);
        let (sender, subscription) = event_channel(EVENT_CAPACITY, move || cleanup.set(false));
        let mut state = self.0.state.borrow_mut();
        state.subscribers.retain(|(live, _)| live.get());
        state.subscribers.push((live, sender));
        subscription
    }

    /// Highest observation sequence across live subscribers.
    pub fn revision(&self) -> u64 {
        let mut state = self.0.state.borrow_mut();
        state.subscribers.retain(|(live, _)| live.get());
        state
            .subscribers
            .iter()
            .map(|(_, sender)| sender.revision())
            .max()
            .unwrap_or(0)
    }

    /// True while an interactive picker owns this surface's lane.
    pub fn active_picker(&self, surface: SurfaceId) -> Option<FileOperation> {
        self.0.clear_stale_pickers();
        self.0
            .state
            .borrow()
            .active
            .get(&surface)
            .map(|active| active.operation)
    }

    /// Interactive picker. A user dismissal is `PickerOutcome::Cancelled`; token
    /// cancellation is an outer `Completion::Cancelled`.
    pub fn open_file(
        &self,
        options: OpenFileOptions,
        cx: OperationContext,
    ) -> Request<PickerOutcome<FileResource>> {
        self.begin_single(
            FileOperation::OpenFile,
            cx,
            move |provider, request, done| provider.open_file(options, request, done),
        )
    }

    pub fn open_files(
        &self,
        options: OpenFilesOptions,
        cx: OperationContext,
    ) -> Request<PickerOutcome<Vec<FileResource>>> {
        self.begin_many(
            FileOperation::OpenMultipleFiles,
            cx,
            move |provider, request, done| provider.open_files(options, request, done),
        )
    }

    pub fn pick_directory(
        &self,
        options: PickDirectoryOptions,
        cx: OperationContext,
    ) -> Request<PickerOutcome<FileResource>> {
        self.begin_single(
            FileOperation::PickDirectory,
            cx,
            move |provider, request, done| provider.pick_directory(options, request, done),
        )
    }

    pub fn save_file(
        &self,
        options: SaveFileOptions,
        cx: OperationContext,
    ) -> Request<PickerOutcome<FileResource>> {
        self.begin_single(
            FileOperation::SaveFile,
            cx,
            move |provider, request, done| provider.save_file(options, request, done),
        )
    }

    fn begin_single(
        &self,
        operation: FileOperation,
        cx: OperationContext,
        start: impl FnOnce(
            &dyn FileProvider,
            FilePickerRequest,
            PickerCompletion<FileResource>,
        ) -> Result<(), SystemError>,
    ) -> Request<PickerOutcome<FileResource>> {
        let (request, port) = gpui_system::request::<PickerOutcome<FileResource>>(cx.clone());
        let (token, surface) = match prepare_picker(&cx, &port, &self.0, operation) {
            Ok(Some(lane)) => lane,
            Ok(None) => return request,
            Err(()) => return request,
        };
        let completion =
            PickerCompletion::new(port, Rc::downgrade(&self.0), token, operation, surface);
        let info = FilePickerRequest {
            request: request.id(),
            operation,
            surface,
        };
        if let Err(error) = start(self.0.provider.as_ref(), info, completion.clone()) {
            completion.fail(error);
        }
        request
    }

    fn begin_many(
        &self,
        operation: FileOperation,
        cx: OperationContext,
        start: impl FnOnce(
            &dyn FileProvider,
            FilePickerRequest,
            PickerCompletion<Vec<FileResource>>,
        ) -> Result<(), SystemError>,
    ) -> Request<PickerOutcome<Vec<FileResource>>> {
        let (request, port) = gpui_system::request::<PickerOutcome<Vec<FileResource>>>(cx.clone());
        let (token, surface) = match prepare_picker(&cx, &port, &self.0, operation) {
            Ok(Some(lane)) => lane,
            Ok(None) => return request,
            Err(()) => return request,
        };
        let completion =
            PickerCompletion::new(port, Rc::downgrade(&self.0), token, operation, surface);
        let info = FilePickerRequest {
            request: request.id(),
            operation,
            surface,
        };
        if let Err(error) = start(self.0.provider.as_ref(), info, completion.clone()) {
            completion.fail(error);
        }
        request
    }

    /// Persist resource access and emit a safe lifecycle event.
    pub fn persist_grant(
        &self,
        resource: &FileResource,
        context: &OperationContext,
    ) -> gpui_system::ServiceResult<PersistedGrant> {
        match resource.persist_grant(context)? {
            Completion::Completed(grant) => {
                self.0.emit(FileEvent::GrantPersisted {
                    resource: resource.id(),
                });
                completed(grant)
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    /// Resolve a durable grant on a later session. All non-restored outcomes are
    /// ordinary recovery results; re-prompting remains the app's decision.
    pub fn restore_grant(
        &self,
        grant: &PersistedGrant,
        context: &OperationContext,
    ) -> gpui_system::ServiceResult<GrantRestoreOutcome> {
        if let Some(cancelled) = crate::error::check_background(context)? {
            return Ok(cancelled);
        }
        let outcome = self.0.provider.restore_grant(grant)?;
        if let GrantRestoreOutcome::Restored { resource, .. } = &outcome {
            self.0.emit(FileEvent::GrantRestored {
                resource: resource.id(),
            });
        }
        completed(outcome)
    }

    /// Decode and restore a serialized grant. Malformed or oversized input is a
    /// typed `InvalidOrCorrupt` recovery outcome, not a crash.
    pub fn restore_grant_bytes(
        &self,
        bytes: impl Into<Vec<u8>>,
        context: &OperationContext,
    ) -> gpui_system::ServiceResult<GrantRestoreOutcome> {
        if let Some(cancelled) = crate::error::check_background(context)? {
            return Ok(cancelled);
        }
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_GRANT_BYTES {
            return completed(GrantRestoreOutcome::InvalidOrCorrupt);
        }
        let grant = match PersistedGrant::from_bytes(bytes) {
            Ok(grant) => grant,
            Err(_) => return completed(GrantRestoreOutcome::InvalidOrCorrupt),
        };
        self.restore_grant(&grant, context)
    }

    /// Explicitly revoke a resource across all clones and emit one safe event.
    pub fn release_resource(&self, resource: &FileResource) -> bool {
        let changed = resource.release();
        if changed {
            self.0.emit(FileEvent::ResourceReleased {
                resource: resource.id(),
            });
        }
        changed
    }

    /// Convenience filter matching for provider or post-selection validation.
    pub fn matches_filters(filters: &[FileTypeFilter], resource: &FileResource) -> bool {
        filters.is_empty()
            || filters
                .iter()
                .any(|filter| filter.matches(&resource.metadata()))
    }
}

/// Validate UI ownership and start the lane, completing the request directly
/// for cancellation or setup failures.
fn prepare_picker<T>(
    cx: &OperationContext,
    port: &gpui_system::CompletionPort<PickerOutcome<T>>,
    service: &Rc<ServiceInner>,
    operation: FileOperation,
) -> Result<Option<(u64, SurfaceId)>, ()> {
    if let Err(error) = cx.require_ui().and_then(|_| cx.check_at(Instant::now())) {
        port.complete(Err(error));
        return Err(());
    }
    if let Some(reason) = cx.cancellation.reason() {
        port.complete(Ok(Completion::Cancelled(reason)));
        return Err(());
    }
    match service.begin_picker(operation, cx) {
        Ok(lane) => {
            service.emit(FileEvent::PickerStarted {
                operation,
                surface: lane.1,
            });
            Ok(Some(lane))
        }
        Err(error) => {
            port.complete(Err(error));
            Err(())
        }
    }
}
