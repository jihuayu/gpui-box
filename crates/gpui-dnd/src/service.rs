//! Capability-gated rich clipboard/drag service with scoped RAII leases,
//! reentrancy-safe callbacks, TOCTOU re-validation and bounded events.

use crate::error::{
    busy, completed, context_expired, expired, invalid, permission_denied, unsupported,
};
use crate::format::{
    DeliveryMode, FormatDescriptor, NegotiatedFormat, PayloadOffer, negotiate_format,
};
use crate::model::{
    AllowedEffects, DragEffect, DragModifiers, DragPoint, DragSessionId, DragSourceSpec,
    DropEvaluationContext, DropTargetId, DropTargetSpec, DropTargetVerdict, HandoffId,
    SourceRepresentation, TypedPayload,
};
use crate::platform::{DndOperation, PlatformTarget};
use crate::provider::{
    ClipboardEntry, ClipboardHandoff, FakeRichTransferProvider, ProviderDragRequest,
    ProviderDropOutcome, RichTransferProvider, RichTransferProviderListener,
    RichTransferProviderNotice,
};
use crate::stream::{
    ClipboardReadOutcome, ConsumedPayload, HandoffLease, HandoffReceipt, HandoffRevokeReason,
    HandoffScope, LazyRepresentation, RichClipboardWrite, StreamBudget, TransferInteraction,
};
use gpui_lifecycle::{AppPhase, LifecycleState};
use gpui_system::{
    Capability, Completion, ErrorCode, EventSender, EventSubscription, OperationContext,
    ServiceResult, SurfaceId, SystemError, UiContext, event_channel,
};
use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Default bounded capacity of one rich-transfer event stream.
pub const DEFAULT_DND_EVENT_CAPACITY: usize = 64;

type Subscriber = (Rc<AtomicBool>, EventSender<DndEvent>);
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Why a drag session was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DragCancelReason {
    User,
    PointerCancel,
    ForbiddenTarget,
    SourceSurfaceDisposed,
    TargetSurfaceDisposed,
    AppBackgrounded,
    AppTerminating,
    SourceExited,
    LeaseDropped,
    ProviderCancelled,
    CallerCancelled,
}

impl DragCancelReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::PointerCancel => "pointer_cancel",
            Self::ForbiddenTarget => "forbidden_target",
            Self::SourceSurfaceDisposed => "source_surface_disposed",
            Self::TargetSurfaceDisposed => "target_surface_disposed",
            Self::AppBackgrounded => "app_backgrounded",
            Self::AppTerminating => "app_terminating",
            Self::SourceExited => "source_exited",
            Self::LeaseDropped => "lease_dropped",
            Self::ProviderCancelled => "provider_cancelled",
            Self::CallerCancelled => "caller_cancelled",
        }
    }
}

/// Why a provider/service callback was rejected as stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StaleCallbackReason {
    UnknownSession,
    UnknownTarget,
    TokenMismatch,
    EpochMismatch,
    SessionCompleted,
    SessionCancelled,
    SurfaceNotLive,
    GenerationMismatch,
    HandoffRevoked,
    ContextExpired,
}

impl StaleCallbackReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::UnknownSession => "unknown_session",
            Self::UnknownTarget => "unknown_target",
            Self::TokenMismatch => "token_mismatch",
            Self::EpochMismatch => "epoch_mismatch",
            Self::SessionCompleted => "session_completed",
            Self::SessionCancelled => "session_cancelled",
            Self::SurfaceNotLive => "surface_not_live",
            Self::GenerationMismatch => "generation_mismatch",
            Self::HandoffRevoked => "handoff_revoked",
            Self::ContextExpired => "context_expired",
        }
    }
}

/// The terminal outcome of a valid drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropOutcome {
    pub session_id: DragSessionId,
    pub target_id: DropTargetId,
    pub effect: DragEffect,
    pub format: FormatDescriptor,
}

/// A non-terminal drag hover result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragUpdate {
    pub session_id: DragSessionId,
    pub target_id: Option<DropTargetId>,
    pub format: Option<FormatDescriptor>,
    pub effect: Option<DragEffect>,
    /// Present when a stale token/epoch or terminal session was rejected.
    pub stale: Option<StaleCallbackReason>,
}

impl DragUpdate {
    pub fn ignored(session_id: DragSessionId, reason: StaleCallbackReason) -> Self {
        Self {
            session_id,
            target_id: None,
            format: None,
            effect: None,
            stale: Some(reason),
        }
    }

    pub fn is_accepted(&self) -> bool {
        self.stale.is_none() && self.target_id.is_some() && self.effect.is_some()
    }

    pub fn is_stale(&self) -> bool {
        self.stale.is_some()
    }
}

/// The terminal result of `complete_drop`.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum DropCompletion {
    Dropped {
        outcome: DropOutcome,
        payload: ConsumedPayload,
    },
    Cancelled(DragCancelReason),
    Ignored(StaleCallbackReason),
}

impl DropCompletion {
    pub fn outcome(&self) -> Option<&DropOutcome> {
        match self {
            Self::Dropped { outcome, .. } => Some(outcome),
            _ => None,
        }
    }

    pub fn payload(&self) -> Option<&ConsumedPayload> {
        match self {
            Self::Dropped { payload, .. } => Some(payload),
            _ => None,
        }
    }

    pub const fn is_dropped(&self) -> bool {
        matches!(self, Self::Dropped { .. })
    }
}

/// The result of an explicit or automatic drag cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragCancelOutcome {
    Cancelled(DragCancelReason),
    AlreadyTerminal,
    Ignored(StaleCallbackReason),
}

/// Public snapshot of one drag session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DragSessionStatus {
    Active,
    Completed(DropOutcome),
    Cancelled(DragCancelReason),
}

/// Public-safe rich-transfer observations. Producers never block on consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DndEvent {
    HandoffPublished {
        handoff_id: HandoffId,
        sequence: Option<u64>,
        eager_formats: Vec<FormatDescriptor>,
        delayed_formats: Vec<FormatDescriptor>,
    },
    HandoffRevoked {
        handoff_id: HandoffId,
        reason: HandoffRevokeReason,
    },
    ClipboardChanged,
    ClipboardEmpty,
    ClipboardNoMatchingFormat {
        available_count: usize,
    },
    ClipboardRead {
        format: FormatDescriptor,
        delivery: DeliveryMode,
    },
    DropTargetRegistered {
        target_id: DropTargetId,
        surface: SurfaceId,
        generation: u64,
    },
    DropTargetRemoved {
        target_id: DropTargetId,
    },
    DragStarted {
        session_id: DragSessionId,
        source_surface: SurfaceId,
        generation: u64,
        format_count: usize,
    },
    DragEntered {
        session_id: DragSessionId,
        target_id: DropTargetId,
        format: FormatDescriptor,
        effect: DragEffect,
    },
    DragOver {
        session_id: DragSessionId,
        target_id: Option<DropTargetId>,
        format: Option<FormatDescriptor>,
        effect: Option<DragEffect>,
    },
    DragLeft {
        session_id: DragSessionId,
        target_id: Option<DropTargetId>,
    },
    DropCompleted {
        session_id: DragSessionId,
        target_id: DropTargetId,
        effect: DragEffect,
        format: FormatDescriptor,
    },
    DragCancelled {
        session_id: DragSessionId,
        reason: DragCancelReason,
    },
    StaleCallbackIgnored {
        session: Option<DragSessionId>,
        reason: StaleCallbackReason,
    },
    ExternalDragEntered {
        native_session_token: u64,
        surface: SurfaceId,
        generation: u64,
        formats: Vec<FormatDescriptor>,
    },
    ExternalDragLeft {
        native_session_token: u64,
    },
    ExternalDrop {
        native_session_token: u64,
        point: DragPoint,
    },
    SurfaceDisposed {
        surface: SurfaceId,
        generation: u64,
    },
    OperationFailed {
        operation: DndOperation,
        code: ErrorCode,
    },
}

/// A bounded drain of a [`DndEntityModel`](crate::gpui_adapter::DndEntityModel).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DndEventDrain {
    pub resync_required: bool,
    pub events: Vec<DndEvent>,
}

#[derive(Clone, Copy)]
struct SurfaceRecord {
    generation: u64,
    visible: bool,
    live: bool,
}

struct HandoffRecord {
    id: HandoffId,
    scope: HandoffScope,
    native_token: Option<u64>,
    sequence: Option<u64>,
    eager: Vec<TypedPayload>,
    delayed: Vec<LazyRepresentation>,
    revoked: bool,
}

impl HandoffRecord {
    fn receipt(&self) -> HandoffReceipt {
        HandoffReceipt {
            handoff_id: self.id,
            sequence: self.sequence,
            eager_formats: self.eager.iter().map(TypedPayload::descriptor).collect(),
            delayed_formats: self
                .delayed
                .iter()
                .map(|delayed| delayed.descriptor().clone())
                .collect(),
        }
    }
}

#[derive(Clone)]
struct TargetRecord {
    id: DropTargetId,
    spec: DropTargetSpec,
}

#[derive(Clone)]
struct DragRecord {
    id: DragSessionId,
    native_token: u64,
    epoch: u64,
    source_surface: SurfaceId,
    source_generation: u64,
    source_effects: AllowedEffects,
    representations: Vec<SourceRepresentation>,
    context: OperationContext,
    state: DragSessionStatus,
    current_target: Option<DropTargetId>,
    current_format: Option<FormatDescriptor>,
    current_effect: Option<DragEffect>,
}

impl DragRecord {
    fn is_active(&self) -> bool {
        matches!(self.state, DragSessionStatus::Active)
    }

    fn offers(&self) -> Vec<PayloadOffer> {
        self.representations
            .iter()
            .map(|representation| {
                let mut offer =
                    PayloadOffer::new(representation.descriptor(), representation.delivery());
                if let SourceRepresentation::Eager(payload) = representation {
                    offer = offer.with_estimated_bytes(payload.byte_len());
                }
                offer
            })
            .collect()
    }
}

struct ServiceState {
    phase: AppPhase,
    surfaces: HashMap<SurfaceId, SurfaceRecord>,
    handoffs: BTreeMap<HandoffId, HandoffRecord>,
    targets: BTreeMap<DropTargetId, TargetRecord>,
    drag: Option<DragRecord>,
    external_drags: BTreeMap<u64, (SurfaceId, u64)>,
    next_handoff: u64,
    next_target: u64,
    next_session: u64,
    subscribers: Vec<Subscriber>,
    stale_callback_count: u64,
    last_stale_callback: Option<StaleCallbackReason>,
    external_drag_notices: u64,
}

pub(crate) struct ServiceInner {
    provider: Rc<dyn RichTransferProvider>,
    state: Mutex<ServiceState>,
    identity: Rc<()>,
    event_capacity: usize,
    budget: Arc<StreamBudget>,
    ui: Option<UiContext>,
}

impl ServiceInner {
    fn emit(&self, event: DndEvent) {
        let senders = {
            let mut state = lock(&self.state);
            state
                .subscribers
                .retain(|(live, _)| live.load(Ordering::Acquire));
            state
                .subscribers
                .iter()
                .map(|(_, sender)| sender.clone())
                .collect::<Vec<_>>()
        };
        for sender in senders {
            sender.send(event.clone());
        }
    }

    fn emit_failure(&self, operation: DndOperation, error: &SystemError) {
        self.emit(DndEvent::OperationFailed {
            operation,
            code: error.code,
        });
    }

    fn fail<T>(&self, operation: DndOperation, error: SystemError) -> ServiceResult<T> {
        self.emit_failure(operation, &error);
        Err(error)
    }

    fn record_stale(&self, session: Option<DragSessionId>, reason: StaleCallbackReason) {
        {
            let mut state = lock(&self.state);
            state.stale_callback_count = state.stale_callback_count.saturating_add(1);
            state.last_stale_callback = Some(reason);
        }
        self.emit(DndEvent::StaleCallbackIgnored { session, reason });
    }

    fn phase(&self) -> AppPhase {
        lock(&self.state).phase
    }

    fn check_phase(&self) -> Result<(), SystemError> {
        match self.phase() {
            AppPhase::ForegroundActive | AppPhase::ForegroundInactive => Ok(()),
            AppPhase::Terminating => Err(context_expired(
                "The application is terminating; transfer operations are revoked",
            )),
            AppPhase::Background | AppPhase::Suspended | AppPhase::Launching => Err(
                permission_denied("A foreground, user-initiated application phase is required"),
            ),
        }
    }

