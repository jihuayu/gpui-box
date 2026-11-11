//! Deterministic, headless cross-service gallery workflows.
//!
//! Every provider is in-process and explicit. Blocking storage work receives a
//! background [`OperationContext`]; interactive work receives a foreground UI
//! context owned by this workbench. No workflow reads or prints a native path,
//! and the protected-secret workflow reports only redacted receipt metadata.

use gpui_activation::{
    ActivationId, ActivationInbox, ActivationPayload, ActivationTarget, InboxConfig, RouteRegistry,
    UrlPolicy,
};
use gpui_app_data::{AppData, DirectoryRoots};
use gpui_box_components::{ScreenModel, model::Navigation};
use gpui_clipboard::{
    Clipboard, ClipboardOperation, ClipboardProvider, ClipboardReceipt, ClipboardText,
};
use gpui_files::{
    DurableGrantSupport, ExportResourceLease, FileProvider, FileResource, FileService,
    OpenFileOptions, PickerOutcome, ScratchFileProvider,
};
use gpui_local_auth::{
    AccountId, AccountUnlockManager, AccountUnlockStatus, AppPhase, FakeLocalAuthProvider,
    FakeVerificationMode, LocalAuthHostState, LocalAuthService, LocalUnlockFlowModel,
    ProtectedKeyId, ProtectedSecretOutcome, ProtectedSecretRequest, SessionLockState, UnlockPolicy,
    VerificationOutcome, VerificationPolicy, VerificationPurpose,
};
use gpui_notifications::{
    ApplicationIdentity, FakeNotificationProvider, NotificationActivationBinding,
    NotificationActivationCallback, NotificationActivationConfig, NotificationActivationStore,
    NotificationContent, NotificationId, NotificationService, NotificationSubmission,
    PermissionKind, PermissionRequest, PermissionService, PermissionStatus,
};
use gpui_secrets::SecretBytes;
use gpui_sharing::{
    ShareItem, ShareOperation, ShareReceipt, ShareRequest, ShareResource, Sharing, SharingProvider,
};
use gpui_system::{
    Availability, Capability, Completion, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, Request, ServiceResult, Support, SurfaceId, SystemError, UiOwner,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    future::Future,
    io::Cursor,
    path::PathBuf,
    pin::Pin,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context as TaskContext, Poll, Waker},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCRATCH_PREFIX: &str = "gpui-box-workbench";
const NOTIFICATION_NAMESPACE: &str = "gallery";
const DOCUMENT_ROUTE: &str = "document";
const DOCUMENT_ID: &str = "reviewed-plan";
const REVIEWED_EDIT: &str = "纪要 · 已审阅 ✅";
const INITIAL_DRAFT: &str = "Initial reviewed draft";
const SECRET_CANARY: &[u8] = b"gpui-box-example-secret-canary";
const MAX_EXPORT_BYTES: usize = 1024 * 1024;

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Screen routes used by the cross-service gallery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GalleryRoute {
    Overview,
    Document { document_id: String },
}

/// A scratch root that owns its temporary directory and removes it on drop.
struct ScratchRoot {
    path: PathBuf,
}
impl ScratchRoot {
    fn create() -> Result<Self, SystemError> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        for _ in 0..8 {
            let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "{SCRATCH_PREFIX}-{}-{stamp}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    return Err(SystemError::new(
                        ErrorCode::PlatformFailure,
                        "Workbench scratch directory is unavailable",
                    ));
                }
            }
        }
        Err(SystemError::new(
            ErrorCode::PlatformFailure,
            "Workbench scratch directory could not be allocated",
        ))
    }
}
impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A staged consumer export that stays pinned until released or dropped.
pub struct ExportStaging {
    lease: Option<ExportResourceLease>,
}
impl ExportStaging {
    fn stage(
        app: &AppData,
        resource: &FileResource,
        max_bytes: usize,
        context: &OperationContext,
    ) -> Result<Self, SystemError> {
        let lease = completed(ExportResourceLease::lease_resource_for_export(
            app, resource, max_bytes, context,
        ))?;
        Ok(Self { lease: Some(lease) })
    }
    pub fn is_active(&self) -> bool {
        self.lease
            .as_ref()
            .is_some_and(ExportResourceLease::is_active)
    }
    pub fn bytes(&self) -> u64 {
        self.lease.as_ref().map_or(0, ExportResourceLease::bytes)
    }
    pub fn release(&mut self) -> bool {
        self.lease
            .as_mut()
            .is_some_and(ExportResourceLease::release)
    }
}

