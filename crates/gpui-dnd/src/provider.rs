//! Explicit rich-transfer provider boundary plus conservative and deterministic
//! fake implementations.
//!
//! A provider owns native clipboard/drag objects. It must never hold its own
//! state lock while notifying a listener. The service re-validates interaction,
//! surface, lifecycle and cancellation after every provider call.

use crate::format::{DeliveryMode, FormatDescriptor, PayloadOffer};
use crate::model::{
    AllowedEffects, DragPoint, DragPreview, DragSessionId, DropTargetId, HandoffId,
    MAX_TRANSFERRED_FILES, TypedPayload,
};
use crate::stream::{HandoffScope, LazyRepresentation};
use gpui_system::{
    Availability, Capability, Configuration, ErrorCode as SystemErrorCode, Evidence,
    OperationContext, PermissionState, ServiceResult, Support, SurfaceId, SystemError,
};
use std::{
    any::Any,
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    rc::{Rc, Weak},
};

/// One clipboard representation observed or hosted by a provider.
#[derive(Clone)]
pub enum ClipboardEntry {
    Eager(TypedPayload),
    Delayed {
        representation: LazyRepresentation,
        /// Present when this representation belongs to a service handoff that
        /// must be checked for liveness before its callback runs.
        handoff_id: Option<HandoffId>,
    },
}

impl ClipboardEntry {
    pub fn eager(payload: TypedPayload) -> Self {
        Self::Eager(payload)
    }

    pub fn delayed(representation: LazyRepresentation) -> Self {
        Self::Delayed {
            representation,
            handoff_id: None,
        }
    }

    pub fn delayed_for(representation: LazyRepresentation, handoff_id: HandoffId) -> Self {
        Self::Delayed {
            representation,
            handoff_id: Some(handoff_id),
        }
    }

    pub fn descriptor(&self) -> FormatDescriptor {
        match self {
            Self::Eager(payload) => payload.descriptor(),
            Self::Delayed { representation, .. } => representation.descriptor().clone(),
        }
    }

    pub const fn delivery(&self) -> DeliveryMode {
        match self {
            Self::Eager(_) => DeliveryMode::Eager,
            Self::Delayed { .. } => DeliveryMode::Delayed,
        }
    }

    pub fn offer(&self) -> PayloadOffer {
        let mut offer = PayloadOffer::new(self.descriptor(), self.delivery());
        match self {
            Self::Eager(payload) => {
                offer = offer.with_estimated_bytes(payload.byte_len());
            }
            Self::Delayed { representation, .. } => {
                if let Some(bytes) = representation.estimated_bytes() {
                    offer = offer.with_estimated_bytes(bytes);
                }
            }
        }
        offer
    }

    pub fn handoff_id(&self) -> Option<HandoffId> {
        match self {
            Self::Eager(_) => None,
            Self::Delayed { handoff_id, .. } => *handoff_id,
        }
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::Eager(payload) => payload.validate(),
            Self::Delayed { representation, .. } => representation.validate(),
        }
    }
}

impl fmt::Debug for ClipboardEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eager(payload) => formatter.debug_tuple("Eager").field(payload).finish(),
            Self::Delayed {
                representation,
                handoff_id,
            } => formatter
                .debug_struct("Delayed")
                .field("representation", representation)
                .field("handoff_id", handoff_id)
                .finish(),
        }
    }
}

/// A complete clipboard snapshot taken by a provider.
#[derive(Debug, Clone, Default)]
pub struct ClipboardSnapshot {
    pub entries: Vec<ClipboardEntry>,
}

