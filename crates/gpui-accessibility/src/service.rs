//! Capability-gated accessibility service over a native/adaptor provider.

use crate::audit::SceneAuditReport;
use crate::error::SemanticError;
use crate::evidence::{
    AccessibilityAcceptanceReport, AssistiveReaderSessionEvidence, AutomationEvidence,
};
use crate::live::{
    LiveAnnouncement, LiveAnnouncementDisposition, LiveAnnouncementQueue, LiveAnnouncementReceipt,
};
use crate::platform::{AccessibilityOperation, AccessibilityPlatform};
use crate::provider::AccessibilityProvider;
use crate::scene::{
    AccessibilityActionReceipt, DisplayedScene, SemanticActionRequest, SemanticTreeSnapshot,
    TextSelectionReceipt, TextSelectionRequest,
};
use crate::tracker::SemanticSceneTracker;
use gpui_system::{
    Capability, Completion, ErrorCode, OperationContext, Request, SurfaceId, SystemError,
    SystemServices,
};
use std::{cell::RefCell, rc::Rc, time::Instant};

/// Capacity of the default bounded live-announcement queue.
pub const DEFAULT_LIVE_CAPACITY: usize = 32;

struct ServiceInner {
    provider: Rc<dyn AccessibilityProvider>,
    tracker: RefCell<SemanticSceneTracker>,
    live: RefCell<LiveAnnouncementQueue>,
    automation: RefCell<Vec<AutomationEvidence>>,
    assistive_reader: RefCell<Vec<AssistiveReaderSessionEvidence>>,
}

/// UI-local accessibility service. Clones share one semantic tracker, live
/// queue and provider.
#[derive(Clone)]
pub struct AccessibilityService(Rc<ServiceInner>);

impl AccessibilityService {
    pub fn new(provider: Rc<dyn AccessibilityProvider>) -> Self {
        Self::with_limits(
            provider,
            crate::tracker::DEFAULT_MAX_SURFACES,
            crate::tracker::DEFAULT_MAX_NODES,
            DEFAULT_LIVE_CAPACITY,
        )
    }

    pub fn from_provider(provider: impl AccessibilityProvider + 'static) -> Self {
        Self::new(Rc::new(provider))
    }

    pub fn with_limits(
        provider: Rc<dyn AccessibilityProvider>,
        max_surfaces: usize,
        max_nodes: usize,
        live_capacity: usize,
    ) -> Self {
        Self(Rc::new(ServiceInner {
            provider,
            tracker: RefCell::new(SemanticSceneTracker::with_limits(max_surfaces, max_nodes)),
            live: RefCell::new(LiveAnnouncementQueue::new(live_capacity)),
            automation: RefCell::new(Vec::new()),
            assistive_reader: RefCell::new(Vec::new()),
        }))
    }

    pub fn with_live_capacity(provider: Rc<dyn AccessibilityProvider>, capacity: usize) -> Self {
        Self::with_limits(
            provider,
            crate::tracker::DEFAULT_MAX_SURFACES,
            crate::tracker::DEFAULT_MAX_NODES,
            capacity,
        )
    }