/// Deterministic sharing provider with an explicitly toggleable unsupported mode.
#[derive(Default)]
pub struct WorkbenchShareProvider {
    unsupported: Cell<bool>,
    calls: Cell<usize>,
}
impl WorkbenchShareProvider {
    pub fn set_unsupported(&self, unsupported: bool) {
        self.unsupported.set(unsupported);
    }
    pub fn is_unsupported(&self) -> bool {
        self.unsupported.get()
    }
    pub fn calls(&self) -> usize {
        self.calls.get()
    }
    fn capability_for(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        let unsupported = self.unsupported.get() && operation == ShareOperation::Files;
        Capability {
            operation,
            support: if unsupported {
                Support::Unsupported
            } else {
                Support::Native
            },
            availability: if unsupported {
                Availability::Unavailable
            } else {
                Availability::Available
            },
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::ContractOnly,
            constraints: vec![],
            observed_revision: 1,
        }
    }
}
impl SharingProvider for WorkbenchShareProvider {
    fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        self.capability_for(operation)
    }
    fn share(
        &self,
        _request: &ShareRequest,
        _context: &OperationContext,
    ) -> ServiceResult<ShareReceipt> {
        self.calls.set(self.calls.get() + 1);
        if self.unsupported.get() {
            return Err(SystemError::new(
                ErrorCode::Unsupported,
                "Native file sharing is unavailable in this workbench",
            ));
        }
        Ok(Completion::Completed(
            ShareReceipt::SubmittedOutcomeUnobservable,
        ))
    }
}

/// In-memory clipboard provider used only by the explicit fallback action.
#[derive(Default)]
pub struct WorkbenchClipboardProvider {
    contents: RefCell<Option<String>>,
}
impl WorkbenchClipboardProvider {
    pub fn contents(&self) -> Option<String> {
        self.contents.borrow().clone()
    }
}
impl ClipboardProvider for WorkbenchClipboardProvider {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        [ClipboardOperation::ReadText, ClipboardOperation::WriteText]
            .into_iter()
            .map(|operation| Capability {
                operation,
                support: Support::Native,
                availability: Availability::Available,
                configuration: Configuration::Ready,
                permission: PermissionState::NotRequired,
                evidence: Evidence::ContractOnly,
                constraints: vec![],
                observed_revision: 1,
            })
            .collect()
    }
    fn read_text(&self, _context: &OperationContext) -> ServiceResult<ClipboardText> {
        let text = self.contents.borrow().clone();
        Ok(Completion::Completed(match text {
            Some(text) => ClipboardText::Text(text),
            None => ClipboardText::Empty,
        }))
    }
    fn write_text(
        &self,
        text: &str,
        _context: &OperationContext,
    ) -> ServiceResult<ClipboardReceipt> {
        *self.contents.borrow_mut() = Some(text.to_owned());
        Ok(Completion::Completed(ClipboardReceipt {
            sequence: Some(1),
        }))
    }
}

/// The three end-to-end workflows plus the retained services that own them.
pub struct GalleryWorkbench {
    screen: ScreenModel<GalleryRoute>,
    app: AppData,
    permissions: PermissionService,
    notifications: NotificationService,
    sharing: Sharing,
    clipboard: Clipboard,
    local_auth: LocalAuthService,
    share_provider: Rc<WorkbenchShareProvider>,
    clipboard_provider: Rc<WorkbenchClipboardProvider>,
    notification_provider: Rc<FakeNotificationProvider>,
    local_auth_provider: Rc<FakeLocalAuthProvider>,
    permission_provider: Rc<gpui_permissions::FakePermissionProvider>,
    _owner: UiOwner,
    foreground: OperationContext,
    background: OperationContext,
    document: FileResource,
    draft: String,
    saved: Option<String>,
    staging: Option<ExportStaging>,
    staged_snapshot: Option<String>,
    share_resource: Option<ShareResource>,
    notification_export: Option<ExportResourceLease>,
    document_status: String,
    save_status: String,
    share_status: String,
    fallback_status: String,
    notification_id: NotificationId,
    notification_binding: Option<NotificationActivationBinding>,
    notification_store: Option<NotificationActivationStore>,
    inbox: Option<ActivationInbox>,
    routes: RouteRegistry,
    notification_status: String,
    activation_status: String,
    handled_activations: BTreeSet<String>,
    account: AccountId,
    flow: LocalUnlockFlowModel,
    manager: AccountUnlockManager,
    verification_policy: VerificationPolicy,
    verification_mode: FakeVerificationMode,
    protected_key: ProtectedKeyId,
    local_auth_status: String,
    unlock_status: String,
    secret_status: String,
    relock_status: String,
    _scratch: ScratchRoot,
}