impl ClipboardSnapshot {
    pub fn new(entries: impl IntoIterator<Item = ClipboardEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn offers(&self) -> Vec<PayloadOffer> {
        self.entries.iter().map(ClipboardEntry::offer).collect()
    }

    pub fn entry_for(&self, descriptor: &FormatDescriptor) -> Option<&ClipboardEntry> {
        self.entries
            .iter()
            .find(|entry| entry.descriptor().matches(descriptor))
    }
}

/// A provider-facing clipboard publication request.
#[derive(Clone)]
pub struct ClipboardHandoff {
    pub handoff_id: HandoffId,
    pub scope: HandoffScope,
    pub eager: Vec<TypedPayload>,
    pub delayed: Vec<LazyRepresentation>,
}

impl ClipboardHandoff {
    pub fn formats(&self) -> Vec<FormatDescriptor> {
        self.eager
            .iter()
            .map(TypedPayload::descriptor)
            .chain(
                self.delayed
                    .iter()
                    .map(|delayed| delayed.descriptor().clone()),
            )
            .collect()
    }

    pub fn delivered_files(&self) -> usize {
        self.eager
            .iter()
            .filter_map(TypedPayload::as_files)
            .map(<[gpui_files::FileResource]>::len)
            .sum()
    }
}

impl fmt::Debug for ClipboardHandoff {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClipboardHandoff")
            .field("handoff_id", &self.handoff_id)
            .field("scope", &self.scope)
            .field("eager", &self.eager.len())
            .field("delayed", &self.delayed.len())
            .field("delivered_files", &self.delivered_files())
            .finish()
    }
}

/// The native acceptance grant for one clipboard handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderClipboardGrant {
    pub native_handoff_token: u64,
    pub sequence: Option<u64>,
}

impl ProviderClipboardGrant {
    pub const fn new(native_handoff_token: u64, sequence: Option<u64>) -> Self {
        Self {
            native_handoff_token,
            sequence,
        }
    }
}

/// A provider-facing drag start request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDragRequest {
    pub session_id: DragSessionId,
    pub source_surface: SurfaceId,
    pub source_generation: u64,
    pub allowed_effects: AllowedEffects,
    pub preview: DragPreview,
    pub offers: Vec<PayloadOffer>,
    pub delayed_formats: Vec<FormatDescriptor>,
    pub promised_files: usize,
}

/// The native session grant returned by a successful drag start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderDragGrant {
    pub native_session_token: u64,
    pub session_epoch: u64,
}

impl ProviderDragGrant {
    pub const fn new(native_session_token: u64, session_epoch: u64) -> Self {
        Self {
            native_session_token,
            session_epoch,
        }
    }
}

/// A provider observation for one completed drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDropOutcome {
    pub session_id: DragSessionId,
    pub target_id: DropTargetId,
    pub format: FormatDescriptor,
    pub effect: crate::model::DragEffect,
}

/// Provider-to-service observations pushed synchronously.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RichTransferProviderNotice {
    ClipboardChanged,
    ExternalDragEntered {
        native_session_token: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        offers: Vec<PayloadOffer>,
    },
    ExternalDragLeft {
        native_session_token: u64,
    },
    ExternalDrop {
        native_session_token: u64,
        point: DragPoint,
    },
    SessionCancelled {
        native_session_token: u64,
    },
    SourceExited,
}

/// Listener registered by a service on its provider.
pub trait RichTransferProviderListener {
    fn on_rich_transfer_notice(&self, notice: RichTransferProviderNotice);
}

/// Explicit native/provider boundary.
///
/// Clipboard and drag methods run on the owning UI thread. `cancel_drag` must be
/// idempotent and return false for an unknown or already-completed token.
pub trait RichTransferProvider {
    fn platform(&self) -> crate::platform::PlatformTarget;

    fn capability(
        &self,
        operation: crate::platform::DndOperation,
    ) -> Capability<crate::platform::DndOperation>;