    fn surface_live_locked(state: &ServiceState, surface: SurfaceId, generation: u64) -> bool {
        match state.surfaces.get(&surface) {
            Some(record) => record.live && record.generation == generation,
            // A host may register resources before attaching a lifecycle
            // snapshot. Explicit dispose still revokes them.
            None => true,
        }
    }

    fn surface_visible_locked(state: &ServiceState, surface: SurfaceId, generation: u64) -> bool {
        match state.surfaces.get(&surface) {
            Some(record) => record.live && record.generation == generation && record.visible,
            None => true,
        }
    }

    fn surface_live(&self, surface: SurfaceId, generation: u64) -> bool {
        Self::surface_live_locked(&lock(&self.state), surface, generation)
    }

    fn check_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
        require_visible: bool,
    ) -> Result<(), SystemError> {
        if surface == SurfaceId(0) || generation == 0 {
            return Err(invalid(
                "A transfer scope requires a nonzero surface and generation",
            ));
        }
        let state = lock(&self.state);
        if !Self::surface_live_locked(&state, surface, generation) {
            return Err(context_expired(
                "The transfer surface generation is not live",
            ));
        }
        if require_visible && !Self::surface_visible_locked(&state, surface, generation) {
            return Err(permission_denied(
                "A visible foreground surface is required for this transfer",
            ));
        }
        Ok(())
    }

    /// Explicit surface scopes must be attached to the service ledger. A UI
    /// context alone is sufficient for a service-level gesture, but an explicit
    /// `(surface, generation)` is rejected unless the host registered it.
    fn require_surface_attached(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), SystemError> {
        if surface == SurfaceId(0) || generation == 0 {
            return Err(invalid(
                "A transfer scope requires a nonzero surface and generation",
            ));
        }
        let state = lock(&self.state);
        match state.surfaces.get(&surface) {
            Some(record) if record.live && record.generation == generation => Ok(()),
            Some(_) => Err(context_expired(
                "The transfer surface generation is not attached or is stale",
            )),
            None => Err(context_expired("The transfer surface is not attached")),
        }
    }

    fn gate_capability(&self, operation: DndOperation) -> Result<(), SystemError> {
        if operation == DndOperation::ProvideDelayedPayload
            && self.provider.platform() == PlatformTarget::Web
        {
            return Err(unsupported(
                "Browsers cannot register persistent delayed OS clipboard callbacks",
            ));
        }
        self.provider.capability(operation).check()
    }

    fn validate_interaction(
        &self,
        interaction: &TransferInteraction,
        operation: DndOperation,
    ) -> Result<InteractionBinding, SystemError> {
        if !interaction.identity_matches(&self.identity) {
            return Err(invalid(
                "Transfer interaction belongs to another rich-transfer service",
            ));
        }
        interaction.context().check_at(Instant::now())?;
        if interaction.context().ui.is_some() {
            interaction.context().require_ui()?;
        }
        self.check_phase()?;
        self.gate_capability(operation)?;
        let explicit = interaction.surface();
        let ui_surface = interaction
            .context()
            .ui
            .as_ref()
            .map(|ui| (ui.surface, ui.generation));
        if let (Some((explicit_surface, explicit_generation)), Some((ui_surface, ui_generation))) =
            (explicit, ui_surface)
            && (explicit_surface != ui_surface || explicit_generation != ui_generation)
        {
            return Err(invalid(
                "Transfer interaction surface does not match its live UI attachment",
            ));
        }
        if let Some((surface, generation)) = explicit {
            self.require_surface_attached(surface, generation)?;
        }
        let surface = explicit.or(ui_surface);
        if let Some((surface, generation)) = surface {
            self.check_surface(surface, generation, true)?;
        } else {
            return Err(invalid(
                "Transfer interaction has no live surface or UI attachment",
            ));
        }
        Ok(InteractionBinding {
            surface,
            context: interaction.context().clone(),
        })
    }

    fn interaction_cancelled(
        &self,
        interaction: &TransferInteraction,
    ) -> Option<gpui_system::CancelReason> {
        interaction.cancellation()
    }

    pub(crate) fn handoff_receipt(&self, id: HandoffId) -> Option<HandoffReceipt> {
        lock(&self.state)
            .handoffs
            .get(&id)
            .map(HandoffRecord::receipt)
    }

    pub(crate) fn handoff_is_active(&self, id: HandoffId) -> bool {
        lock(&self.state).handoffs.contains_key(&id)
    }

    pub(crate) fn handoff_delayed_revoked(&self, id: HandoffId) -> bool {
        lock(&self.state)
            .handoffs
            .get(&id)
            .is_none_or(|record| record.revoked)
    }

    pub(crate) fn revoke_handoff_delayed(
        &self,
        id: HandoffId,
        reason: HandoffRevokeReason,
    ) -> bool {
        let (delayed, token) = {
            let mut state = lock(&self.state);
            let Some(record) = state.handoffs.get_mut(&id) else {
                return false;
            };
            if record.revoked || record.delayed.is_empty() {
                record.revoked = true;
                return false;
            }
            record.revoked = true;
            (std::mem::take(&mut record.delayed), record.native_token)
        };
        for representation in &delayed {
            representation.release_promised_files();
        }
        if let Some(token) = token {
            let _ = self.provider.release_delayed(token);
        }
        self.emit(DndEvent::HandoffRevoked {
            handoff_id: id,
            reason,
        });
        true
    }

    /// Remove a provisional handoff after a failed or cancelled publication.
    ///
    /// If a re-entrant provider hook already removed the pre-grant record, the
    /// hook's purge released the promised files but could not know the native
    /// token. In that case only the native delayed entry is released here; the
    /// promised files are not released a second time.
    fn discard_failed_handoff(
        &self,
        id: HandoffId,
        published_token: Option<u64>,
        had_delayed: bool,
        revoked_reason: Option<HandoffRevokeReason>,
    ) {
        let removed = lock(&self.state).handoffs.remove(&id);
        let Some(record) = removed else {
            if had_delayed && let Some(token) = published_token {
                let _ = self.provider.release_delayed(token);
            }
            return;
        };
        let token = record.native_token.or(published_token);
        let delayed = record.delayed;
        let emitted = !delayed.is_empty();
        for representation in &delayed {
            representation.release_promised_files();
        }
        if had_delayed && let Some(token) = token {
            let _ = self.provider.release_delayed(token);
        }
        if emitted && let Some(reason) = revoked_reason {
            self.emit(DndEvent::HandoffRevoked {
                handoff_id: id,
                reason,
            });
        }
    }

    fn take_external_drag_locked(
        state: &mut ServiceState,
        native_session_token: u64,
    ) -> Result<(), StaleCallbackReason> {
        let Some((surface, generation)) = state.external_drags.remove(&native_session_token) else {
            return Err(StaleCallbackReason::UnknownSession);
        };
        if Self::surface_live_locked(state, surface, generation) {
            Ok(())
        } else {
            Err(StaleCallbackReason::SurfaceNotLive)
        }
    }

    fn purge_surface_locked(
        state: &mut ServiceState,
        surface: SurfaceId,
        generation: u64,
        _reason: DragCancelReason,
    ) -> Purged {
        let mut purged = Purged::default();
        let current_target_surface = state.drag.as_ref().and_then(|drag| {
            drag.current_target.and_then(|target| {
                state
                    .targets
                    .get(&target)
                    .map(|target| (target.spec.surface, target.spec.generation))
            })
        });
        let handoff_ids = state
            .handoffs
            .iter()
            .filter(|(_, record)| {
                matches!(
                    record.scope,
                    HandoffScope::Surface {
                        surface: record_surface,
                        generation: record_generation,
                    } if record_surface == surface && record_generation == generation
                )
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in handoff_ids {
            if let Some(record) = state.handoffs.remove(&id) {
                purged.handoffs.push(PurgedHandoff {
                    id,
                    token: record.native_token,
                    delayed: record.delayed,
                });
            }
        }

        state
            .external_drags
            .retain(|_, (drag_surface, drag_generation)| {
                *drag_surface != surface || *drag_generation != generation
            });

        let target_ids = state
            .targets
            .iter()
            .filter(|(_, target)| {
                target.spec.surface == surface && target.spec.generation == generation
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in target_ids {
            state.targets.remove(&id);
            purged.targets.push(id);
        }

        if let Some(drag) = state.drag.as_mut()
            && drag.is_active()
        {
            let source_match =
                drag.source_surface == surface && drag.source_generation == generation;
            let target_surface_match = current_target_surface == Some((surface, generation));
            if source_match || target_surface_match {
                let cancel_reason = if source_match {
                    DragCancelReason::SourceSurfaceDisposed
                } else {
                    DragCancelReason::TargetSurfaceDisposed
                };
                drag.state = DragSessionStatus::Cancelled(cancel_reason);
                drag.current_target = None;
                drag.current_format = None;
                drag.current_effect = None;
                purged.drag = Some((
                    drag.id,
                    drag.native_token,
                    drag.representations.clone(),
                    cancel_reason,
                ));
            }
        }
        purged.surface = Some((surface, generation));
        purged
    }

    fn process_purged(&self, purged: Purged) {
        for handoff in purged.handoffs {
            for representation in &handoff.delayed {
                representation.release_promised_files();
            }
            if let Some(token) = handoff.token {
                let _ = self.provider.release_delayed(token);
            }
            self.emit(DndEvent::HandoffRevoked {
                handoff_id: handoff.id,
                reason: HandoffRevokeReason::SurfaceDisposed,
            });
        }
        for target in purged.targets {
            self.emit(DndEvent::DropTargetRemoved { target_id: target });
        }
        if let Some((id, token, representations, reason)) = purged.drag {
            if token != 0 {
                let _ = self.provider.cancel_drag(token);
            }
            release_representations(&representations);
            self.emit(DndEvent::DragCancelled {
                session_id: id,
                reason,
            });
        }
        if let Some((surface, generation)) = purged.surface {
            self.emit(DndEvent::SurfaceDisposed {
                surface,
                generation,
            });
        }
    }

    fn cancel_active_drag_locked(
        state: &mut ServiceState,
        reason: DragCancelReason,
    ) -> Option<(
        DragSessionId,
        u64,
        Vec<SourceRepresentation>,
        DragCancelReason,
    )> {
        let drag = state.drag.as_mut()?;
        if !drag.is_active() {
            return None;
        }
        drag.state = DragSessionStatus::Cancelled(reason);
        drag.current_target = None;
        drag.current_format = None;
        drag.current_effect = None;
        Some((
            drag.id,
            drag.native_token,
            drag.representations.clone(),
            reason,
        ))
    }

    fn process_drag_cancel(
        &self,
        id: DragSessionId,
        token: u64,
        representations: Vec<SourceRepresentation>,
        reason: DragCancelReason,
    ) {
        if token != 0 {
            let _ = self.provider.cancel_drag(token);
        }
        release_representations(&representations);
        self.emit(DndEvent::DragCancelled {
            session_id: id,
            reason,
        });
    }

    fn revoke_all_handoffs_locked(
        state: &mut ServiceState,
        reason: HandoffRevokeReason,
    ) -> Vec<PurgedHandoff> {
        let ids = state.handoffs.keys().copied().collect::<Vec<_>>();
        let mut purged = Vec::new();
        for id in ids {
            if let Some(mut record) = state.handoffs.remove(&id) {
                record.revoked = true;
                purged.push(PurgedHandoff {
                    id,
                    token: record.native_token,
                    delayed: std::mem::take(&mut record.delayed),
                });
            }
        }
        let _ = reason;
        purged
    }

    fn process_handoff_purges(&self, handoffs: Vec<PurgedHandoff>, reason: HandoffRevokeReason) {
        for handoff in handoffs {
            for representation in &handoff.delayed {
                representation.release_promised_files();
            }
            if let Some(token) = handoff.token {
                let _ = self.provider.release_delayed(token);
            }
            self.emit(DndEvent::HandoffRevoked {
                handoff_id: handoff.id,
                reason,
            });
        }
    }

    fn cancel_session(&self, id: DragSessionId, reason: DragCancelReason) -> DragCancelOutcome {
        let cleanup = {
            let mut state = lock(&self.state);
            let Some(drag) = state.drag.as_mut().filter(|drag| drag.id == id) else {
                return DragCancelOutcome::Ignored(StaleCallbackReason::UnknownSession);
            };
            if !drag.is_active() {
                return DragCancelOutcome::AlreadyTerminal;
            }
            drag.state = DragSessionStatus::Cancelled(reason);
            drag.current_target = None;
            drag.current_format = None;
            drag.current_effect = None;
            (drag.native_token, drag.representations.clone())
        };
        self.process_drag_cancel(id, cleanup.0, cleanup.1, reason);
        DragCancelOutcome::Cancelled(reason)
    }
}

#[derive(Default)]
struct Purged {
    handoffs: Vec<PurgedHandoff>,
    targets: Vec<DropTargetId>,
    drag: Option<(
        DragSessionId,
        u64,
        Vec<SourceRepresentation>,
        DragCancelReason,
    )>,
    surface: Option<(SurfaceId, u64)>,
}

struct PurgedHandoff {
    id: HandoffId,
    token: Option<u64>,
    delayed: Vec<LazyRepresentation>,
}

struct InteractionBinding {
    surface: Option<(SurfaceId, u64)>,
    context: OperationContext,
}

fn release_representations(representations: &[SourceRepresentation]) {
    for representation in representations {
        match representation {
            SourceRepresentation::Eager(payload) => payload.release_resources(),
            SourceRepresentation::Delayed(delayed) => delayed.release_promised_files(),
        }
    }
}

impl RichTransferProviderListener for ServiceInner {
    fn on_rich_transfer_notice(&self, notice: RichTransferProviderNotice) {
        match notice {
            RichTransferProviderNotice::ClipboardChanged => {
                self.emit(DndEvent::ClipboardChanged);
            }
            RichTransferProviderNotice::ExternalDragEntered {
                native_session_token,
                surface,
                generation,
                point: _,
                offers,
            } => {
                if native_session_token == 0 {
                    self.record_stale(None, StaleCallbackReason::TokenMismatch);
                    return;
                }
                let accepted = {
                    let mut state = lock(&self.state);
                    if surface == SurfaceId(0)
                        || generation == 0
                        || !Self::surface_live_locked(&state, surface, generation)
                    {
                        false
                    } else {
                        state
                            .external_drags
                            .insert(native_session_token, (surface, generation));
                        state.external_drag_notices = state.external_drag_notices.saturating_add(1);
                        true
                    }
                };
                if !accepted {
                    self.record_stale(None, StaleCallbackReason::SurfaceNotLive);
                    return;
                }
                self.emit(DndEvent::ExternalDragEntered {
                    native_session_token,
                    surface,
                    generation,
                    formats: offers.into_iter().map(|offer| offer.descriptor).collect(),
                });
            }
            RichTransferProviderNotice::ExternalDragLeft {
                native_session_token,
            } => {
                if native_session_token == 0 {
                    self.record_stale(None, StaleCallbackReason::TokenMismatch);
                    return;
                }
                let ended = {
                    let mut state = lock(&self.state);
                    Self::take_external_drag_locked(&mut state, native_session_token)
                };
                match ended {
                    Ok(()) => self.emit(DndEvent::ExternalDragLeft {
                        native_session_token,
                    }),
                    Err(reason) => self.record_stale(None, reason),
                }
            }
            RichTransferProviderNotice::ExternalDrop {
                native_session_token,
                point,
            } => {
                if native_session_token == 0 {
                    self.record_stale(None, StaleCallbackReason::TokenMismatch);
                    return;
                }
                let ended = {
                    let mut state = lock(&self.state);
                    Self::take_external_drag_locked(&mut state, native_session_token)
                };
                match ended {
                    Ok(()) => self.emit(DndEvent::ExternalDrop {
                        native_session_token,
                        point,
                    }),
                    Err(reason) => self.record_stale(None, reason),
                }
            }
            RichTransferProviderNotice::SessionCancelled {
                native_session_token,
            } => {
                let id = {
                    let mut state = lock(&self.state);
                    state.external_drags.remove(&native_session_token);
                    state.drag.as_ref().and_then(|drag| {
                        (drag.native_token == native_session_token && drag.is_active())
                            .then_some(drag.id)
                    })
                };
                if let Some(id) = id {
                    self.cancel_session(id, DragCancelReason::ProviderCancelled);
                }
            }
            RichTransferProviderNotice::SourceExited => {
                let (handoffs, drag) = {
                    let mut state = lock(&self.state);
                    state.external_drags.clear();
                    let handoffs = Self::revoke_all_handoffs_locked(
                        &mut state,
                        HandoffRevokeReason::SourceExit,
                    );
                    let drag =
                        Self::cancel_active_drag_locked(&mut state, DragCancelReason::SourceExited);
                    (handoffs, drag)
                };
                self.process_handoff_purges(handoffs, HandoffRevokeReason::SourceExit);
                if let Some((id, token, representations, reason)) = drag {
                    self.process_drag_cancel(id, token, representations, reason);
                }
            }
        }
    }
}

/// A capability-gated, UI-local rich clipboard and drag-and-drop service.
///
/// Clones share the same state and identity. All operations run on the owning UI
/// thread; background streaming uses [`ConsumedPayload::open_stream`] with a
/// background [`OperationContext`].
#[derive(Clone)]
pub struct RichTransferService {
    inner: Rc<ServiceInner>,
}

impl RichTransferService {
    pub fn new(provider: Rc<dyn RichTransferProvider>) -> Self {
        Self::with_capacity(provider, DEFAULT_DND_EVENT_CAPACITY, None)
    }

    pub fn with_ui(
        provider: Rc<dyn RichTransferProvider>,
        ui: UiContext,
    ) -> Result<Self, SystemError> {
        if !ui.is_current() {
            return Err(context_expired("Rich-transfer UI context is expired"));
        }
        Ok(Self::with_capacity(
            provider,
            DEFAULT_DND_EVENT_CAPACITY,
            Some(ui),
        ))
    }

    pub fn new_with_ui(
        provider: Rc<dyn RichTransferProvider>,
        ui: UiContext,
    ) -> Result<Self, SystemError> {
        Self::with_ui(provider, ui)
    }

    pub fn with_event_capacity(provider: Rc<dyn RichTransferProvider>, capacity: usize) -> Self {
        Self::with_capacity(provider, capacity, None)
    }

    pub fn from_fake(fake: FakeRichTransferProvider) -> Self {
        Self::new(Rc::new(fake))
    }

    pub fn from_fake_with_ui(
        fake: FakeRichTransferProvider,
        ui: UiContext,
    ) -> Result<Self, SystemError> {
        Self::with_ui(Rc::new(fake), ui)
    }

    fn with_capacity(
        provider: Rc<dyn RichTransferProvider>,
        capacity: usize,
        ui: Option<UiContext>,
    ) -> Self {
        let inner = Rc::new(ServiceInner {
            provider,
            state: Mutex::new(ServiceState {
                phase: AppPhase::ForegroundActive,
                surfaces: HashMap::new(),
                handoffs: BTreeMap::new(),
                targets: BTreeMap::new(),
                drag: None,
                external_drags: BTreeMap::new(),
                next_handoff: 1,
                next_target: 1,
                next_session: 1,
                subscribers: Vec::new(),
                stale_callback_count: 0,
                last_stale_callback: None,
                external_drag_notices: 0,
            }),
            identity: Rc::new(()),
            event_capacity: capacity.max(1),
            budget: Arc::new(StreamBudget::new()),
            ui,
        });
        let listener: Rc<dyn RichTransferProviderListener> = inner.clone();
        let weak = Rc::downgrade(&listener);
        drop(listener);
        inner.provider.attach_listener(weak);
        Self { inner }
    }

    pub fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    pub fn platform(&self) -> PlatformTarget {
        self.inner.provider.platform()
    }

    pub fn capabilities(&self) -> Vec<Capability<DndOperation>> {
        self.inner.provider.capabilities()
    }

    pub fn capability(&self, operation: DndOperation) -> Capability<DndOperation> {
        self.inner.provider.capability(operation)
    }

    pub fn active_stream_leases(&self) -> usize {
        self.inner.budget.active()
    }

    pub fn subscribe(&self) -> EventSubscription<DndEvent> {
        let live = Rc::new(AtomicBool::new(true));
        let cleanup_live = Rc::clone(&live);
        let (sender, subscription) = event_channel(self.inner.event_capacity, move || {
            cleanup_live.store(false, Ordering::Release);
        });
        lock(&self.inner.state).subscribers.push((live, sender));
        subscription
    }

    /// Create a service-level foreground interaction. Requires an installed UI
    /// context.
    pub fn user_initiated(&self) -> Result<TransferInteraction, SystemError> {
        let ui =
            self.inner.ui.clone().ok_or_else(|| {
                context_expired("Rich-transfer service has no installed UI context")
            })?;
        self.user_initiated_with(ui)
    }

    /// Create a foreground interaction for one explicit live UI attachment.
    pub fn user_initiated_with(&self, ui: UiContext) -> Result<TransferInteraction, SystemError> {
        if !ui.is_current() {
            return Err(context_expired(
                "Rich-transfer interaction requires a current UI context",
            ));
        }
        self.inner.check_phase()?;
        let context = OperationContext {
            ui: Some(ui),
            ..OperationContext::default()
        };
        context.require_ui()?;
        let surface = context.ui.as_ref().map(|ui| (ui.surface, ui.generation));
        Ok(TransferInteraction::new(
            Rc::clone(&self.inner.identity),
            context,
            surface,
        ))
    }

    /// Create an interaction for an explicit surface/generation without carrying
    /// a live UI attachment. The service surface ledger must still consider the
    /// scope live.
    pub fn user_initiated_for_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<TransferInteraction, SystemError> {
        self.inner.check_phase()?;
        self.inner.require_surface_attached(surface, generation)?;
        self.inner.check_surface(surface, generation, true)?;
        Ok(TransferInteraction::new(
            Rc::clone(&self.inner.identity),
            OperationContext::default(),
            Some((surface, generation)),
        ))
    }

    /// Clone a live operation context into a service-bound interaction.
    pub fn user_initiated_in_context(
        &self,
        context: &OperationContext,
    ) -> Result<TransferInteraction, SystemError> {
        context.check_at(Instant::now())?;
        context.require_ui()?;
        self.inner.check_phase()?;
        let surface = context.ui.as_ref().map(|ui| (ui.surface, ui.generation));
        Ok(TransferInteraction::new(
            Rc::clone(&self.inner.identity),
            context.clone(),
            surface,
        ))
    }

    /// Synchronize the host lifecycle snapshot. Terminating revokes all delayed
    /// callbacks; backgrounding cancels an active drag.
    pub fn notify_lifecycle(&self, lifecycle: &LifecycleState) {
        let phase = lifecycle.phase();
        let (handoffs, drag, terminating) = {
            let mut state = lock(&self.inner.state);
            state.phase = phase;
            if phase == AppPhase::Terminating {
                state.external_drags.clear();
                let handoffs = ServiceInner::revoke_all_handoffs_locked(
                    &mut state,
                    HandoffRevokeReason::AppTerminating,
                );
                let drag = ServiceInner::cancel_active_drag_locked(
                    &mut state,
                    DragCancelReason::AppTerminating,
                );
                (handoffs, drag, true)
            } else if !phase.is_foreground() {
                let drag = ServiceInner::cancel_active_drag_locked(
                    &mut state,
                    DragCancelReason::AppBackgrounded,
                );
                (Vec::new(), drag, false)
            } else {
                (Vec::new(), None, false)
            }
        };
        self.inner.process_handoff_purges(
            handoffs,
            if terminating {
                HandoffRevokeReason::AppTerminating
            } else {
                HandoffRevokeReason::SourceExit
            },
        );
        if let Some((id, token, representations, reason)) = drag {
            self.inner
                .process_drag_cancel(id, token, representations, reason);
        }
    }

    /// Notify the service that the source application is exiting. Eager OS
    /// clipboard data remains; every delayed callback and promised resource is
    /// revoked immediately.
    pub fn notify_source_exit(&self) {
        let (handoffs, drag) = {
            let mut state = lock(&self.inner.state);
            state.external_drags.clear();
            let handoffs = ServiceInner::revoke_all_handoffs_locked(
                &mut state,
                HandoffRevokeReason::SourceExit,
            );
            let drag =
                ServiceInner::cancel_active_drag_locked(&mut state, DragCancelReason::SourceExited);
            (handoffs, drag)
        };
        self.inner
            .process_handoff_purges(handoffs, HandoffRevokeReason::SourceExit);
        if let Some((id, token, representations, reason)) = drag {
            self.inner
                .process_drag_cancel(id, token, representations, reason);
        }
    }

    /// Attach a surface generation. A newer generation purges resources bound to
    /// older generations of the same surface.
    pub fn attach_surface(
        &self,
        surface: SurfaceId,
        generation: u64,
        visible: bool,
    ) -> Result<(), SystemError> {
        if surface == SurfaceId(0) || generation == 0 {
            return Err(invalid(
                "A surface attachment requires a nonzero surface and generation",
            ));
        }
        let purged = {
            let mut state = lock(&self.inner.state);
            match state
                .surfaces
                .get(&surface)
                .copied()
                .map(|record| record.generation)
            {
                Some(current) if current > generation => return Ok(()),
                Some(current) if current == generation => {
                    let record = state.surfaces.get_mut(&surface).expect("checked above");
                    record.visible = visible;
                    record.live = true;
                    None
                }
                Some(current) => {
                    let purged = ServiceInner::purge_surface_locked(
                        &mut state,
                        surface,
                        current,
                        DragCancelReason::SourceSurfaceDisposed,
                    );
                    state.surfaces.insert(
                        surface,
                        SurfaceRecord {
                            generation,
                            visible,
                            live: true,
                        },
                    );
                    Some(purged)
                }
                None => {
                    state.surfaces.insert(
                        surface,
                        SurfaceRecord {
                            generation,
                            visible,
                            live: true,
                        },
                    );
                    None
                }
            }
        };
        if let Some(purged) = purged {
            self.inner.process_purged(purged);
        }
        Ok(())
    }

    pub fn notify_surface_visible(&self, surface: SurfaceId, generation: u64) {
        if let Some(record) = lock(&self.inner.state).surfaces.get_mut(&surface)
            && record.generation == generation
        {
            record.visible = true;
        }
    }

    pub fn notify_surface_hidden(&self, surface: SurfaceId, generation: u64) {
        if let Some(record) = lock(&self.inner.state).surfaces.get_mut(&surface)
            && record.generation == generation
        {
            record.visible = false;
        }
    }

    pub fn is_surface_live(&self, surface: SurfaceId, generation: u64) -> bool {
        self.inner.surface_live(surface, generation)
    }

    /// Dispose exactly one surface generation. A newer generation on the same
    /// surface is never touched.
    pub fn dispose_surface_generation(&self, surface: SurfaceId, generation: u64) {
        let purged = {
            let mut state = lock(&self.inner.state);
            let record_matches = state
                .surfaces
                .get(&surface)
                .is_some_and(|record| record.generation == generation);
            let purged = ServiceInner::purge_surface_locked(
                &mut state,
                surface,
                generation,
                DragCancelReason::SourceSurfaceDisposed,
            );
            if record_matches && let Some(record) = state.surfaces.get_mut(&surface) {
                record.live = false;
                record.visible = false;
            }
            purged
        };
        self.inner.process_purged(purged);
    }

    /// Dispose the currently attached generation of a surface.
    pub fn dispose_surface(&self, surface: SurfaceId) {
        let generation = lock(&self.inner.state)
            .surfaces
            .get(&surface)
            .map(|record| record.generation);
        if let Some(generation) = generation {
            self.dispose_surface_generation(surface, generation);
            lock(&self.inner.state).surfaces.remove(&surface);
        }
    }

    /// Write a rich clipboard handoff. Eager formats are published to the OS;
    /// delayed callbacks live exactly as long as the returned lease.
    pub fn write_clipboard(
        &self,
        interaction: TransferInteraction,
        write: RichClipboardWrite,
    ) -> ServiceResult<HandoffLease> {
        let binding = self
            .inner
            .validate_interaction(&interaction, DndOperation::WriteRichClipboard)?;
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            return Ok(Completion::Cancelled(reason));
        }
        write.validate().inspect_err(|error| {
            self.inner
                .emit_failure(DndOperation::WriteRichClipboard, error);
        })?;
        if write.has_delayed() {
            if let Err(error) = self
                .inner
                .gate_capability(DndOperation::ProvideDelayedPayload)
            {
                return self.inner.fail(DndOperation::WriteRichClipboard, error);
            }
            if self.inner.provider.platform() == PlatformTarget::Web {
                return self.inner.fail(
                    DndOperation::WriteRichClipboard,
                    unsupported("Browsers cannot register persistent delayed clipboard callbacks"),
                );
            }
        }
        if let HandoffScope::Surface {
            surface,
            generation,
        } = write.scope
        {
            let Some((bound_surface, bound_generation)) = binding.surface else {
                return self.inner.fail(
                    DndOperation::WriteRichClipboard,
                    invalid("A surface handoff requires a surface-bound interaction"),
                );
            };
            if surface != bound_surface || generation != bound_generation {
                return self.inner.fail(
                    DndOperation::WriteRichClipboard,
                    invalid("Clipboard handoff surface does not match the user gesture"),
                );
            }
            if let Err(error) = self.inner.require_surface_attached(surface, generation) {
                return self.inner.fail(DndOperation::WriteRichClipboard, error);
            }
        }

        let handoff_id = {
            let mut state = lock(&self.inner.state);
            let id = HandoffId(state.next_handoff);
            state.next_handoff = state.next_handoff.saturating_add(1);
            state.handoffs.insert(
                id,
                HandoffRecord {
                    id,
                    scope: write.scope,
                    native_token: None,
                    sequence: None,
                    eager: write.eager.clone(),
                    delayed: write.delayed.clone(),
                    revoked: false,
                },
            );
            id
        };
        let request = ClipboardHandoff {
            handoff_id,
            scope: write.scope,
            eager: write.eager.clone(),
            delayed: write.delayed.clone(),
        };
        let grant = match self.inner.provider.publish_clipboard(&request) {
            Ok(grant) => grant,
            Err(error) => {
                self.inner
                    .discard_failed_handoff(handoff_id, None, write.has_delayed(), None);
                return self.inner.fail(DndOperation::WriteRichClipboard, error);
            }
        };

        {
            let mut state = lock(&self.inner.state);
            let Some(record) = state.handoffs.get_mut(&handoff_id) else {
                drop(state);
                let _ = self
                    .inner
                    .provider
                    .release_delayed(grant.native_handoff_token);
                write.release_promised_files();
                return self.inner.fail(
                    DndOperation::WriteRichClipboard,
                    context_expired(
                        "The clipboard handoff surface was disposed during provider publication",
                    ),
                );
            };
            record.native_token = Some(grant.native_handoff_token);
            record.sequence = grant.sequence;
        }

        // Post-provider TOCTOU: the surface/lifecycle/context may have changed
        // while the native publication was in flight.
        let post = self
            .inner
            .validate_interaction(&interaction, DndOperation::WriteRichClipboard)
            .and_then(|_| {
                if let HandoffScope::Surface {
                    surface,
                    generation,
                } = write.scope
                {
                    self.inner.check_surface(surface, generation, true)?;
                }
                Ok(())
            });
        if let Err(error) = post {
            self.inner.discard_failed_handoff(
                handoff_id,
                Some(grant.native_handoff_token),
                write.has_delayed(),
                Some(HandoffRevokeReason::SurfaceDisposed),
            );
            return self.inner.fail(DndOperation::WriteRichClipboard, error);
        }

        // Cancellation can be delivered by a re-entrant provider hook while
        // publication was in flight. Tear down only this handoff and leave any
        // previously published clipboard handoff untouched.
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            self.inner.discard_failed_handoff(
                handoff_id,
                Some(grant.native_handoff_token),
                write.has_delayed(),
                None,
            );
            return Ok(Completion::Cancelled(reason));
        }

        let replaced = {
            let mut state = lock(&self.inner.state);
            let ids = state
                .handoffs
                .keys()
                .copied()
                .filter(|id| *id != handoff_id)
                .collect::<Vec<_>>();
            let mut purged = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(record) = state.handoffs.remove(&id) {
                    if record.revoked {
                        // A revoked record already released its delayed
                        // callbacks and native token exactly once.
                        for delayed in &record.delayed {
                            delayed.release_promised_files();
                        }
                        continue;
                    }
                    purged.push(PurgedHandoff {
                        id,
                        token: record.native_token,
                        delayed: record.delayed,
                    });
                }
            }
            purged
        };
        self.inner
            .process_handoff_purges(replaced, HandoffRevokeReason::Replaced);

        let receipt = self
            .inner
            .handoff_receipt(handoff_id)
            .ok_or_else(|| context_expired("Clipboard handoff disappeared after publication"))?;
        self.inner.emit(DndEvent::HandoffPublished {
            handoff_id,
            sequence: receipt.sequence,
            eager_formats: receipt.eager_formats.clone(),
            delayed_formats: receipt.delayed_formats.clone(),
        });
        Ok(Completion::Completed(HandoffLease::new(
            Rc::clone(&self.inner),
            handoff_id,
        )))
    }

    /// Convenience form returning both the receipt and the owning lease.
    pub fn write_clipboard_receipt(
        &self,
        interaction: TransferInteraction,
        write: RichClipboardWrite,
    ) -> ServiceResult<(HandoffReceipt, HandoffLease)> {
        match self.write_clipboard(interaction, write)? {
            Completion::Completed(lease) => {
                let receipt = lease.receipt().ok_or_else(|| {
                    context_expired("Clipboard handoff disappeared after publication")
                })?;
                Ok(Completion::Completed((receipt, lease)))
            }
            Completion::Cancelled(reason) => Ok(Completion::Cancelled(reason)),
        }
    }

    /// Read and negotiate the current rich clipboard against target preferences.
    pub fn read_clipboard(
        &self,
        interaction: TransferInteraction,
        target_preferences: &[FormatDescriptor],
    ) -> ServiceResult<ClipboardReadOutcome> {
        let binding = self
            .inner
            .validate_interaction(&interaction, DndOperation::ReadRichClipboard)?;
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            return Ok(Completion::Cancelled(reason));
        }
        if target_preferences.is_empty() {
            return self.inner.fail(
                DndOperation::ReadRichClipboard,
                invalid("At least one target format preference is required"),
            );
        }
        let snapshot = match self.inner.provider.clipboard_snapshot(&binding.context) {
            Ok(Completion::Completed(snapshot)) => snapshot,
            Ok(Completion::Cancelled(reason)) => return Ok(Completion::Cancelled(reason)),
            Err(error) => {
                return self.inner.fail(DndOperation::ReadRichClipboard, error);
            }
        };
        for entry in &snapshot.entries {
            if let Err(error) = entry.validate() {
                let error = crate::error::integrity(match error.code {
                    ErrorCode::InvalidInput => {
                        "Provider clipboard payload violates its typed bounds"
                    }
                    _ => "Provider clipboard payload is invalid",
                });
                return self.inner.fail(DndOperation::ReadRichClipboard, error);
            }
        }
        let offers = snapshot.offers();
        if offers.is_empty() {
            self.inner.emit(DndEvent::ClipboardEmpty);
            return completed(ClipboardReadOutcome::Empty);
        }
        let Some(negotiated) = negotiate_format(&offers, target_preferences) else {
            self.inner.emit(DndEvent::ClipboardNoMatchingFormat {
                available_count: offers.len(),
            });
            return completed(ClipboardReadOutcome::NoMatchingFormat {
                available_offers: offers,
            });
        };
        let Some(entry) = snapshot.entries.get(negotiated.offer_index).cloned() else {
            return self.inner.fail(
                DndOperation::ReadRichClipboard,
                crate::error::integrity("Negotiated clipboard offer index is inconsistent"),
            );
        };
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            return Ok(Completion::Cancelled(reason));
        }

        let mut delayed_handoff = None;
        let payload = match entry {
            ClipboardEntry::Eager(payload) => {
                if !payload.conforms_to(&negotiated.descriptor) {
                    return self.inner.fail(
                        DndOperation::ReadRichClipboard,
                        crate::error::integrity(
                            "Provider eager clipboard payload does not match its descriptor",
                        ),
                    );
                }
                payload
            }
            ClipboardEntry::Delayed {
                representation,
                handoff_id,
            } => {
                delayed_handoff = handoff_id;
                if let Some(handoff_id) = handoff_id {
                    let check = {
                        let state = lock(&self.inner.state);
                        match state.handoffs.get(&handoff_id) {
                            None => Err(expired("The clipboard handoff was replaced or expired")),
                            Some(record) if record.revoked => {
                                Err(expired("Delayed clipboard representations were revoked"))
                            }
                            Some(record) => match record.scope {
                                HandoffScope::Application => Ok(()),
                                HandoffScope::Surface {
                                    surface,
                                    generation,
                                } => {
                                    if ServiceInner::surface_live_locked(
                                        &state, surface, generation,
                                    ) {
                                        Ok(())
                                    } else {
                                        Err(context_expired(
                                            "The clipboard handoff surface generation is not live",
                                        ))
                                    }
                                }
                            },
                        }
                    };
                    check.inspect_err(|error| {
                        self.inner
                            .emit_failure(DndOperation::ReadRichClipboard, error);
                    })?;
                }
                match representation.invoke(&binding.context) {
                    Ok(Completion::Completed(payload)) => payload,
                    Ok(Completion::Cancelled(reason)) => {
                        return Ok(Completion::Cancelled(reason));
                    }
                    Err(error) => {
                        return self.inner.fail(DndOperation::ReadRichClipboard, error);
                    }
                }
            }
        };

        if let Some(handoff_id) = delayed_handoff {
            let delayed_live = {
                let state = lock(&self.inner.state);
                match state.handoffs.get(&handoff_id) {
                    Some(record) if !record.revoked => match record.scope {
                        HandoffScope::Application => Ok(()),
                        HandoffScope::Surface {
                            surface,
                            generation,
                        } => {
                            if ServiceInner::surface_live_locked(&state, surface, generation) {
                                Ok(())
                            } else {
                                Err(context_expired(
                                    "The clipboard handoff surface generation was revoked during generation",
                                ))
                            }
                        }
                    },
                    Some(_) => Err(expired(
                        "Delayed clipboard representations were revoked during generation",
                    )),
                    None => Err(expired(
                        "The clipboard handoff was replaced during delayed generation",
                    )),
                }
            };
            if let Err(error) = delayed_live {
                payload.release_resources();
                return self.inner.fail(DndOperation::ReadRichClipboard, error);
            }
        }
        let post = self
            .inner
            .check_phase()
            .and_then(|_| {
                binding.context.check_at(Instant::now())?;
                Ok(())
            })
            .and_then(|_| {
                if let Some((surface, generation)) = binding.surface {
                    self.inner.check_surface(surface, generation, true)?;
                }
                Ok(())
            });
        if let Err(error) = post {
            payload.release_resources();
            return self.inner.fail(DndOperation::ReadRichClipboard, error);
        }
        if !payload.conforms_to(&negotiated.descriptor) {
            payload.release_resources();
            return self.inner.fail(
                DndOperation::ReadRichClipboard,
                crate::error::integrity(
                    "Deferred clipboard payload does not conform to the promised format",
                ),
            );
        }
        if let Err(error) = payload.validate() {
            payload.release_resources();
            return self.inner.fail(DndOperation::ReadRichClipboard, error);
        }
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            payload.release_resources();
            return Ok(Completion::Cancelled(reason));
        }
        let consumed = ConsumedPayload::new(
            negotiated.descriptor.clone(),
            negotiated.delivery,
            payload,
            Arc::clone(&self.inner.budget),
        );
        self.inner.emit(DndEvent::ClipboardRead {
            format: negotiated.descriptor,
            delivery: negotiated.delivery,
        });
        completed(ClipboardReadOutcome::Matched(consumed))
    }

    /// Register a scoped drop target. Targets are removed automatically when
    /// their surface generation advances or is disposed.
    pub fn register_drop_target(
        &self,
        spec: DropTargetSpec,
    ) -> Result<DropTargetLease, SystemError> {
        spec.validate()?;
        if let Err(error) = self.inner.gate_capability(DndOperation::AcceptDropTarget) {
            self.inner
                .emit_failure(DndOperation::AcceptDropTarget, &error);
            return Err(error);
        }
        self.inner
            .require_surface_attached(spec.surface, spec.generation)?;
        let id = {
            let mut state = lock(&self.inner.state);
            let id = DropTargetId(state.next_target);
            state.next_target = state.next_target.saturating_add(1);
            state.targets.insert(id, TargetRecord { id, spec });
            id
        };
        let target = lock(&self.inner.state).targets.get(&id).cloned();
        if let Some(target) = target {
            self.inner.emit(DndEvent::DropTargetRegistered {
                target_id: id,
                surface: target.spec.surface,
                generation: target.spec.generation,
            });
        }
        Ok(DropTargetLease {
            inner: Rc::clone(&self.inner),
            target_id: id,
        })
    }

    pub fn drop_target_ids(&self) -> Vec<DropTargetId> {
        lock(&self.inner.state).targets.keys().copied().collect()
    }

    pub fn stale_callback_count(&self) -> u64 {
        lock(&self.inner.state).stale_callback_count
    }

    pub fn last_stale_callback(&self) -> Option<StaleCallbackReason> {
        lock(&self.inner.state).last_stale_callback
    }

    pub fn external_drag_notice_count(&self) -> u64 {
        lock(&self.inner.state).external_drag_notices
    }

    /// Start exactly one native drag session.
    pub fn start_drag(
        &self,
        interaction: TransferInteraction,
        spec: DragSourceSpec,
    ) -> ServiceResult<DragSessionLease> {
        let binding = self
            .inner
            .validate_interaction(&interaction, DndOperation::StartDragSession)?;
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            return Ok(Completion::Cancelled(reason));
        }
        spec.validate().inspect_err(|error| {
            self.inner
                .emit_failure(DndOperation::StartDragSession, error);
        })?;
        let (source_surface, source_generation) = if let Some(scope) = spec.source_scope() {
            let Some((bound_surface, bound_generation)) = binding.surface else {
                return self.inner.fail(
                    DndOperation::StartDragSession,
                    invalid("A scoped drag source requires a surface-bound gesture"),
                );
            };
            if scope != (bound_surface, bound_generation) {
                return self.inner.fail(
                    DndOperation::StartDragSession,
                    invalid("Drag source surface does not match the user gesture"),
                );
            }
            scope
        } else {
            binding
                .surface
                .expect("validated interactions always carry a surface")
        };
        self.inner
            .check_surface(source_surface, source_generation, true)?;
        if spec.has_delayed() {
            if let Err(error) = self
                .inner
                .gate_capability(DndOperation::ProvideDelayedPayload)
            {
                return self.inner.fail(DndOperation::StartDragSession, error);
            }
            if self.inner.provider.platform() == PlatformTarget::Web {
                return self.inner.fail(
                    DndOperation::StartDragSession,
                    unsupported("Browsers cannot provide persistent delayed drag payloads"),
                );
            }
        }

        let session_id = {
            let mut state = lock(&self.inner.state);
            if state.drag.as_ref().is_some_and(DragRecord::is_active) {
                return self.inner.fail(
                    DndOperation::StartDragSession,
                    busy("A drag session is already active"),
                );
            }
            let session_id = DragSessionId(state.next_session);
            state.next_session = state.next_session.saturating_add(1);
            state.drag = Some(DragRecord {
                id: session_id,
                native_token: 0,
                epoch: 0,
                source_surface,
                source_generation,
                source_effects: spec.allowed_effects,
                representations: spec.representations.clone(),
                context: binding.context.clone(),
                state: DragSessionStatus::Active,
                current_target: None,
                current_format: None,
                current_effect: None,
            });
            session_id
        };

        let promised_files = spec.representations.iter().fold(0usize, |total, item| {
            if let SourceRepresentation::Delayed(delayed) = item {
                total.saturating_add(delayed.promised_files().len())
            } else {
                total
            }
        });
        let request = ProviderDragRequest {
            session_id,
            source_surface,
            source_generation,
            allowed_effects: spec.allowed_effects,
            preview: spec.preview.clone(),
            offers: spec.offers(),
            delayed_formats: spec.delayed_formats(),
            promised_files,
        };
        let grant = match self.inner.provider.start_drag(&request) {
            Ok(grant) => grant,
            Err(error) => {
                if let Some(drag) = lock(&self.inner.state)
                    .drag
                    .take_if(|drag| drag.id == session_id)
                {
                    release_representations(&drag.representations);
                }
                return self.inner.fail(DndOperation::StartDragSession, error);
            }
        };

        if grant.native_session_token == 0 || grant.session_epoch == 0 {
            let cleanup = lock(&self.inner.state)
                .drag
                .take_if(|drag| drag.id == session_id);
            let _ = self.inner.provider.cancel_drag(grant.native_session_token);
            if let Some(drag) = cleanup {
                release_representations(&drag.representations);
            }
            self.inner.emit(DndEvent::DragCancelled {
                session_id,
                reason: DragCancelReason::SourceSurfaceDisposed,
            });
            return self.inner.fail(
                DndOperation::StartDragSession,
                crate::error::integrity("Provider returned an invalid drag session grant"),
            );
        }
        let post = self
            .inner
            .validate_interaction(&interaction, DndOperation::StartDragSession)
            .and_then(|_| {
                self.inner
                    .check_surface(source_surface, source_generation, true)
            });
        if let Err(error) = post {
            let cleanup = lock(&self.inner.state)
                .drag
                .take_if(|drag| drag.id == session_id);
            let _ = self.inner.provider.cancel_drag(grant.native_session_token);
            if let Some(drag) = cleanup {
                release_representations(&drag.representations);
            }
            self.inner.emit(DndEvent::DragCancelled {
                session_id,
                reason: DragCancelReason::SourceSurfaceDisposed,
            });
            return self.inner.fail(DndOperation::StartDragSession, error);
        }
        if let Some(reason) = self.inner.interaction_cancelled(&interaction) {
            let cleanup = lock(&self.inner.state)
                .drag
                .take_if(|drag| drag.id == session_id);
            let _ = self.inner.provider.cancel_drag(grant.native_session_token);
            if let Some(drag) = cleanup {
                release_representations(&drag.representations);
            }
            self.inner.emit(DndEvent::DragCancelled {
                session_id,
                reason: DragCancelReason::CallerCancelled,
            });
            return Ok(Completion::Cancelled(reason));
        }

        {
            let mut state = lock(&self.inner.state);
            let Some(drag) = state.drag.as_mut().filter(|drag| drag.id == session_id) else {
                let _ = self.inner.provider.cancel_drag(grant.native_session_token);
                return self.inner.fail(
                    DndOperation::StartDragSession,
                    context_expired("Drag session disappeared during provider start"),
                );
            };
            drag.native_token = grant.native_session_token;
            drag.epoch = grant.session_epoch;
        }
        self.inner.emit(DndEvent::DragStarted {
            session_id,
            source_surface,
            generation: source_generation,
            format_count: spec.representations.len(),
        });
        Ok(Completion::Completed(DragSessionLease::new(
            Rc::clone(&self.inner),
            session_id,
        )))
    }

    fn stale_update(
        &self,
        session_id: DragSessionId,
        reason: StaleCallbackReason,
    ) -> ServiceResult<DragUpdate> {
        self.inner.record_stale(Some(session_id), reason);
        completed(DragUpdate::ignored(session_id, reason))
    }

    fn validate_callback_locked(
        state: &ServiceState,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
    ) -> Result<DragRecord, StaleCallbackReason> {
        let Some(drag) = state.drag.as_ref().filter(|drag| drag.id == session_id) else {
            return Err(StaleCallbackReason::UnknownSession);
        };
        if !drag.is_active() {
            return Err(match drag.state {
                DragSessionStatus::Completed(_) => StaleCallbackReason::SessionCompleted,
                DragSessionStatus::Cancelled(_) => StaleCallbackReason::SessionCancelled,
                DragSessionStatus::Active => unreachable!("active checked above"),
            });
        }
        if drag.native_token != native_session_token {
            return Err(StaleCallbackReason::TokenMismatch);
        }
        if drag.epoch != session_epoch {
            return Err(StaleCallbackReason::EpochMismatch);
        }
        Ok(drag.clone())
    }

    fn candidates_locked(
        state: &ServiceState,
        drag: &DragRecord,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> Vec<Candidate> {
        if !ServiceInner::surface_live_locked(state, surface, generation) {
            return Vec::new();
        }
        let offers = drag.offers();
        let mut candidates = state
            .targets
            .values()
            .filter(|target| {
                target.spec.surface == surface
                    && target.spec.generation == generation
                    && target.spec.rect.contains(point)
            })
            .filter_map(|target| {
                let negotiated = negotiate_format(&offers, &target.spec.accepted_formats)?;
                let allowed = drag
                    .source_effects
                    .intersection(target.spec.allowed_effects);
                let effect = allowed.choose(modifiers)?;
                Some(Candidate {
                    target_id: target.id,
                    spec: target.spec.clone(),
                    negotiated,
                    effect,
                    source_effects: drag.source_effects,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .spec
                .z_index
                .cmp(&left.spec.z_index)
                .then_with(|| left.target_id.0.cmp(&right.target_id.0))
        });
        candidates
    }

    fn evaluate_candidate(
        &self,
        drag: &DragRecord,
        candidate: &Candidate,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> DropTargetVerdict {
        let Some(evaluator) = candidate.spec.evaluator.as_ref() else {
            return DropTargetVerdict::Accept;
        };
        evaluator(&DropEvaluationContext {
            session_id: drag.id,
            target_id: candidate.target_id,
            point,
            modifiers,
            format: candidate.negotiated.descriptor.clone(),
            effect: candidate.effect,
            source_effects: drag.source_effects,
            target_effects: candidate.spec.allowed_effects,
        })
    }

    fn revalidate_candidate(
        &self,
        session_id: DragSessionId,
        token: u64,
        epoch: u64,
        candidate: &Candidate,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<(), StaleCallbackReason> {
        let state = lock(&self.inner.state);
        let drag = Self::validate_callback_locked(&state, session_id, token, epoch)?;
        if !ServiceInner::surface_live_locked(&state, drag.source_surface, drag.source_generation) {
            return Err(StaleCallbackReason::SurfaceNotLive);
        }
        if !ServiceInner::surface_live_locked(&state, surface, generation) {
            return Err(StaleCallbackReason::SurfaceNotLive);
        }
        let Some(target) = state.targets.get(&candidate.target_id) else {
            return Err(StaleCallbackReason::UnknownTarget);
        };
        if target.spec.surface != surface || target.spec.generation != generation {
            return Err(StaleCallbackReason::GenerationMismatch);
        }
        Ok(())
    }

    fn choose_candidate(
        &self,
        drag: &DragRecord,
        candidates: &[Candidate],
        point: DragPoint,
        modifiers: DragModifiers,
        surface: SurfaceId,
        generation: u64,
    ) -> Result<Option<Candidate>, StaleCallbackReason> {
        for candidate in candidates {
            let verdict = self.evaluate_candidate(drag, candidate, point, modifiers);
            match verdict {
                DropTargetVerdict::Reject => continue,
                DropTargetVerdict::Accept => {}
                DropTargetVerdict::AcceptWithEffect(effect) => {
                    if !candidate.source_effects.contains(effect)
                        || !candidate.spec.allowed_effects.contains(effect)
                    {
                        continue;
                    }
                    self.revalidate_candidate(
                        drag.id,
                        drag.native_token,
                        drag.epoch,
                        candidate,
                        surface,
                        generation,
                    )?;
                    return Ok(Some(Candidate {
                        effect,
                        ..candidate.clone()
                    }));
                }
            }
            self.revalidate_candidate(
                drag.id,
                drag.native_token,
                drag.epoch,
                candidate,
                surface,
                generation,
            )?;
            return Ok(Some(candidate.clone()));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn update_drag(
        &self,
        phase: DragUpdatePhase,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DragUpdate> {
        if surface == SurfaceId(0) || generation == 0 {
            return self.inner.fail(
                DndOperation::AcceptDropTarget,
                invalid("A drag callback requires a nonzero surface and generation"),
            );
        }
        self.inner
            .gate_capability(DndOperation::AcceptDropTarget)
            .inspect_err(|error| {
                self.inner
                    .emit_failure(DndOperation::AcceptDropTarget, error);
            })?;
        let (drag, candidates) = {
            let state = lock(&self.inner.state);
            let drag = match Self::validate_callback_locked(
                &state,
                session_id,
                native_session_token,
                session_epoch,
            ) {
                Ok(drag) => drag,
                Err(reason) => {
                    drop(state);
                    return self.stale_update(session_id, reason);
                }
            };
            if !ServiceInner::surface_live_locked(&state, surface, generation) {
                drop(state);
                return self.stale_update(session_id, StaleCallbackReason::SurfaceNotLive);
            }
            if !ServiceInner::surface_live_locked(
                &state,
                drag.source_surface,
                drag.source_generation,
            ) {
                drop(state);
                return self.stale_update(session_id, StaleCallbackReason::SurfaceNotLive);
            }
            let candidates =
                Self::candidates_locked(&state, &drag, surface, generation, point, modifiers);
            (drag, candidates)
        };

        if let Some(reason) = drag.context.cancellation.reason() {
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::CallerCancelled);
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = drag.context.check_at(Instant::now()) {
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::CallerCancelled);
            return self.inner.fail(DndOperation::AcceptDropTarget, error);
        }

        let chosen = match self.choose_candidate(
            &drag,
            &candidates,
            point,
            modifiers,
            surface,
            generation,
        ) {
            Ok(chosen) => chosen,
            Err(reason) => return self.stale_update(session_id, reason),
        };

        let event = match chosen {
            Some(candidate) => {
                let mut state = lock(&self.inner.state);
                let Some(current) = state.drag.as_mut().filter(|drag| drag.id == session_id) else {
                    drop(state);
                    return self.stale_update(session_id, StaleCallbackReason::UnknownSession);
                };
                if !current.is_active() {
                    let reason = match current.state {
                        DragSessionStatus::Completed(_) => StaleCallbackReason::SessionCompleted,
                        DragSessionStatus::Cancelled(_) => StaleCallbackReason::SessionCancelled,
                        DragSessionStatus::Active => unreachable!(),
                    };
                    drop(state);
                    return self.stale_update(session_id, reason);
                }
                current.current_target = Some(candidate.target_id);
                current.current_format = Some(candidate.negotiated.descriptor.clone());
                current.current_effect = Some(candidate.effect);
                match phase {
                    DragUpdatePhase::Enter => DndEvent::DragEntered {
                        session_id,
                        target_id: candidate.target_id,
                        format: candidate.negotiated.descriptor.clone(),
                        effect: candidate.effect,
                    },
                    DragUpdatePhase::Over => DndEvent::DragOver {
                        session_id,
                        target_id: Some(candidate.target_id),
                        format: Some(candidate.negotiated.descriptor.clone()),
                        effect: Some(candidate.effect),
                    },
                }
            }
            None => {
                let mut state = lock(&self.inner.state);
                if let Some(current) = state.drag.as_mut().filter(|drag| drag.id == session_id)
                    && current.is_active()
                {
                    current.current_target = None;
                    current.current_format = None;
                    current.current_effect = None;
                }
                DndEvent::DragOver {
                    session_id,
                    target_id: None,
                    format: None,
                    effect: None,
                }
            }
        };
        self.inner.emit(event);
        let state = lock(&self.inner.state);
        let current = state.drag.as_ref().filter(|drag| drag.id == session_id);
        let update = DragUpdate {
            session_id,
            target_id: current.and_then(|drag| drag.current_target),
            format: current.and_then(|drag| drag.current_format.clone()),
            effect: current.and_then(|drag| drag.current_effect),
            stale: None,
        };
        drop(state);
        completed(update)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn drag_enter(
        &self,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DragUpdate> {
        self.update_drag(
            DragUpdatePhase::Enter,
            session_id,
            native_session_token,
            session_epoch,
            surface,
            generation,
            point,
            modifiers,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn drag_over(
        &self,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DragUpdate> {
        self.update_drag(
            DragUpdatePhase::Over,
            session_id,
            native_session_token,
            session_epoch,
            surface,
            generation,
            point,
            modifiers,
        )
    }

    pub fn drag_leave(
        &self,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
    ) -> ServiceResult<DragUpdate> {
        let target = {
            let mut state = lock(&self.inner.state);
            let drag = match Self::validate_callback_locked(
                &state,
                session_id,
                native_session_token,
                session_epoch,
            ) {
                Ok(drag) => drag,
                Err(reason) => {
                    drop(state);
                    return self.stale_update(session_id, reason);
                }
            };
            let target = drag.current_target;
            if let Some(current) = state.drag.as_mut().filter(|drag| drag.id == session_id) {
                current.current_target = None;
                current.current_format = None;
                current.current_effect = None;
            }
            target
        };
        self.inner.emit(DndEvent::DragLeft {
            session_id,
            target_id: target,
        });
        completed(DragUpdate {
            session_id,
            target_id: None,
            format: None,
            effect: None,
            stale: None,
        })
    }

    /// Complete a drag at the supplied target point. Re-evaluates the target,
    /// format, effect and custom evaluator; a rejected drop cancels the session
    /// with `DragCancelReason::ForbiddenTarget` and releases promised resources.
    #[allow(clippy::too_many_arguments)]
    pub fn complete_drop(
        &self,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DropCompletion> {
        if surface == SurfaceId(0) || generation == 0 {
            return self.inner.fail(
                DndOperation::AcceptDropTarget,
                invalid("A drop callback requires a nonzero surface and generation"),
            );
        }
        if let Err(error) = self.inner.gate_capability(DndOperation::AcceptDropTarget) {
            return self.inner.fail(DndOperation::AcceptDropTarget, error);
        }
        let (drag, candidates) = {
            let state = lock(&self.inner.state);
            let drag = match Self::validate_callback_locked(
                &state,
                session_id,
                native_session_token,
                session_epoch,
            ) {
                Ok(drag) => drag,
                Err(reason) => {
                    drop(state);
                    self.inner.record_stale(Some(session_id), reason);
                    return completed(DropCompletion::Ignored(reason));
                }
            };
            if !ServiceInner::surface_live_locked(&state, surface, generation)
                || !ServiceInner::surface_live_locked(
                    &state,
                    drag.source_surface,
                    drag.source_generation,
                )
            {
                drop(state);
                self.inner
                    .record_stale(Some(session_id), StaleCallbackReason::SurfaceNotLive);
                return completed(DropCompletion::Ignored(StaleCallbackReason::SurfaceNotLive));
            }
            let candidates =
                Self::candidates_locked(&state, &drag, surface, generation, point, modifiers);
            (drag, candidates)
        };

        if let Some(reason) = drag.context.cancellation.reason() {
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::CallerCancelled);
            return Ok(Completion::Cancelled(reason));
        }
        if let Err(error) = drag.context.check_at(Instant::now()) {
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::CallerCancelled);
            return self.inner.fail(DndOperation::AcceptDropTarget, error);
        }

        let chosen = match self.choose_candidate(
            &drag,
            &candidates,
            point,
            modifiers,
            surface,
            generation,
        ) {
            Ok(Some(candidate)) => candidate,
            Ok(None) => {
                return Ok(Completion::Completed(self.forbid_drop(session_id)));
            }
            Err(reason) => {
                self.inner.record_stale(Some(session_id), reason);
                return completed(DropCompletion::Ignored(reason));
            }
        };

        let payload = {
            let representation = drag
                .representations
                .iter()
                .find(|representation| representation.descriptor() == chosen.negotiated.descriptor)
                .cloned();
            let Some(representation) = representation else {
                return Ok(Completion::Completed(self.forbid_drop(session_id)));
            };
            match representation {
                SourceRepresentation::Eager(payload) => payload,
                SourceRepresentation::Delayed(delayed) => {
                    let result = delayed.invoke(&drag.context);
                    let payload = match result {
                        Ok(Completion::Completed(payload)) => payload,
                        Ok(Completion::Cancelled(reason)) => {
                            let _ = self
                                .inner
                                .cancel_session(session_id, DragCancelReason::CallerCancelled);
                            return Ok(Completion::Cancelled(reason));
                        }
                        Err(error) => {
                            let _ = self
                                .inner
                                .cancel_session(session_id, DragCancelReason::ForbiddenTarget);
                            return self.inner.fail(DndOperation::AcceptDropTarget, error);
                        }
                    };
                    let post = {
                        let state = lock(&self.inner.state);
                        Self::validate_callback_locked(
                            &state,
                            session_id,
                            native_session_token,
                            session_epoch,
                        )
                        .is_ok()
                            && ServiceInner::surface_live_locked(
                                &state,
                                drag.source_surface,
                                drag.source_generation,
                            )
                            && ServiceInner::surface_live_locked(&state, surface, generation)
                    };
                    if let Some(reason) = drag.context.cancellation.reason() {
                        payload.release_resources();
                        let _ = self
                            .inner
                            .cancel_session(session_id, DragCancelReason::CallerCancelled);
                        return Ok(Completion::Cancelled(reason));
                    }
                    if !post {
                        payload.release_resources();
                        let _ = self
                            .inner
                            .cancel_session(session_id, DragCancelReason::SourceSurfaceDisposed);
                        return self.inner.fail(
                            DndOperation::AcceptDropTarget,
                            context_expired(
                                "The drag source or target surface was revoked during delayed payload generation",
                            ),
                        );
                    }
                    if let Err(error) = drag.context.check_at(Instant::now()) {
                        payload.release_resources();
                        let _ = self
                            .inner
                            .cancel_session(session_id, DragCancelReason::CallerCancelled);
                        return self.inner.fail(DndOperation::AcceptDropTarget, error);
                    }
                    payload
                }
            }
        };

        if !payload.conforms_to(&chosen.negotiated.descriptor) {
            payload.release_resources();
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::ForbiddenTarget);
            return self.inner.fail(
                DndOperation::AcceptDropTarget,
                crate::error::integrity(
                    "Deferred drag payload does not conform to the negotiated format",
                ),
            );
        }
        if let Err(error) = payload.validate() {
            payload.release_resources();
            let _ = self
                .inner
                .cancel_session(session_id, DragCancelReason::ForbiddenTarget);
            return self.inner.fail(DndOperation::AcceptDropTarget, error);
        }

        let outcome = DropOutcome {
            session_id,
            target_id: chosen.target_id,
            effect: chosen.effect,
            format: chosen.negotiated.descriptor.clone(),
        };
        let (token, unconsumed) = {
            let mut state = lock(&self.inner.state);
            let Some(current) = state.drag.as_mut().filter(|drag| drag.id == session_id) else {
                drop(state);
                payload.release_resources();
                return completed(DropCompletion::Ignored(StaleCallbackReason::UnknownSession));
            };
            if !current.is_active() {
                let reason = match current.state {
                    DragSessionStatus::Completed(_) => StaleCallbackReason::SessionCompleted,
                    DragSessionStatus::Cancelled(_) => StaleCallbackReason::SessionCancelled,
                    DragSessionStatus::Active => unreachable!(),
                };
                drop(state);
                payload.release_resources();
                return completed(DropCompletion::Ignored(reason));
            }
            let unconsumed = current
                .representations
                .iter()
                .filter(|representation| {
                    representation.descriptor() != chosen.negotiated.descriptor
                })
                .cloned()
                .collect::<Vec<_>>();
            current.state = DragSessionStatus::Completed(outcome.clone());
            current.current_target = None;
            current.current_format = None;
            current.current_effect = None;
            (current.native_token, unconsumed)
        };
        release_representations(&unconsumed);
        let provider_outcome = ProviderDropOutcome {
            session_id,
            target_id: chosen.target_id,
            format: chosen.negotiated.descriptor.clone(),
            effect: chosen.effect,
        };
        if token != 0
            && let Err(error) = self.inner.provider.complete_drag(token, &provider_outcome)
        {
            self.inner
                .emit_failure(DndOperation::AcceptDropTarget, &error);
        }
        self.inner.emit(DndEvent::DropCompleted {
            session_id,
            target_id: chosen.target_id,
            effect: chosen.effect,
            format: chosen.negotiated.descriptor.clone(),
        });
        let consumed = ConsumedPayload::new(
            chosen.negotiated.descriptor,
            chosen.negotiated.delivery,
            payload,
            Arc::clone(&self.inner.budget),
        );
        completed(DropCompletion::Dropped {
            outcome,
            payload: consumed,
        })
    }

    fn forbid_drop(&self, session_id: DragSessionId) -> DropCompletion {
        let _ = self
            .inner
            .cancel_session(session_id, DragCancelReason::ForbiddenTarget);
        DropCompletion::Cancelled(DragCancelReason::ForbiddenTarget)
    }

    /// Explicitly cancel a drag session. Duplicate terminal callbacks are
    /// deterministic and never resurrect the session.
    pub fn cancel_drag(
        &self,
        session_id: DragSessionId,
        native_session_token: u64,
        session_epoch: u64,
        reason: DragCancelReason,
    ) -> ServiceResult<DragCancelOutcome> {
        {
            let state = lock(&self.inner.state);
            match Self::validate_callback_locked(
                &state,
                session_id,
                native_session_token,
                session_epoch,
            ) {
                Ok(_) => {}
                Err(reason) => {
                    let terminal = state
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.id == session_id && !drag.is_active());
                    drop(state);
                    if terminal
                        && matches!(
                            reason,
                            StaleCallbackReason::SessionCompleted
                                | StaleCallbackReason::SessionCancelled
                        )
                    {
                        return completed(DragCancelOutcome::AlreadyTerminal);
                    }
                    self.inner.record_stale(Some(session_id), reason);
                    return completed(DragCancelOutcome::Ignored(reason));
                }
            }
        }
        completed(self.inner.cancel_session(session_id, reason))
    }

    pub fn drag_status(&self, session_id: DragSessionId) -> Option<DragSessionStatus> {
        lock(&self.inner.state)
            .drag
            .as_ref()
            .filter(|drag| drag.id == session_id)
            .map(|drag| drag.state.clone())
    }
}

enum DragUpdatePhase {
    Enter,
    Over,
}

#[derive(Clone)]
struct Candidate {
    target_id: DropTargetId,
    spec: DropTargetSpec,
    negotiated: NegotiatedFormat,
    effect: DragEffect,
    source_effects: AllowedEffects,
}

/// RAII ownership of one registered drop target.
pub struct DropTargetLease {
    inner: Rc<ServiceInner>,
    target_id: DropTargetId,
}

impl DropTargetLease {
    pub const fn id(&self) -> DropTargetId {
        self.target_id
    }

    pub fn is_active(&self) -> bool {
        lock(&self.inner.state)
            .targets
            .contains_key(&self.target_id)
    }

    /// Idempotent unregister. Returns true on the transition.
    pub fn unregister(&self) -> bool {
        let removed = lock(&self.inner.state)
            .targets
            .remove(&self.target_id)
            .is_some();
        if removed {
            self.inner.emit(DndEvent::DropTargetRemoved {
                target_id: self.target_id,
            });
        }
        removed
    }
}

impl Drop for DropTargetLease {
    fn drop(&mut self) {
        let removed = lock(&self.inner.state)
            .targets
            .remove(&self.target_id)
            .is_some();
        if removed {
            self.inner.emit(DndEvent::DropTargetRemoved {
                target_id: self.target_id,
            });
        }
    }
}

impl fmt::Debug for DropTargetLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropTargetLease")
            .field("target_id", &self.target_id)
            .field("active", &self.is_active())
            .finish()
    }
}

/// RAII ownership of one active drag session. Dropping an active lease cancels
/// the session, revokes delayed callbacks and releases promised resources.
pub struct DragSessionLease {
    inner: Rc<ServiceInner>,
    session_id: DragSessionId,
}

impl DragSessionLease {
    fn new(inner: Rc<ServiceInner>, session_id: DragSessionId) -> Self {
        Self { inner, session_id }
    }

    fn service(&self) -> RichTransferService {
        RichTransferService {
            inner: Rc::clone(&self.inner),
        }
    }

    pub const fn id(&self) -> DragSessionId {
        self.session_id
    }

    pub const fn session_id(&self) -> DragSessionId {
        self.session_id
    }

    pub fn native_session_token(&self) -> Option<u64> {
        lock(&self.inner.state)
            .drag
            .as_ref()
            .filter(|drag| drag.id == self.session_id)
            .map(|drag| drag.native_token)
            .filter(|token| *token != 0)
    }

    pub fn session_epoch(&self) -> Option<u64> {
        lock(&self.inner.state)
            .drag
            .as_ref()
            .filter(|drag| drag.id == self.session_id)
            .map(|drag| drag.epoch)
            .filter(|epoch| *epoch != 0)
    }

    pub fn is_active(&self) -> bool {
        lock(&self.inner.state)
            .drag
            .as_ref()
            .is_some_and(|drag| drag.id == self.session_id && drag.is_active())
    }

    pub fn status(&self) -> Option<DragSessionStatus> {
        lock(&self.inner.state)
            .drag
            .as_ref()
            .filter(|drag| drag.id == self.session_id)
            .map(|drag| drag.state.clone())
    }

    fn correlation(&self) -> Result<(u64, u64), SystemError> {
        let state = lock(&self.inner.state);
        let Some(drag) = state
            .drag
            .as_ref()
            .filter(|drag| drag.id == self.session_id)
        else {
            return Err(context_expired("Drag session no longer exists"));
        };
        if drag.native_token == 0 || drag.epoch == 0 {
            return Err(context_expired(
                "Drag session has no live native provider correlation",
            ));
        }
        Ok((drag.native_token, drag.epoch))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn drag_enter(
        &self,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DragUpdate> {
        let (token, epoch) = self.correlation()?;
        self.service().drag_enter(
            self.session_id,
            token,
            epoch,
            surface,
            generation,
            point,
            modifiers,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn drag_over(
        &self,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DragUpdate> {
        let (token, epoch) = self.correlation()?;
        self.service().drag_over(
            self.session_id,
            token,
            epoch,
            surface,
            generation,
            point,
            modifiers,
        )
    }

    pub fn drag_leave(&self) -> ServiceResult<DragUpdate> {
        let (token, epoch) = self.correlation()?;
        self.service().drag_leave(self.session_id, token, epoch)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_drop(
        &self,
        surface: SurfaceId,
        generation: u64,
        point: DragPoint,
        modifiers: DragModifiers,
    ) -> ServiceResult<DropCompletion> {
        let (token, epoch) = self.correlation()?;
        self.service().complete_drop(
            self.session_id,
            token,
            epoch,
            surface,
            generation,
            point,
            modifiers,
        )
    }

    pub fn cancel(&self, reason: DragCancelReason) -> ServiceResult<DragCancelOutcome> {
        let (token, epoch) = self.correlation()?;
        self.service()
            .cancel_drag(self.session_id, token, epoch, reason)
    }
}

impl Drop for DragSessionLease {
    fn drop(&mut self) {
        let _ = self
            .inner
            .cancel_session(self.session_id, DragCancelReason::LeaseDropped);
    }
}

impl fmt::Debug for DragSessionLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DragSessionLease")
            .field("session_id", &self.session_id)
            .field("status", &self.status())
            .finish()
    }
}

impl fmt::Debug for RichTransferService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RichTransferService")
            .field("platform", &self.platform())
            .field("active_streams", &self.active_stream_leases())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::FormatDescriptor;
    use crate::model::TypedPayload;
    use crate::provider::FakeRichTransferProvider;
    use crate::stream::{HandoffScope, LazyRepresentation, RichClipboardWrite};
    use gpui_system::{CancelReason, CancellationToken, Event, UiOwner};
    use std::{
        cell::Cell,
        pin::Pin,
        task::{Context as TaskContext, Poll, Waker},
    };

    fn fixture(
        surface: u64,
    ) -> (
        FakeRichTransferProvider,
        RichTransferService,
        UiOwner,
        UiContext,
    ) {
        let fake = FakeRichTransferProvider::new();
        let service = RichTransferService::from_fake(fake.clone());
        let owner = UiOwner::new(SurfaceId(surface));
        let ui = owner.context(None).expect("ui context");
        service
            .attach_surface(SurfaceId(surface), 1, true)
            .expect("surface");
        (fake, service, owner, ui)
    }

    fn completed<T>(result: ServiceResult<T>) -> T {
        match result.expect("operation succeeded") {
            Completion::Completed(value) => value,
            Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
        }
    }

    fn drain_events(subscription: &EventSubscription<DndEvent>) -> Vec<Event<DndEvent>> {
        let mut events = Vec::new();
        loop {
            let Ok(mut next) = subscription.try_next() else {
                break;
            };
            let waker = Waker::noop();
            let mut context = TaskContext::from_waker(waker);
            match Pin::new(&mut next).poll(&mut context) {
                Poll::Ready(Some(envelope)) => events.push(envelope.payload),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }
        events
    }

    #[test]
    fn foreign_interactions_are_rejected_before_cancellation_is_observed() {
        let (_fake_a, service_a, _owner_a, ui_a) = fixture(91);
        let (_fake_b, service_b, _owner_b, _ui_b) = fixture(92);
        let cancellation = gpui_system::CancellationToken::default();
        cancellation.cancel(gpui_system::CancelReason::User);
        let interaction = service_a
            .user_initiated_with(ui_a)
            .expect("gesture")
            .with_cancellation(cancellation);
        let error = service_b
            .read_clipboard(interaction, &[FormatDescriptor::plain_text()])
            .expect_err("foreign interaction");
        assert_eq!(error.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn explicit_surface_interactions_require_an_attached_live_generation() {
        let (_fake, service, owner, _ui) = fixture(93);
        service.dispose_surface_generation(SurfaceId(93), 1);
        let error = service
            .user_initiated_for_surface(SurfaceId(93), 1)
            .expect_err("disposed generation");
        assert_eq!(error.code, ErrorCode::ContextExpired);
        owner.advance_generation().expect("advance");
        service
            .attach_surface(SurfaceId(93), 2, true)
            .expect("attach");
        assert!(service.user_initiated_for_surface(SurfaceId(93), 2).is_ok());
        let error = service
            .user_initiated_for_surface(SurfaceId(94), 1)
            .expect_err("unattached surface");
        assert_eq!(error.code, ErrorCode::ContextExpired);
    }

    #[test]
    fn hidden_surface_gestures_are_permission_denied() {
        let (_fake, service, _owner, _ui) = fixture(95);
        service
            .attach_surface(SurfaceId(95), 1, false)
            .expect("hide");
        let error = service
            .user_initiated_for_surface(SurfaceId(95), 1)
            .expect_err("hidden surface");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn replacing_clipboard_revokes_only_the_previous_delayed_callbacks() {
        let (fake, service, _owner, ui) = fixture(96);
        let calls = Rc::new(Cell::new(0u32));
        let captured = Rc::clone(&calls);
        let delayed = LazyRepresentation::new(FormatDescriptor::html(), move |_| {
            captured.set(captured.get().saturating_add(1));
            Ok(Completion::Completed(
                TypedPayload::html("<b>old</b>").expect("html"),
            ))
        });
        let first = completed(
            service.write_clipboard(
                service.user_initiated_with(ui.clone()).expect("gesture"),
                RichClipboardWrite::application()
                    .with_delayed(delayed)
                    .expect("write"),
            ),
        );
        let first_id = first.id();
        assert!(!first.is_delayed_revoked());
        let _second = completed(
            service.write_clipboard(
                service.user_initiated_with(ui).expect("gesture"),
                RichClipboardWrite::application()
                    .with_eager(TypedPayload::plain_text("new").expect("text"))
                    .expect("write"),
            ),
        );
        assert!(!first.is_active());
        assert!(first.receipt().is_none());
        assert_eq!(service.inner.handoff_receipt(first_id), None);
        assert_eq!(fake.released_delayed_tokens().len(), 1);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn revoked_handoff_is_not_revoked_or_released_twice_on_replacement() {
        let (fake, service, _owner, ui) = fixture(104);
        let delayed = LazyRepresentation::new(FormatDescriptor::html(), |_| {
            Ok(Completion::Completed(
                TypedPayload::html("<b>revoked</b>").expect("html"),
            ))
        });
        let lease = completed(
            service.write_clipboard(
                service.user_initiated_with(ui.clone()).expect("gesture"),
                RichClipboardWrite::application()
                    .with_delayed(delayed)
                    .expect("write"),
            ),
        );
        assert!(lease.revoke_delayed());
        assert_eq!(fake.released_delayed_tokens().len(), 1);

        let subscription = service.subscribe();
        let _replacement = completed(
            service.write_clipboard(
                service.user_initiated_with(ui).expect("gesture"),
                RichClipboardWrite::application()
                    .with_eager(TypedPayload::plain_text("replacement").expect("text"))
                    .expect("write"),
            ),
        );
        assert!(
            !drain_events(&subscription)
                .iter()
                .any(|event| matches!(event, Event::Value(DndEvent::HandoffRevoked { .. }))),
            "an already-revoked handoff must not emit a second revocation"
        );
        assert_eq!(fake.released_delayed_tokens().len(), 1);
    }

    #[test]
    fn application_scoped_delayed_content_survives_a_surface_disposal() {
        let (_fake, service, owner, ui) = fixture(97);
        let calls = Rc::new(Cell::new(0u32));
        let captured = Rc::clone(&calls);
        let delayed = LazyRepresentation::new(FormatDescriptor::html(), move |_| {
            captured.set(captured.get().saturating_add(1));
            Ok(Completion::Completed(
                TypedPayload::html("<b>app</b>").expect("html"),
            ))
        });
        let _lease = completed(
            service.write_clipboard(
                service.user_initiated_with(ui).expect("gesture"),
                RichClipboardWrite::application()
                    .with_delayed(delayed)
                    .expect("write"),
            ),
        );
        service.dispose_surface_generation(SurfaceId(97), 1);
        assert!(owner.context(None).is_ok(), "host owns the UiOwner");
        let error = service
            .user_initiated_for_surface(SurfaceId(97), 1)
            .expect_err("surface disposed");
        assert_eq!(error.code, ErrorCode::ContextExpired);
    }

    #[test]
    fn open_handoff_scope_metadata_is_public_safe() {
        let scope = HandoffScope::surface(SurfaceId(98), 1).expect("scope");
        assert_eq!(scope.surface_id(), Some(SurfaceId(98)));
        assert_eq!(scope.generation(), Some(1));
        assert_eq!(HandoffScope::Application.surface_id(), None);
        assert!(HandoffScope::surface(SurfaceId(0), 1).is_err());
    }
    #[test]
    fn mid_publish_cancellation_removes_provisional_handoff() {
        let (fake, service, _owner, ui) = fixture(99);
        let existing = completed(
            service.write_clipboard(
                service.user_initiated_with(ui.clone()).expect("gesture"),
                RichClipboardWrite::application()
                    .with_eager(TypedPayload::plain_text("old").expect("text"))
                    .expect("write"),
            ),
        );
        let existing_id = existing.id();

        let cancellation = CancellationToken::default();
        let hook_cancellation = cancellation.clone();
        let published_id = Rc::new(Cell::new(None));
        let hook_id = Rc::clone(&published_id);
        fake.set_on_publish(Some(Rc::new(move |event| {
            hook_id.set(Some(event.handoff_id));
            hook_cancellation.cancel(CancelReason::User);
        })));
        let delayed = LazyRepresentation::new(FormatDescriptor::html(), |_| {
            Ok(Completion::Completed(
                TypedPayload::html("<b>cancelled</b>").expect("html"),
            ))
        });
        let write = RichClipboardWrite::application()
            .with_eager(TypedPayload::plain_text("cancelled").expect("text"))
            .expect("eager")
            .with_delayed(delayed)
            .expect("delayed");
        let interaction = service
            .user_initiated_with(ui.clone())
            .expect("gesture")
            .with_cancellation(cancellation);
        match service
            .write_clipboard(interaction, write)
            .expect("cancelled")
        {
            Completion::Cancelled(CancelReason::User) => {}
            other => panic!("unexpected publication result: {other:?}"),
        }
        let cancelled_id = published_id.get().expect("published handoff id");
        assert!(!service.inner.handoff_is_active(cancelled_id));
        assert!(existing.is_active(), "previous handoff must survive");
        assert_eq!(fake.released_delayed_tokens().len(), 1);
        fake.set_on_publish(None);

        // A later successful write replaces only the previous live handoff. It
        // must not rediscover and revoke a dead cancelled handoff as `Replaced`.
        let subscription = service.subscribe();
        let _replacement = completed(
            service.write_clipboard(
                service.user_initiated_with(ui).expect("gesture"),
                RichClipboardWrite::application()
                    .with_eager(TypedPayload::plain_text("replacement").expect("text"))
                    .expect("write"),
            ),
        );
        let revoked = drain_events(&subscription)
            .into_iter()
            .filter_map(|event| match event {
                Event::Value(DndEvent::HandoffRevoked { handoff_id, .. }) => Some(handoff_id),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(revoked, vec![existing_id]);
        assert!(!existing.is_active());
    }

    #[test]
    fn post_publish_failure_removes_provisional_handoff() {
        let (fake, service, owner, ui) = fixture(100);
        let published_id = Rc::new(Cell::new(None));
        let hook_id = Rc::clone(&published_id);
        let hook_owner = owner.clone();
        fake.set_on_publish(Some(Rc::new(move |event| {
            hook_id.set(Some(event.handoff_id));
            hook_owner
                .advance_generation()
                .expect("advance during publish");
        })));
        let delayed = LazyRepresentation::new(FormatDescriptor::html(), |_| {
            Ok(Completion::Completed(
                TypedPayload::html("<b>failed</b>").expect("html"),
            ))
        });
        let write = RichClipboardWrite::application()
            .with_delayed(delayed)
            .expect("write");
        let error = service
            .write_clipboard(service.user_initiated_with(ui).expect("gesture"), write)
            .expect_err("UI generation changed mid-publication");
        assert_eq!(error.code, ErrorCode::ContextExpired);
        let cancelled_id = published_id.get().expect("published handoff id");
        assert!(!service.inner.handoff_is_active(cancelled_id));
        assert_eq!(fake.released_delayed_tokens().len(), 1);
        fake.set_on_publish(None);

        service
            .attach_surface(SurfaceId(100), 2, true)
            .expect("new generation");
        let ui = owner.context(None).expect("new ui");
        let subscription = service.subscribe();
        let replacement = completed(
            service.write_clipboard(
                service.user_initiated_with(ui).expect("gesture"),
                RichClipboardWrite::application()
                    .with_eager(TypedPayload::plain_text("recovered").expect("text"))
                    .expect("write"),
            ),
        );
        assert!(replacement.is_active());
        assert_eq!(fake.released_delayed_tokens().len(), 1);
        assert!(
            !drain_events(&subscription)
                .iter()
                .any(|event| matches!(event, Event::Value(DndEvent::HandoffRevoked { .. }))),
            "a failed publication must not be re-revoked as Replaced"
        );
    }

    #[test]
    fn external_drag_notices_require_live_tokens_and_generations() {
        let (fake, service, _owner, _ui) = fixture(103);
        let surface = SurfaceId(103);
        let offer = || PayloadOffer::new(FormatDescriptor::plain_text(), DeliveryMode::Eager);

        let subscription = service.subscribe();
        fake.simulate_external_drag_enter_with_token(
            0,
            surface,
            1,
            DragPoint::from_milli(1, 1),
            [offer()],
        );
        assert_eq!(service.external_drag_notice_count(), 0);
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::TokenMismatch)
        );
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::StaleCallbackIgnored {
                session: None,
                reason: StaleCallbackReason::TokenMismatch
            })]
        ));

        service.dispose_surface_generation(surface, 1);
        let subscription = service.subscribe();
        fake.simulate_external_drag_enter_with_token(
            7,
            surface,
            1,
            DragPoint::from_milli(2, 2),
            [offer()],
        );
        assert_eq!(service.external_drag_notice_count(), 0);
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::SurfaceNotLive)
        );
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::StaleCallbackIgnored {
                session: None,
                reason: StaleCallbackReason::SurfaceNotLive
            })]
        ));

        service
            .attach_surface(surface, 2, true)
            .expect("new surface");
        let subscription = service.subscribe();
        let token = fake.simulate_external_drag_enter_with_token(
            11,
            surface,
            2,
            DragPoint::from_milli(3, 3),
            [offer()],
        );
        assert_eq!(service.external_drag_notice_count(), 1);
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::ExternalDragEntered {
                native_session_token: 11,
                ..
            })]
        ));

        fake.simulate_external_drag_leave(token);
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::ExternalDragLeft {
                native_session_token: 11
            })]
        ));
        fake.simulate_external_drag_leave(token);
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::UnknownSession)
        );
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::StaleCallbackIgnored {
                session: None,
                reason: StaleCallbackReason::UnknownSession
            })]
        ));
        fake.simulate_external_drop(token, DragPoint::from_milli(4, 4));
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::StaleCallbackIgnored {
                session: None,
                reason: StaleCallbackReason::UnknownSession
            })]
        ));

        fake.simulate_external_drag_leave(0);
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::TokenMismatch)
        );
        fake.simulate_external_drop(0, DragPoint::from_milli(5, 5));
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::TokenMismatch)
        );
        drain_events(&subscription);

        let token =
            fake.simulate_external_drag_enter(surface, 2, DragPoint::from_milli(6, 6), [offer()]);
        assert_eq!(service.external_drag_notice_count(), 2);
        drain_events(&subscription);
        fake.simulate_external_drop(token, DragPoint::from_milli(7, 7));
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::ExternalDrop {
                native_session_token,
                ..
            })] if *native_session_token == token
        ));

        let token =
            fake.simulate_external_drag_enter(surface, 2, DragPoint::from_milli(8, 8), [offer()]);
        assert_eq!(service.external_drag_notice_count(), 3);
        drain_events(&subscription);
        fake.simulate_source_exit();
        fake.simulate_external_drag_leave(token);
        assert_eq!(
            service.last_stale_callback(),
            Some(StaleCallbackReason::UnknownSession)
        );
        assert!(matches!(
            drain_events(&subscription).as_slice(),
            [Event::Value(DndEvent::StaleCallbackIgnored {
                session: None,
                reason: StaleCallbackReason::UnknownSession
            })]
        ));
    }
}