impl GalleryWorkbench {
    pub fn new() -> Result<Self, SystemError> {
        let scratch = ScratchRoot::create()?;
        let app = AppData::open(DirectoryRoots::under(scratch.path.join("app-data")))?;
        let provider = Rc::new(ScratchFileProvider::open(scratch.path.join("files"))?);
        let seeded = provider.create_durable_file(
            "notes/plan.txt",
            INITIAL_DRAFT.as_bytes(),
            DurableGrantSupport::SecurityScopedBookmark,
        )?;
        provider.queue_selection(vec![seeded]);
        provider.set_auto_complete(true);
        let files = FileService::new(provider as Rc<dyn FileProvider>);
        let owner = UiOwner::new(SurfaceId(7));
        let foreground = OperationContext {
            ui: Some(owner.context(None)?),
            ..OperationContext::default()
        };
        let background = OperationContext::default();
        let mut picker = files.open_file(
            OpenFileOptions::new().with_prompt("Open reviewed document"),
            foreground.clone(),
        );
        let document = match resolve(&mut picker)? {
            PickerOutcome::Selected(resource) => resource,
            PickerOutcome::Cancelled => {
                return Err(SystemError::new(
                    ErrorCode::Unavailable,
                    "Deterministic document picker was cancelled",
                ));
            }
        };
        let draft = read_document(&document, &background)?;

        let permission_provider = Rc::new(
            gpui_permissions::FakePermissionProvider::new()
                .with_status(PermissionKind::Notifications, PermissionStatus::Granted),
        );
        let permissions = PermissionService::new(permission_provider.clone());
        let notification_provider = Rc::new(
            FakeNotificationProvider::new()
                .with_identity(ApplicationIdentity::Ready)
                .with_auto_display(true),
        );
        let notifications =
            NotificationService::new(notification_provider.clone(), permissions.clone());
        let share_provider = Rc::new(WorkbenchShareProvider::default());
        let ui = foreground
            .ui
            .clone()
            .ok_or_else(|| SystemError::new(ErrorCode::Unavailable, "UI context is missing"))?;
        let sharing = Sharing::new(share_provider.clone(), ui.clone());
        let clipboard_provider = Rc::new(WorkbenchClipboardProvider::default());
        let clipboard = Clipboard::new(clipboard_provider.clone(), ui)?;

        let local_auth_provider = Rc::new(FakeLocalAuthProvider::new());
        local_auth_provider.set_auto_verify(Some(FakeVerificationMode::Biometric));
        local_auth_provider.set_policy_fallback_required(true);
        let local_auth = LocalAuthService::new(local_auth_provider.clone());
        let account = AccountId::new("gallery-user")?;
        let protected_key = ProtectedKeyId::new("gallery-vault-key")?;
        local_auth_provider.seal_protected_secret(
            &account,
            &protected_key,
            SecretBytes::new(SECRET_CANARY.to_vec())?,
        );
        let unlock_policy = UnlockPolicy::new(
            Duration::from_secs(60),
            Duration::from_secs(600),
            Duration::from_secs(30),
        )?;
        let flow = LocalUnlockFlowModel::new(
            account.clone(),
            unlock_policy,
            VerificationPolicy::BiometricOrDeviceCredential,
        )?;
        let mut manager = AccountUnlockManager::new(unlock_policy)?;
        manager.set_active_account(Some(account.clone()));
        local_auth.reconcile_host_state(
            &LocalAuthHostState::foreground(SurfaceId(7), 1),
            Instant::now(),
        );

        let mut routes = RouteRegistry::default();
        routes.register(DOCUMENT_ROUTE, 1)?;
        let notification_id = NotificationId::new("gallery-document-ready")?;
        let notification_store = completed(NotificationActivationStore::open(
            &app,
            NOTIFICATION_NAMESPACE,
            NotificationActivationConfig::default(),
            routes.clone(),
            &background,
        ))?;
        let inbox = completed(ActivationInbox::open(
            &app,
            NOTIFICATION_NAMESPACE,
            InboxConfig::default(),
            UrlPolicy::new(["https"])?,
            routes.clone(),
            &background,
        ))?;
        inbox.set_ready(true);

        let mut screen = ScreenModel::new(GalleryRoute::Overview);
        screen.insert_service(files.clone())?;
        screen.insert_service(permissions.clone())?;
        screen.insert_service(notifications.clone())?;
        screen.insert_service(sharing.clone())?;
        screen.insert_service(clipboard.clone())?;
        screen.insert_service(local_auth.clone())?;

        Ok(Self {
            screen,
            app,
            permissions,
            notifications,
            sharing,
            clipboard,
            local_auth,
            share_provider,
            clipboard_provider,
            notification_provider,
            local_auth_provider,
            permission_provider,
            _owner: owner,
            foreground,
            background,
            document,
            draft,
            saved: None,
            staging: None,
            staged_snapshot: None,
            share_resource: None,
            notification_export: None,
            document_status: "Document ready · picker grant retained".into(),
            save_status: "Atomic save not run".into(),
            share_status: "Share not run".into(),
            fallback_status: "Clipboard fallback not used".into(),
            notification_id,
            notification_binding: None,
            notification_store: Some(notification_store),
            inbox: Some(inbox),
            routes,
            notification_status: "Notification not posted".into(),
            activation_status: "No activation routed".into(),
            handled_activations: BTreeSet::new(),
            account,
            flow,
            manager,
            verification_policy: VerificationPolicy::BiometricOrDeviceCredential,
            verification_mode: FakeVerificationMode::Biometric,
            protected_key,
            local_auth_status: "Local auth not probed".into(),
            unlock_status: "Session locked".into(),
            secret_status: "Protected secret not released".into(),
            relock_status: "No relock observed".into(),
            _scratch: scratch,
        })
    }