    fn capabilities(&self) -> Vec<Capability<crate::platform::DndOperation>> {
        crate::platform::DndOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    fn clipboard_snapshot(&self, context: &OperationContext) -> ServiceResult<ClipboardSnapshot>;

    fn publish_clipboard(
        &self,
        request: &ClipboardHandoff,
    ) -> Result<ProviderClipboardGrant, SystemError>;

    /// Revoke only delayed/lazy representations for one native handoff. Eager
    /// formats remain in the OS clipboard as users expect.
    fn release_delayed(&self, native_handoff_token: u64) -> bool {
        let _ = native_handoff_token;
        false
    }

    fn start_drag(&self, request: &ProviderDragRequest) -> Result<ProviderDragGrant, SystemError>;

    fn complete_drag(
        &self,
        native_session_token: u64,
        outcome: &ProviderDropOutcome,
    ) -> Result<(), SystemError> {
        let _ = (native_session_token, outcome);
        Ok(())
    }

    /// Cancel a native session. Returns true exactly once for a live token.
    fn cancel_drag(&self, native_session_token: u64) -> bool;

    fn attach_listener(&self, listener: Weak<dyn RichTransferProviderListener>);

    fn as_any(&self) -> &dyn Any;
}

/// Conservative default provider. Every operation is unsupported and no native
/// state is fabricated.
#[derive(Debug, Clone, Copy)]
pub struct UnsupportedRichTransferProvider {
    platform: crate::platform::PlatformTarget,
}

impl Default for UnsupportedRichTransferProvider {
    fn default() -> Self {
        Self::new(
            crate::platform::PlatformTarget::current()
                .unwrap_or(crate::platform::PlatformTarget::Linux),
        )
    }
}

impl UnsupportedRichTransferProvider {
    pub fn new(platform: crate::platform::PlatformTarget) -> Self {
        Self { platform }
    }
}

impl RichTransferProvider for UnsupportedRichTransferProvider {
    fn platform(&self) -> crate::platform::PlatformTarget {
        self.platform
    }

    fn capability(
        &self,
        operation: crate::platform::DndOperation,
    ) -> Capability<crate::platform::DndOperation> {
        Capability {
            operation,
            support: Support::Unsupported,
            availability: Availability::Unavailable,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: Vec::new(),
            observed_revision: 0,
        }
    }

    fn clipboard_snapshot(&self, _: &OperationContext) -> ServiceResult<ClipboardSnapshot> {
        Err(SystemError::new(
            SystemErrorCode::Unsupported,
            "Rich clipboard access is unsupported on this target",
        ))
    }

    fn publish_clipboard(
        &self,
        _: &ClipboardHandoff,
    ) -> Result<ProviderClipboardGrant, SystemError> {
        Err(SystemError::new(
            SystemErrorCode::Unsupported,
            "Rich clipboard publication is unsupported on this target",
        ))
    }

    fn start_drag(&self, _: &ProviderDragRequest) -> Result<ProviderDragGrant, SystemError> {
        Err(SystemError::new(
            SystemErrorCode::Unsupported,
            "Native drag sessions are unsupported on this target",
        ))
    }

    fn cancel_drag(&self, _: u64) -> bool {
        false
    }

    fn attach_listener(&self, _: Weak<dyn RichTransferProviderListener>) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn capability_for(
    platform: crate::platform::PlatformTarget,
    operation: crate::platform::DndOperation,
    revision: u64,
) -> Capability<crate::platform::DndOperation> {
    let support = platform.support(operation);
    let supported = matches!(support, Support::Native | Support::Conditional);
    Capability {
        operation,
        support,
        availability: if supported {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: revision,
    }
}

/// The event delivered to a fake provider's mid-publication hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakePublishEvent {
    pub handoff_id: HandoffId,
    pub native_handoff_token: u64,
    pub sequence: Option<u64>,
}

pub type FakePublishHook = Rc<dyn Fn(&FakePublishEvent)>;

/// The event delivered to a fake provider's mid-drag-start hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeDragStartEvent {
    pub session_id: DragSessionId,
    pub native_session_token: u64,
    pub session_epoch: u64,
}

pub type FakeDragStartHook = Rc<dyn Fn(&FakeDragStartEvent)>;

/// One hosted native drag observed by the fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeHostedDrag {
    pub native_session_token: u64,
    pub session_epoch: u64,
    pub request: ProviderDragRequest,
}

/// One incoming external drag simulated by the fake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeIncomingDrag {
    pub native_session_token: u64,
    pub surface: SurfaceId,
    pub generation: u64,
    pub point: DragPoint,
    pub offers: Vec<PayloadOffer>,
}