    /// Register this concrete wrapper in `SystemServices`.
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }

    pub fn install_cloned(&self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self.clone())
    }

    pub fn provider(&self) -> &Rc<dyn AccessibilityProvider> {
        &self.0.provider
    }

    /// True when both handles share the same tracker, live queue and provider.
    pub fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn platform(&self) -> AccessibilityPlatform {
        self.0.provider.platform()
    }

    pub fn capability(
        &self,
        operation: AccessibilityOperation,
    ) -> Capability<AccessibilityOperation> {
        self.0.provider.capability(operation)
    }

    pub fn capabilities(&self) -> Vec<Capability<AccessibilityOperation>> {
        AccessibilityOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }

    pub fn snapshot(&self, surface: SurfaceId) -> Option<SemanticTreeSnapshot> {
        self.0.tracker.borrow().snapshot(surface).cloned()
    }

    pub fn identity_audit(
        &self,
        surface: SurfaceId,
    ) -> Option<crate::tracker::IdentityRetentionAudit> {
        self.0.tracker.borrow().identity_audit(surface).cloned()
    }

    pub fn active_generation(&self, surface: SurfaceId) -> Option<u64> {
        self.0.tracker.borrow().active_generation(surface)
    }

    pub fn live_queue_len(&self) -> usize {
        self.0.live.borrow().len()
    }

    pub fn live_dropped_count(&self) -> u64 {
        self.0.live.borrow().dropped_count()
    }

    pub fn dequeue_announcement(&self) -> Option<LiveAnnouncement> {
        self.0.live.borrow_mut().dequeue()
    }

    pub fn drain_announcements(&self) -> Vec<LiveAnnouncement> {
        self.0.live.borrow_mut().drain()
    }

    pub fn automation_evidence(&self) -> Vec<AutomationEvidence> {
        self.0.automation.borrow().clone()
    }

    pub fn assistive_reader_evidence(&self) -> Vec<AssistiveReaderSessionEvidence> {
        self.0.assistive_reader.borrow().clone()
    }

    /// Dispose one native surface, clearing its scene and pending live speech.
    pub fn dispose_surface(&self, surface: SurfaceId) {
        self.0.tracker.borrow_mut().dispose_surface(surface);
        self.0.live.borrow_mut().dispose_surface(surface.0);
    }

    /// Explicitly advance a generation and revoke old announcements. A full
    /// scene must arrive before dependent operations are accepted again.
    pub fn advance_generation(&self, scene: DisplayedScene) -> Result<(), SystemError> {
        self.0
            .tracker
            .borrow_mut()
            .advance_generation(scene.surface, scene.generation)?;
        self.0
            .live
            .borrow_mut()
            .advance_generation(scene)
            .map(|_| ())
            .map_err(SystemError::from)
    }

    /// Ingest an AccessKit update through the capability gate, then deliver the
    /// accepted snapshot to the provider. The tracker clone is discarded if the
    /// provider rejects the commit, so a failed native handoff cannot silently
    /// advance the canonical scene.
    pub fn observe_tree(
        &self,
        scene: DisplayedScene,
        update: accesskit::TreeUpdate,
        cx: OperationContext,
    ) -> Request<SemanticTreeSnapshot> {
        self.execute(
            AccessibilityOperation::ObserveTree,
            Some(scene.surface),
            false,
            cx,
            || {
                let mut tracker = self.0.tracker.borrow().clone();
                let snapshot = tracker.ingest_accesskit(scene, update)?;
                self.0.provider.commit_snapshot(&snapshot)?;
                *self.0.tracker.borrow_mut() = tracker;
                Ok(snapshot)
            },
        )
    }

    /// Commit a complete semantic snapshot. Incremental AccessKit updates can
    /// follow because the snapshot is converted back to AccessKit nodes.
    pub fn commit_scene(
        &self,
        snapshot: SemanticTreeSnapshot,
        cx: OperationContext,
    ) -> Request<SemanticTreeSnapshot> {
        let scene = snapshot.displayed_scene();
        self.execute(
            AccessibilityOperation::ObserveTree,
            Some(scene.surface),
            false,
            cx,
            || {
                let mut tracker = self.0.tracker.borrow().clone();
                let accepted = tracker.commit(snapshot)?;
                self.0.provider.commit_snapshot(&accepted)?;
                *self.0.tracker.borrow_mut() = tracker;
                Ok(accepted)
            },
        )
    }

    /// Dispatch a click/focus/value/selection/scroll/dismiss action. This is an
    /// interactive UI operation: a current UI context on the owning thread and
    /// an exact scene revision are required.
    pub fn perform_action(
        &self,
        request: SemanticActionRequest,
        cx: OperationContext,
    ) -> Request<AccessibilityActionReceipt> {
        let scene = request.scene;
        self.execute(
            AccessibilityOperation::PerformAction,
            Some(scene.surface),
            true,
            cx,
            || {
                request.validate()?;
                let snapshot = self.current_scene(scene)?;
                let node = snapshot
                    .node(&request.target)
                    .ok_or(SemanticError::UnknownNode {
                        node: request.target.node_id,
                    })?;
                if !node.supports_action(request.action) {
                    return Err(SemanticError::UnsupportedAction {
                        action: request.action,
                    }
                    .into());
                }
                self.0.provider.perform_action(&request, &snapshot)
            },
        )
    }

    /// Perform a directed text-selection edit and return the selection in UTF-8,
    /// scalar and UTF-16 units. The target value must match `request.text`.
    pub fn edit_text_selection(
        &self,
        request: TextSelectionRequest,
        cx: OperationContext,
    ) -> Request<TextSelectionReceipt> {
        let scene = request.scene;
        self.execute(
            AccessibilityOperation::EditTextSelection,
            Some(scene.surface),
            true,
            cx,
            || {
                request.validate()?;
                let snapshot = self.current_scene(scene)?;
                let node = snapshot
                    .node(&request.target)
                    .ok_or(SemanticError::UnknownNode {
                        node: request.target.node_id,
                    })?;
                if !node.supports_action(crate::scene::SemanticAction::SetTextSelection) {
                    return Err(SemanticError::UnsupportedAction {
                        action: crate::scene::SemanticAction::SetTextSelection,
                    }
                    .into());
                }
                if let Some(value) = &node.value
                    && value != &request.text
                {
                    let mut error = SystemError::new(
                        ErrorCode::InvalidInput,
                        "Text selection does not match the target value",
                    );
                    error.retry = gpui_system::RetryAdvice::ModifyRequest;
                    return Err(error);
                }
                self.0.provider.edit_text_selection(&request, &snapshot)
            },
        )
    }

    /// Queue one bounded live announcement. Identical consecutive announcements
    /// in the same scene revision are deduplicated.
    pub fn announce(
        &self,
        announcement: LiveAnnouncement,
        cx: OperationContext,
    ) -> Request<LiveAnnouncementReceipt> {
        let scene = announcement.scene;
        self.execute(
            AccessibilityOperation::AnnounceLive,
            Some(scene.surface),
            true,
            cx,
            || {
                self.current_scene(scene)?;
                // Stage the queue mutation so a provider failure cannot leave
                // a phantom announcement that would deduplicate a retry.
                let mut next = self.0.live.borrow().clone();
                let disposition = next.enqueue(announcement.clone())?;
                if disposition == LiveAnnouncementDisposition::Queued {
                    self.0.provider.announce_live(&announcement)?;
                }
                *self.0.live.borrow_mut() = next;
                let live = self.0.live.borrow();
                Ok(LiveAnnouncementReceipt {
                    disposition,
                    queue_len: live.len(),
                    dropped: live.dropped_count(),
                })
            },
        )
    }

    /// Audit a committed scene: structure, modal isolation and virtual-list
    /// invariants.
    pub fn audit_scene(
        &self,
        scene: DisplayedScene,
        cx: OperationContext,
    ) -> Request<SceneAuditReport> {
        self.execute(
            AccessibilityOperation::AuditScene,
            Some(scene.surface),
            false,
            cx,
            || {
                let snapshot = self.current_scene(scene)?;
                Ok(SceneAuditReport::inspect(&snapshot))
            },
        )
    }

    pub fn record_automation_evidence(
        &self,
        evidence: AutomationEvidence,
    ) -> Result<(), SystemError> {
        self.check_capability(AccessibilityOperation::AuditScene)?;
        evidence.validate()?;
        self.ensure_platform(evidence.platform)?;
        self.0.provider.record_automation_evidence(&evidence)?;
        self.0.automation.borrow_mut().push(evidence);
        Ok(())
    }

    pub fn record_assistive_reader_evidence(
        &self,
        evidence: AssistiveReaderSessionEvidence,
    ) -> Result<(), SystemError> {
        self.check_capability(AccessibilityOperation::AuditScene)?;
        evidence.validate()?;
        self.ensure_platform(evidence.platform)?;
        self.0
            .provider
            .record_assistive_reader_evidence(&evidence)?;
        self.0.assistive_reader.borrow_mut().push(evidence);
        Ok(())
    }

    /// Build a fresh report for this service platform. Recorded evidence is
    /// copied into explicit automation and reader gates; a passing automation
    /// check never marks the reader gate as passed.
    pub fn acceptance_report(&self) -> AccessibilityAcceptanceReport {
        let mut report = AccessibilityAcceptanceReport::new(self.platform());
        if let Some(evidence) = self.0.automation.borrow().last() {
            let _ = report.record_automation(evidence.clone());
        }
        if let Some(evidence) = self.0.assistive_reader.borrow().last() {
            let _ = report.record_assistive_reader_session(evidence.clone());
        }
        report
    }

    fn current_scene(&self, scene: DisplayedScene) -> Result<SemanticTreeSnapshot, SemanticError> {
        let tracker = self.0.tracker.borrow();
        tracker.validate_scene(scene)?;
        tracker
            .snapshot(scene.surface)
            .cloned()
            .ok_or(SemanticError::SurfaceNotTracked {
                surface: scene.surface,
            })
    }

    fn ensure_platform(&self, platform: AccessibilityPlatform) -> Result<(), SystemError> {
        if platform == self.platform() {
            return Ok(());
        }
        let mut error = SystemError::new(
            ErrorCode::InvalidConfiguration,
            "Evidence platform does not match the installed provider",
        );
        error.retry = gpui_system::RetryAdvice::ModifyRequest;
        Err(error)
    }

    fn check_capability(&self, operation: AccessibilityOperation) -> Result<(), SystemError> {
        self.0.provider.capability(operation).check()
    }

    fn check_context(
        &self,
        cx: &OperationContext,
        surface: Option<SurfaceId>,
        require_ui: bool,
    ) -> Result<(), SystemError> {
        cx.check_at(Instant::now())?;
        if require_ui {
            let ui = cx.require_ui()?;
            if surface.is_some_and(|surface| surface != ui.surface) {
                let mut error = SystemError::new(
                    ErrorCode::InvalidInput,
                    "UI context belongs to a different surface",
                );
                error.retry = gpui_system::RetryAdvice::ModifyRequest;
                return Err(error);
            }
        } else if let Some(ui) = &cx.ui
            && surface.is_some_and(|surface| surface != ui.surface)
        {
            let mut error = SystemError::new(
                ErrorCode::InvalidInput,
                "UI context belongs to a different surface",
            );
            error.retry = gpui_system::RetryAdvice::ModifyRequest;
            return Err(error);
        }
        Ok(())
    }

    fn execute<T>(
        &self,
        operation: AccessibilityOperation,
        surface: Option<SurfaceId>,
        require_ui: bool,
        cx: OperationContext,
        action: impl FnOnce() -> Result<T, SystemError>,
    ) -> Request<T> {
        let (request, port) = gpui_system::request(cx.clone());
        if let Some(reason) = cx.cancellation.reason() {
            port.complete(Ok(Completion::Cancelled(reason)));
            return request;
        }
        let result = self
            .check_capability(operation)
            .and_then(|_| self.check_context(&cx, surface, require_ui))
            .and_then(|_| action());
        port.complete(result.map(Completion::Completed));
        request
    }
}