    pub fn screen(&self) -> &ScreenModel<GalleryRoute> {
        &self.screen
    }
    pub fn navigation(&self) -> &Navigation<GalleryRoute> {
        self.screen.navigation()
    }
    pub fn current_route(&self) -> &GalleryRoute {
        self.screen.navigation().current()
    }
    pub fn document_draft(&self) -> &str {
        &self.draft
    }
    pub fn document_status(&self) -> &str {
        &self.document_status
    }
    pub fn save_status(&self) -> &str {
        &self.save_status
    }
    pub fn share_status(&self) -> &str {
        &self.share_status
    }
    pub fn fallback_status(&self) -> &str {
        &self.fallback_status
    }
    pub fn notification_status(&self) -> &str {
        &self.notification_status
    }
    pub fn activation_status(&self) -> &str {
        &self.activation_status
    }
    pub fn local_auth_status(&self) -> &str {
        &self.local_auth_status
    }
    pub fn unlock_status(&self) -> &str {
        &self.unlock_status
    }
    pub fn secret_status(&self) -> &str {
        &self.secret_status
    }
    pub fn relock_status(&self) -> &str {
        &self.relock_status
    }
    pub fn staged_export_active(&self) -> bool {
        self.staging.as_ref().is_some_and(ExportStaging::is_active)
    }
    pub fn staged_export_bytes(&self) -> u64 {
        self.staging.as_ref().map_or(0, ExportStaging::bytes)
    }
    pub fn share_native_supported(&self) -> bool {
        !self.share_provider.is_unsupported()
    }
    pub fn share_calls(&self) -> usize {
        self.share_provider.calls()
    }
    pub fn share_handoff_retained(&self) -> bool {
        self.share_resource
            .as_ref()
            .is_some_and(|resource| resource.export_lease().is_active())
    }
    pub fn notification_export_bytes(&self) -> u64 {
        self.notification_export
            .as_ref()
            .map_or(0, ExportResourceLease::bytes)
    }
    pub fn handled_activation_count(&self) -> usize {
        self.handled_activations.len()
    }
    pub fn clipboard_contents(&self) -> Option<String> {
        self.clipboard_provider.contents()
    }
    pub fn notification_provider(&self) -> &Rc<FakeNotificationProvider> {
        &self.notification_provider
    }
    pub fn permission_provider(&self) -> &Rc<gpui_permissions::FakePermissionProvider> {
        &self.permission_provider
    }
    pub fn local_auth_provider(&self) -> &Rc<FakeLocalAuthProvider> {
        &self.local_auth_provider
    }
    pub fn verification_policy(&self) -> VerificationPolicy {
        self.verification_policy
    }
    pub fn unlock_state(&mut self) -> AccountUnlockStatus {
        self.manager.status(&self.account, Instant::now())
    }

    // Workflow 1: open, edit, atomic save and leased share.