struct FakeState {
    platform: crate::platform::PlatformTarget,
    capabilities:
        BTreeMap<crate::platform::DndOperation, Capability<crate::platform::DndOperation>>,
    clipboard: Vec<ClipboardEntry>,
    clipboard_sequence: u64,
    publish_calls: Vec<ClipboardHandoff>,
    publish_grants: Vec<ProviderClipboardGrant>,
    released_delayed: Vec<u64>,
    start_calls: Vec<ProviderDragRequest>,
    start_grants: Vec<ProviderDragGrant>,
    handoff_tokens: BTreeMap<u64, HandoffId>,
    active_drags: BTreeMap<u64, FakeHostedDrag>,
    cancel_calls: Vec<u64>,
    complete_calls: Vec<(u64, ProviderDropOutcome)>,
    incoming_drags: Vec<FakeIncomingDrag>,
    listeners: Vec<Weak<dyn RichTransferProviderListener>>,
    next_token: u64,
    snapshot_failure: Option<SystemError>,
    publish_failure: Option<SystemError>,
    start_failure: Option<SystemError>,
    complete_failure: Option<SystemError>,
    on_publish: Option<FakePublishHook>,
    on_start_drag: Option<FakeDragStartHook>,
}

/// Deterministic provider for headless tests and examples.
///
/// The handle is `Clone` and interior-mutable, so tests can drive transitions
/// while the service owns its `Rc<dyn RichTransferProvider>`. Hooks and listener
/// notifications run outside the fake's state borrow.
#[derive(Clone)]
pub struct FakeRichTransferProvider {
    state: Rc<RefCell<FakeState>>,
}

fn borrow<T>(value: &RefCell<T>) -> std::cell::RefMut<'_, T> {
    value.borrow_mut()
}

impl Default for FakeRichTransferProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeRichTransferProvider {
    pub fn new() -> Self {
        Self::with_platform(
            crate::platform::PlatformTarget::current()
                .unwrap_or(crate::platform::PlatformTarget::Linux),
        )
    }

    pub fn with_platform(platform: crate::platform::PlatformTarget) -> Self {
        let capabilities = crate::platform::DndOperation::ALL
            .into_iter()
            .map(|operation| (operation, capability_for(platform, operation, 0)))
            .collect();
        Self {
            state: Rc::new(RefCell::new(FakeState {
                platform,
                capabilities,
                clipboard: Vec::new(),
                clipboard_sequence: 0,
                publish_calls: Vec::new(),
                publish_grants: Vec::new(),
                released_delayed: Vec::new(),
                start_calls: Vec::new(),
                start_grants: Vec::new(),
                handoff_tokens: BTreeMap::new(),
                active_drags: BTreeMap::new(),
                cancel_calls: Vec::new(),
                complete_calls: Vec::new(),
                incoming_drags: Vec::new(),
                listeners: Vec::new(),
                next_token: 1,
                snapshot_failure: None,
                publish_failure: None,
                start_failure: None,
                complete_failure: None,
                on_publish: None,
                on_start_drag: None,
            })),
        }
    }

    pub fn platform(&self) -> crate::platform::PlatformTarget {
        borrow(&self.state).platform
    }

    fn notify(&self, notice: RichTransferProviderNotice) {
        let listeners = {
            let mut state = borrow(&self.state);
            state
                .listeners
                .retain(|listener| listener.strong_count() > 0);
            state
                .listeners
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for listener in listeners {
            listener.on_rich_transfer_notice(notice.clone());
        }
    }

    pub fn set_capability(
        &self,
        operation: crate::platform::DndOperation,
        capability: Capability<crate::platform::DndOperation>,
    ) {
        borrow(&self.state)
            .capabilities
            .insert(operation, capability);
    }

    pub fn set_support(&self, operation: crate::platform::DndOperation, support: Support) {
        let mut state = borrow(&self.state);
        if let Some(capability) = state.capabilities.get_mut(&operation) {
            capability.support = support;
            capability.availability = if matches!(support, Support::Native | Support::Conditional) {
                Availability::Available
            } else {
                Availability::Unavailable
            };
        }
    }

    pub fn set_availability(
        &self,
        operation: crate::platform::DndOperation,
        availability: Availability,
    ) {
        if let Some(capability) = borrow(&self.state).capabilities.get_mut(&operation) {
            capability.availability = availability;
        }
    }

    pub fn set_configuration(
        &self,
        operation: crate::platform::DndOperation,
        configuration: Configuration,
    ) {
        if let Some(capability) = borrow(&self.state).capabilities.get_mut(&operation) {
            capability.configuration = configuration;
        }
    }

    pub fn set_snapshot_failure(&self, failure: Option<SystemError>) {
        borrow(&self.state).snapshot_failure = failure;
    }

    pub fn set_publish_failure(&self, failure: Option<SystemError>) {
        borrow(&self.state).publish_failure = failure;
    }

    pub fn set_start_failure(&self, failure: Option<SystemError>) {
        borrow(&self.state).start_failure = failure;
    }

    pub fn set_complete_failure(&self, failure: Option<SystemError>) {
        borrow(&self.state).complete_failure = failure;
    }

    pub fn set_on_publish(&self, hook: Option<FakePublishHook>) {
        borrow(&self.state).on_publish = hook;
    }

    pub fn with_on_publish(self, hook: FakePublishHook) -> Self {
        self.set_on_publish(Some(hook));
        self
    }

    pub fn set_on_start_drag(&self, hook: Option<FakeDragStartHook>) {
        borrow(&self.state).on_start_drag = hook;
    }

    pub fn with_on_start_drag(self, hook: FakeDragStartHook) -> Self {
        self.set_on_start_drag(Some(hook));
        self
    }

    /// Replace the hosted clipboard with explicit entries.
    pub fn set_clipboard(&self, entries: impl IntoIterator<Item = ClipboardEntry>) {
        let mut state = borrow(&self.state);
        state.clipboard = entries.into_iter().collect();
        state.clipboard_sequence = state.clipboard_sequence.saturating_add(1);
        drop(state);
        self.notify(RichTransferProviderNotice::ClipboardChanged);
    }

    /// Simulate an external application publishing plain text.
    pub fn set_external_plain_text(&self, text: &str) -> Result<(), SystemError> {
        let payload = TypedPayload::plain_text(text)?;
        self.set_clipboard([ClipboardEntry::Eager(payload)]);
        Ok(())
    }

    /// Simulate an external application publishing opaque file resources.
    pub fn set_external_files(
        &self,
        files: impl IntoIterator<Item = gpui_files::FileResource>,
    ) -> Result<(), SystemError> {
        let payload = TypedPayload::files(files)?;
        self.set_clipboard([ClipboardEntry::Eager(payload)]);
        Ok(())
    }

    pub fn set_external_html(&self, html: &str) -> Result<(), SystemError> {
        let payload = TypedPayload::html(html)?;
        self.set_clipboard([ClipboardEntry::Eager(payload)]);
        Ok(())
    }

    pub fn clear_clipboard(&self) {
        let mut state = borrow(&self.state);
        state.clipboard.clear();
        state.clipboard_sequence = state.clipboard_sequence.saturating_add(1);
        drop(state);
        self.notify(RichTransferProviderNotice::ClipboardChanged);
    }

    pub fn clipboard_entries(&self) -> Vec<ClipboardEntry> {
        borrow(&self.state).clipboard.clone()
    }

    pub fn clipboard_offers(&self) -> Vec<PayloadOffer> {
        borrow(&self.state)
            .clipboard
            .iter()
            .map(ClipboardEntry::offer)
            .collect()
    }

    pub fn publish_calls(&self) -> Vec<ClipboardHandoff> {
        borrow(&self.state).publish_calls.clone()
    }

    pub fn publish_grants(&self) -> Vec<ProviderClipboardGrant> {
        borrow(&self.state).publish_grants.clone()
    }

    pub fn publish_call_count(&self) -> usize {
        borrow(&self.state).publish_calls.len()
    }

    pub fn released_delayed_tokens(&self) -> Vec<u64> {
        borrow(&self.state).released_delayed.clone()
    }

    pub fn start_drag_calls(&self) -> Vec<ProviderDragRequest> {
        borrow(&self.state).start_calls.clone()
    }

    pub fn start_drag_grants(&self) -> Vec<ProviderDragGrant> {
        borrow(&self.state).start_grants.clone()
    }

    pub fn start_drag_call_count(&self) -> usize {
        borrow(&self.state).start_calls.len()
    }

    pub fn active_drags(&self) -> Vec<FakeHostedDrag> {
        borrow(&self.state).active_drags.values().cloned().collect()
    }

    pub fn active_drag_tokens(&self) -> Vec<u64> {
        borrow(&self.state).active_drags.keys().copied().collect()
    }

    pub fn cancel_drag_calls(&self) -> Vec<u64> {
        borrow(&self.state).cancel_calls.clone()
    }

    pub fn complete_drag_calls(&self) -> Vec<(u64, ProviderDropOutcome)> {
        borrow(&self.state).complete_calls.clone()
    }

    pub fn incoming_drags(&self) -> Vec<FakeIncomingDrag> {
        borrow(&self.state).incoming_drags.clone()
    }

    /// Simulate an external drag entering a target surface.
    pub fn simulate_external_drag_enter(
        &self,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        offers: impl IntoIterator<Item = PayloadOffer>,
    ) -> u64 {
        let token = {
            let mut state = borrow(&self.state);
            let token = state.next_token;
            state.next_token = state.next_token.saturating_add(1);
            token
        };
        self.simulate_external_drag_enter_with_token(token, surface, generation, point, offers)
    }

    /// Simulate an external drag with a caller-selected native token. Test-only
    /// callers use this to exercise zero/unknown-token rejection paths.
    pub fn simulate_external_drag_enter_with_token(
        &self,
        native_session_token: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        offers: impl IntoIterator<Item = PayloadOffer>,
    ) -> u64 {
        let offers = offers.into_iter().collect::<Vec<_>>();
        {
            let mut state = borrow(&self.state);
            state.incoming_drags.push(FakeIncomingDrag {
                native_session_token,
                surface,
                generation,
                point,
                offers: offers.clone(),
            });
        }
        self.notify(RichTransferProviderNotice::ExternalDragEntered {
            native_session_token,
            surface,
            generation,
            point,
            offers,
        });
        native_session_token
    }

    pub fn simulate_external_drag_leave(&self, native_session_token: u64) {
        self.notify(RichTransferProviderNotice::ExternalDragLeft {
            native_session_token,
        });
    }

    pub fn simulate_external_drop(&self, native_session_token: u64, point: DragPoint) {
        self.notify(RichTransferProviderNotice::ExternalDrop {
            native_session_token,
            point,
        });
    }

    /// Simulate the OS canceling a native session (source exit, lock screen,
    /// external app crash). The service must revoke delayed promises and release
    /// producer resources.
    pub fn simulate_session_cancelled(&self, native_session_token: u64) {
        self.notify(RichTransferProviderNotice::SessionCancelled {
            native_session_token,
        });
    }

    pub fn simulate_source_exit(&self) {
        self.notify(RichTransferProviderNotice::SourceExited);
    }
}

impl RichTransferProvider for FakeRichTransferProvider {
    fn platform(&self) -> crate::platform::PlatformTarget {
        FakeRichTransferProvider::platform(self)
    }