    pub fn open_document(&mut self) -> Result<(), SystemError> {
        self.draft = read_document(&self.document, &self.background)?;
        self.document_status = format!(
            "Document opened · {} bytes · grant retained",
            self.draft.len()
        );
        Ok(())
    }

    pub fn apply_reviewed_edit(&mut self) {
        if !self.draft.contains(REVIEWED_EDIT) {
            self.draft.push('\n');
            self.draft.push_str(REVIEWED_EDIT);
        }
        self.document_status = format!("Document edited · {} bytes", self.draft.len());
    }

    pub fn open_and_edit_document(&mut self) -> Result<(), SystemError> {
        self.open_document()?;
        self.apply_reviewed_edit();
        Ok(())
    }

    pub fn set_draft(&mut self, draft: impl Into<String>) {
        self.draft = draft.into();
        self.document_status = format!("Document draft replaced · {} bytes", self.draft.len());
    }

    pub fn save_atomically(&mut self) -> Result<(), SystemError> {
        let receipt = completed(self.document.save_from_reader(
            &mut Cursor::new(self.draft.as_bytes()),
            gpui_files::WriteOptions::overwrite(),
            &self.background,
        ))?;
        let persisted = read_document(&self.document, &self.background)?;
        if persisted != self.draft {
            return Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "Atomic save read-back did not match the retained draft",
            ));
        }
        self.saved = Some(persisted);
        self.save_status = format!(
            "Atomic save · {} bytes · {:?}",
            receipt.bytes_written, receipt.atomicity
        );
        Ok(())
    }

    pub fn stage_and_share(&mut self) -> Result<(), SystemError> {
        let staging = ExportStaging::stage(
            &self.app,
            &self.document,
            MAX_EXPORT_BYTES,
            &self.background,
        )?;
        let resource = completed(ShareResource::prepare(
            &self.app,
            &self.document,
            MAX_EXPORT_BYTES,
            &self.background,
        ))?;
        let snapshot = read_document(&self.document, &self.background)?;
        self.staging = Some(staging);
        self.staged_snapshot = Some(snapshot);
        self.share_resource = Some(resource.clone());
        let interaction = self.sharing.user_initiated()?;
        let request = ShareRequest::native(ShareItem::Resource(resource));
        match completed(self.sharing.share(request, interaction)) {
            Ok(receipt) => {
                self.share_status = format!(
                    "Native share submitted · {receipt:?} · export lease retained ({} bytes)",
                    self.staged_export_bytes()
                );
                Ok(())
            }
            Err(error) if error.code == ErrorCode::Unsupported => {
                self.share_status = format!(
                    "Native share unsupported · export lease retained ({} bytes) · explicit clipboard fallback available",
                    self.staged_export_bytes()
                );
                Ok(())
            }
            Err(error) => {
                self.share_status = format!("Native share failed · {:?}", error.code);
                Err(error)
            }
        }
    }

    pub fn toggle_share_unsupported(&mut self) {
        self.share_provider
            .set_unsupported(!self.share_provider.is_unsupported());
        self.share_status = if self.share_provider.is_unsupported() {
            "Native share configured as Unsupported".into()
        } else {
            "Native share configured as supported".into()
        };
    }

    /// Explicit caller choice. This never runs automatically after a native
    /// share failure and it keeps the staged export lease alive. The clipboard
    /// receives the document snapshot captured when the export was staged, not
    /// later uncommitted draft edits.
    pub fn copy_export_fallback(&mut self) -> Result<(), SystemError> {
        if !self.staged_export_active() {
            self.fallback_status =
                "Clipboard fallback unavailable · stage an export before copying".into();
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Stage an export before requesting the clipboard fallback",
            ));
        }
        let Some(snapshot) = self.staged_snapshot.clone() else {
            self.fallback_status =
                "Clipboard fallback unavailable · staged snapshot is missing".into();
            return Err(SystemError::new(
                ErrorCode::Unavailable,
                "The staged export snapshot is unavailable",
            ));
        };
        let interaction = self.clipboard.user_initiated()?;
        let receipt = completed(self.clipboard.write_text(&snapshot, interaction))?;
        self.fallback_status = format!(
            "Explicit clipboard fallback · {} bytes · sequence {:?} · lease retained",
            snapshot.len(),
            receipt.sequence
        );
        Ok(())
    }

    // Workflow 2: export, notification post/replace and warm/cold activation.

    pub fn export_and_post_notification(&mut self) -> Result<(), SystemError> {
        self.ensure_notification_permission()?;
        let bytes = self.saved.clone().unwrap_or_else(|| self.draft.clone());
        let lease = completed(ExportResourceLease::stage_export_bytes(
            &self.app,
            bytes.as_bytes(),
            MAX_EXPORT_BYTES,
            &self.background,
        ))?;
        self.notification_export = Some(lease);
        let target = self.activation_target()?;
        let binding = NotificationActivationBinding::new(target);
        {
            let store = self.notification_store.as_ref().ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Notification store is closed")
            })?;
            store.remember(
                &self.notification_id,
                binding.clone(),
                SystemTime::now(),
                &self.background,
            )?;
        }
        let content =
            NotificationContent::new("Reviewed document ready", "Open the reviewed plan")?
                .with_activation(binding.clone());
        let receipt = self
            .notifications
            .post(NotificationSubmission::new(
                self.notification_id.clone(),
                content,
            ))?
            .into_result()?;
        self.notification_binding = Some(binding);
        self.notification_status = format!(
            "Notification posted · {:?} · provider {:?}",
            receipt.submission, receipt.provider_id
        );
        self.activation_status = "Notification binding stored".into();
        Ok(())
    }

    pub fn replace_notification(&mut self) -> Result<(), SystemError> {
        let Some(binding) = self.notification_binding.clone() else {
            self.notification_status =
                "Notification replacement rejected · post the notification first".into();
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Post the notification before replacing it",
            ));
        };
        let content =
            NotificationContent::new("Reviewed document updated", "Open the updated plan")?
                .with_activation(binding);
        let receipt = self
            .notifications
            .replace(NotificationSubmission::new(
                self.notification_id.clone(),
                content,
            ))?
            .into_result()?;
        self.notification_status = format!(
            "Notification replaced · replaced_existing {:?}",
            receipt.replaced_existing
        );
        Ok(())
    }

    pub fn trigger_warm_activation(&mut self) -> Result<(), SystemError> {
        self.trigger_activation("gallery-warm-tap", "warm")
    }

    /// Re-open the durable store and inbox over the same AppData roots, reset
    /// process-local activation memory and the screen to its root, then route a
    /// new notification tap. Duplicate suppression must still come from the
    /// durable store tombstone, not from the discarded in-memory set.
    pub fn simulate_cold_launch_activation(&mut self) -> Result<(), SystemError> {
        self.notification_store = None;
        self.inbox = None;
        self.handled_activations.clear();
        self.notification_store = Some(completed(NotificationActivationStore::open(
            &self.app,
            NOTIFICATION_NAMESPACE,
            NotificationActivationConfig::default(),
            self.routes.clone(),
            &self.background,
        ))?);
        let inbox = completed(ActivationInbox::open(
            &self.app,
            NOTIFICATION_NAMESPACE,
            InboxConfig::default(),
            UrlPolicy::new(["https"])?,
            self.routes.clone(),
            &self.background,
        ))?;
        inbox.set_ready(true);
        self.inbox = Some(inbox);
        self.screen.navigation_mut().reset_to_root();
        self.trigger_activation("gallery-cold-tap", "cold launch")
    }

    pub fn reset_route_to_root(&mut self) {
        self.screen.navigation_mut().reset_to_root();
        self.activation_status = "Screen route reset to root".into();
    }

    fn trigger_activation(&mut self, event_id: &str, label: &str) -> Result<(), SystemError> {
        let now = SystemTime::now();
        let callback = NotificationActivationCallback::new(
            ActivationId::new(event_id)?,
            self.notification_id.clone(),
            None,
            now,
        );
        let route = {
            let store = self.notification_store.as_ref().ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Notification store is closed")
            })?;
            let inbox = self.inbox.as_ref().ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Activation inbox is closed")
            })?;
            completed(store.ingest_activation(callback, inbox, now, &self.background))?
        };
        if route.is_duplicate() {
            self.activation_status = format!("Duplicate {label} activation suppressed");
            return Ok(());
        }
        if route.receipt().is_none() {
            self.activation_status = format!("{label} activation has no live binding");
            return Ok(());
        }
        self.drain_pending_activations(now, label)
    }

    fn drain_pending_activations(
        &mut self,
        now: SystemTime,
        label: &str,
    ) -> Result<(), SystemError> {
        let pending = {
            let inbox = self.inbox.as_ref().ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Activation inbox is closed")
            })?;
            completed(inbox.pending(now, &self.background))?
        };
        for event in pending {
            let id = event.id().as_str().to_owned();
            let document = match event.payload() {
                ActivationPayload::Notification { target, .. } => {
                    target.parameters().get("document").cloned()
                }
                _ => None,
            };
            if let Some(document_id) = document {
                self.screen
                    .navigation_mut()
                    .push(GalleryRoute::Document { document_id });
                self.handled_activations.insert(id);
                self.activation_status = format!(
                    "{label} activation routed · {:?}",
                    self.screen.navigation().current()
                );
            } else {
                self.activation_status = format!("{label} activation target was incomplete");
            }
            let inbox = self.inbox.as_ref().ok_or_else(|| {
                SystemError::new(ErrorCode::Unavailable, "Activation inbox is closed")
            })?;
            completed(inbox.acknowledge(event.id(), now, &self.background))?;
        }
        Ok(())
    }

    fn activation_target(&self) -> Result<ActivationTarget, SystemError> {
        ActivationTarget::new(
            DOCUMENT_ROUTE,
            1,
            std::collections::BTreeMap::from([("document".to_owned(), DOCUMENT_ID.to_owned())]),
        )
    }

    fn ensure_notification_permission(&mut self) -> Result<PermissionStatus, SystemError> {
        let mut query = self
            .permissions
            .query(PermissionKind::Notifications, self.background.clone());
        let snapshot = resolve(&mut query)?;
        if snapshot.status.allows_use() {
            return Ok(snapshot.status);
        }
        let mut request = self.permissions.request_permission(
            PermissionRequest::new(PermissionKind::Notifications),
            self.foreground.clone(),
        );
        let outcome = resolve(&mut request)?;
        if !outcome.status.allows_use() {
            return Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "Notification permission is not granted",
            ));
        }
        Ok(outcome.status)
    }

    // Workflow 3: local verification, relock and protected-secret release.

    pub fn probe_local_auth(&mut self) -> Result<(), SystemError> {
        self.flow.begin_availability_check();
        let mut request = self
            .local_auth
            .query_availability(self.verification_policy, self.background.clone());
        let snapshot = resolve(&mut request)?;
        self.flow.apply_availability(&snapshot);
        self.local_auth_status = format!(
            "Local auth readiness · {:?} · policy {:?}",
            snapshot.readiness, snapshot.policy
        );
        Ok(())
    }

    pub fn set_verification_policy(
        &mut self,
        policy: VerificationPolicy,
    ) -> Result<(), SystemError> {
        self.verification_policy = policy;
        self.flow =
            LocalUnlockFlowModel::new(self.account.clone(), *self.flow.unlock_policy(), policy)?;
        self.local_auth_status = format!("Local auth policy · {policy:?}");
        Ok(())
    }

    /// Exercise a user-cancelled verification prompt. The previous scripted
    /// provider mode is always restored, and the account session must stay
    /// locked.
    pub fn simulate_cancelled_verification(&mut self) -> Result<(), SystemError> {
        let previous = self.verification_mode;
        self.local_auth_provider
            .set_auto_verify(Some(FakeVerificationMode::UserCancelled));
        let result = self.verify_and_unlock_session();
        self.local_auth_provider.set_auto_verify(Some(previous));
        result?;
        if matches!(
            self.manager.status(&self.account, Instant::now()),
            AccountUnlockStatus::Locked { .. }
        ) {
            Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "Local verification was cancelled by the user",
            ))
        } else {
            Err(SystemError::new(
                ErrorCode::IntegrityFailure,
                "A cancelled verification unexpectedly unlocked the session",
            ))
        }
    }

    pub fn verify_and_unlock_session(&mut self) -> Result<(), SystemError> {
        self.reconcile_foreground();
        self.flow.begin_availability_check();
        let mut request = self
            .local_auth
            .query_availability(self.verification_policy, self.background.clone());
        let snapshot = resolve(&mut request)?;
        self.flow.apply_availability(&snapshot);
        let purpose = VerificationPurpose::new("unlock gallery session")?;
        if !self.flow.begin_prompt() {
            return Err(SystemError::new(
                ErrorCode::Unavailable,
                "Local unlock flow is not ready to prompt",
            ));
        }
        let mut request = self.local_auth.verify_user(
            self.flow.verification_request(purpose),
            self.foreground.clone(),
        );
        let outcome = resolve(&mut request)?;
        self.flow.apply_outcome(&outcome);
        match outcome {
            VerificationOutcome::Verified(receipt) => {
                let grant = self
                    .manager
                    .grant_unlock(&self.account, &receipt, Instant::now())?;
                self.unlock_status = format!(
                    "Session unlocked · {:?} · remaining {:?}",
                    grant.factor,
                    grant.expires_at.saturating_duration_since(Instant::now())
                );
            }
            other => {
                self.unlock_status = format!("Session locked · {other:?}");
            }
        }
        Ok(())
    }

    /// Release the sealed secret through the capability-gated local-auth
    /// operation. The account session must already be unlocked; the local-auth
    /// service independently enforces foreground and session-lock capability
    /// gating. Only the key identity and byte length are retained.
    pub fn release_protected_secret(&mut self) -> Result<(), SystemError> {
        if !matches!(
            self.manager.status(&self.account, Instant::now()),
            AccountUnlockStatus::Unlocked { .. }
        ) {
            self.secret_status = format!(
                "Protected secret receipt · key {} · session locked · redacted",
                self.protected_key.as_str()
            );
            return Err(SystemError::new(
                ErrorCode::PermissionDenied,
                "Unlock the account session before releasing protected secrets",
            ));
        }
        let purpose = VerificationPurpose::new("release gallery secret")?;
        let mut request = self.local_auth.unlock_protected_secret(
            ProtectedSecretRequest::new(
                self.account.clone(),
                self.protected_key.clone(),
                self.verification_policy,
                purpose,
            ),
            self.foreground.clone(),
        );
        let outcome = resolve(&mut request)?;
        match outcome {
            ProtectedSecretOutcome::Unlocked(secret) => {
                let length = secret.expose().len();
                drop(secret);
                self.secret_status = format!(
                    "Protected secret receipt · key {} · {length} bytes · released",
                    self.protected_key.as_str()
                );
            }
            other => {
                self.secret_status = format!(
                    "Protected secret receipt · key {} · unavailable ({other:?})",
                    self.protected_key.as_str()
                );
            }
        }
        Ok(())
    }

    pub fn simulate_background_relock(&mut self) -> Result<(), SystemError> {
        let host = LocalAuthHostState::foreground(SurfaceId(7), 1).with_phase(AppPhase::Background);
        let now = Instant::now();
        self.local_auth.reconcile_host_state(&host, now);
        self.manager.reconcile_host_state(&host, now);
        self.flow.lock();
        self.relock_status = format!(
            "Background relock · {:?}",
            self.manager.status(&self.account, now)
        );
        Ok(())
    }

    pub fn simulate_session_lock(&mut self) -> Result<(), SystemError> {
        let host = LocalAuthHostState::foreground(SurfaceId(7), 1)
            .with_session_lock(SessionLockState::Locked);
        let now = Instant::now();
        self.local_auth.reconcile_host_state(&host, now);
        self.manager.reconcile_host_state(&host, now);
        self.flow.lock();
        self.relock_status = format!(
            "Session-lock relock · {:?}",
            self.manager.status(&self.account, now)
        );
        Ok(())
    }

    fn reconcile_foreground(&self) {
        self.local_auth.reconcile_host_state(
            &LocalAuthHostState::foreground(SurfaceId(7), 1),
            Instant::now(),
        );
    }
}

fn read_document(
    document: &FileResource,
    context: &OperationContext,
) -> Result<String, SystemError> {
    let mut bytes = Vec::new();
    completed(document.copy_to_writer(&mut bytes, context, 64 * 1024))?;
    String::from_utf8(bytes).map_err(|_| {
        SystemError::new(
            ErrorCode::IntegrityFailure,
            "Document content was not valid UTF-8",
        )
    })
}

fn completed<T>(result: ServiceResult<T>) -> Result<T, SystemError> {
    match result? {
        Completion::Completed(value) => Ok(value),
        Completion::Cancelled(_) => Err(SystemError::new(
            ErrorCode::Unavailable,
            "Workbench operation was cancelled",
        )),
    }
}

fn resolve<T>(request: &mut Request<T>) -> Result<T, SystemError> {
    match Pin::new(request).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(Ok(Completion::Completed(value))) => Ok(value),
        Poll::Ready(Ok(Completion::Cancelled(_))) => Err(SystemError::new(
            ErrorCode::Unavailable,
            "Workbench request was cancelled",
        )),
        Poll::Ready(Err(error)) => Err(error),
        Poll::Pending => Err(SystemError::new(
            ErrorCode::Busy,
            "Workbench request did not resolve synchronously",
        )),
    }
}