    fn capability(
        &self,
        operation: crate::platform::DndOperation,
    ) -> Capability<crate::platform::DndOperation> {
        let state = borrow(&self.state);
        state
            .capabilities
            .get(&operation)
            .cloned()
            .unwrap_or_else(|| capability_for(state.platform, operation, 0))
    }

    fn clipboard_snapshot(&self, _: &OperationContext) -> ServiceResult<ClipboardSnapshot> {
        let state = borrow(&self.state);
        if let Some(failure) = state.snapshot_failure.clone() {
            return Err(failure);
        }
        Ok(gpui_system::Completion::Completed(ClipboardSnapshot::new(
            state.clipboard.clone(),
        )))
    }

    fn publish_clipboard(
        &self,
        request: &ClipboardHandoff,
    ) -> Result<ProviderClipboardGrant, SystemError> {
        let (token, sequence, hook) = {
            let mut state = borrow(&self.state);
            state.publish_calls.push(request.clone());
            if let Some(failure) = state.publish_failure.clone() {
                return Err(failure);
            }
            let token = state.next_token;
            state.next_token = state.next_token.saturating_add(1);
            let sequence = Some(state.clipboard_sequence.saturating_add(1));
            let hook = state.on_publish.clone();
            (token, sequence, hook)
        };
        if let Some(hook) = hook {
            hook(&FakePublishEvent {
                handoff_id: request.handoff_id,
                native_handoff_token: token,
                sequence,
            });
        }
        {
            let mut state = borrow(&self.state);
            state.clipboard_sequence = state.clipboard_sequence.saturating_add(1);
            state.clipboard = request
                .eager
                .iter()
                .cloned()
                .map(ClipboardEntry::Eager)
                .chain(request.delayed.iter().cloned().map(|representation| {
                    ClipboardEntry::Delayed {
                        representation,
                        handoff_id: Some(request.handoff_id),
                    }
                }))
                .collect();
            let grant = ProviderClipboardGrant::new(token, sequence);
            state.publish_grants.push(grant);
            state.handoff_tokens.insert(token, request.handoff_id);
            Ok(grant)
        }
    }

    fn release_delayed(&self, native_handoff_token: u64) -> bool {
        let mut state = borrow(&self.state);
        state.released_delayed.push(native_handoff_token);
        let Some(handoff_id) = state.handoff_tokens.get(&native_handoff_token).copied() else {
            return false;
        };
        let before = state.clipboard.len();
        state.clipboard.retain(|entry| match entry {
            ClipboardEntry::Delayed {
                handoff_id: Some(id),
                ..
            } => *id != handoff_id,
            _ => true,
        });
        state.clipboard.len() != before
    }

    fn start_drag(&self, request: &ProviderDragRequest) -> Result<ProviderDragGrant, SystemError> {
        let (token, epoch, hook) = {
            let mut state = borrow(&self.state);
            state.start_calls.push(request.clone());
            if let Some(failure) = state.start_failure.clone() {
                return Err(failure);
            }
            let token = state.next_token;
            state.next_token = state.next_token.saturating_add(1);
            let epoch = state.start_calls.len() as u64;
            let hook = state.on_start_drag.clone();
            (token, epoch, hook)
        };
        if let Some(hook) = hook {
            hook(&FakeDragStartEvent {
                session_id: request.session_id,
                native_session_token: token,
                session_epoch: epoch,
            });
        }
        let grant = ProviderDragGrant::new(token, epoch);
        {
            let mut state = borrow(&self.state);
            state.active_drags.insert(
                token,
                FakeHostedDrag {
                    native_session_token: token,
                    session_epoch: epoch,
                    request: request.clone(),
                },
            );
            state.start_grants.push(grant);
        }
        Ok(grant)
    }

    fn complete_drag(
        &self,
        native_session_token: u64,
        outcome: &ProviderDropOutcome,
    ) -> Result<(), SystemError> {
        let mut state = borrow(&self.state);
        if let Some(failure) = state.complete_failure.clone() {
            return Err(failure);
        }
        state
            .complete_calls
            .push((native_session_token, outcome.clone()));
        state.active_drags.remove(&native_session_token);
        Ok(())
    }

    fn cancel_drag(&self, native_session_token: u64) -> bool {
        let mut state = borrow(&self.state);
        state.cancel_calls.push(native_session_token);
        state.active_drags.remove(&native_session_token).is_some()
    }

    fn attach_listener(&self, listener: Weak<dyn RichTransferProviderListener>) {
        let mut state = borrow(&self.state);
        state
            .listeners
            .retain(|existing| existing.strong_count() > 0);
        state.listeners.push(listener);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl fmt::Debug for FakeRichTransferProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = borrow(&self.state);
        formatter
            .debug_struct("FakeRichTransferProvider")
            .field("platform", &state.platform)
            .field("clipboard_entries", &state.clipboard.len())
            .field("publish_calls", &state.publish_calls.len())
            .field("active_drags", &state.active_drags.len())
            .field("incoming_drags", &state.incoming_drags.len())
            .finish()
    }
}

/// Convenience alias used by examples that only need file counting.
pub const FAKE_PROVIDER_MAX_FILES: usize = MAX_TRANSFERRED_FILES;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::FormatDescriptor;
    use gpui_system::Completion;

    #[test]
    fn fake_provider_hosts_clipboard_and_releases_only_delayed_entries() {
        let fake = FakeRichTransferProvider::new();
        fake.set_external_plain_text("external").expect("text");
        assert_eq!(fake.clipboard_entries().len(), 1);
        let snapshot = match fake.clipboard_snapshot(&OperationContext::default()) {
            Ok(Completion::Completed(snapshot)) => snapshot,
            other => panic!("unexpected snapshot: {other:?}"),
        };
        assert_eq!(snapshot.entries.len(), 1);
        assert_eq!(
            snapshot.entries[0].descriptor(),
            FormatDescriptor::plain_text()
        );

        let representation = LazyRepresentation::new(FormatDescriptor::html(), |_| {
            crate::error::completed(TypedPayload::html("<b>x</b>").expect("html"))
        });
        let handoff = ClipboardHandoff {
            handoff_id: HandoffId(1),
            scope: HandoffScope::Application,
            eager: vec![TypedPayload::plain_text("eager").expect("text")],
            delayed: vec![representation],
        };
        let grant = fake.publish_clipboard(&handoff).expect("publish");
        assert_eq!(grant.sequence, Some(2));
        assert_eq!(fake.clipboard_entries().len(), 2);
        assert!(fake.release_delayed(grant.native_handoff_token));
        let entries = fake.clipboard_entries();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], ClipboardEntry::Eager(_)));
    }

    #[test]
    fn fake_provider_grants_drags_and_tracks_cancel_and_complete() {
        let fake = FakeRichTransferProvider::new();
        let request = ProviderDragRequest {
            session_id: DragSessionId(1),
            source_surface: SurfaceId(2),
            source_generation: 1,
            allowed_effects: AllowedEffects::all(),
            preview: DragPreview::new("preview").expect("preview"),
            offers: vec![PayloadOffer::new(
                FormatDescriptor::plain_text(),
                DeliveryMode::Eager,
            )],
            delayed_formats: Vec::new(),
            promised_files: 0,
        };
        let grant = fake.start_drag(&request).expect("grant");
        assert_eq!(fake.active_drag_tokens(), vec![grant.native_session_token]);
        let outcome = ProviderDropOutcome {
            session_id: DragSessionId(1),
            target_id: DropTargetId(1),
            format: FormatDescriptor::plain_text(),
            effect: crate::model::DragEffect::Copy,
        };
        fake.complete_drag(grant.native_session_token, &outcome)
            .expect("complete");
        assert!(fake.active_drag_tokens().is_empty());
        assert_eq!(fake.complete_drag_calls().len(), 1);
        assert!(!fake.cancel_drag(grant.native_session_token));
    }

    #[test]
    fn unsupported_provider_never_fabricates_state() {
        let provider = UnsupportedRichTransferProvider::default();
        assert!(provider.capabilities().iter().all(|capability| {
            capability.support == Support::Unsupported
                && capability.availability == Availability::Unavailable
        }));
        assert!(
            provider
                .clipboard_snapshot(&OperationContext::default())
                .is_err()
        );
        assert!(
            provider
                .start_drag(&ProviderDragRequest {
                    session_id: DragSessionId(1),
                    source_surface: SurfaceId(1),
                    source_generation: 1,
                    allowed_effects: AllowedEffects::all(),
                    preview: DragPreview::new("").expect("preview"),
                    offers: Vec::new(),
                    delayed_formats: Vec::new(),
                    promised_files: 0,
                })
                .is_err()
        );
    }
}
