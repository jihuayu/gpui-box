use gpui_sharing::*;
use gpui_system::{
    Availability, CancelReason, Capability, Configuration, Evidence, LogicalRect, PermissionState,
    Support, SurfaceId, UiOwner,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

struct Provider {
    calls: Cell<usize>,
    support: Cell<Support>,
    permission: Cell<PermissionState>,
    hold: RefCell<Option<ShareResource>>,
    cancel_after: RefCell<Option<gpui_system::CancellationToken>>,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            calls: Cell::new(0),
            support: Cell::new(Support::Native),
            permission: Cell::new(PermissionState::NotRequired),
            hold: RefCell::new(None),
            cancel_after: RefCell::new(None),
        }
    }
}
fn capability(operation: ShareOperation) -> Capability<ShareOperation> {
    Capability {
        operation,
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::Compiled,
        constraints: vec![],
        observed_revision: 1,
    }
}
impl SharingProvider for Provider {
    fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        let mut c = capability(operation);
        c.support = self.support.get();
        c.permission = self.permission.get();
        c
    }
    fn share(&self, request: &ShareRequest, _: &OperationContext) -> ServiceResult<ShareReceipt> {
        self.calls.set(self.calls.get() + 1);
        if let ShareItem::Resource(resource) = &request.items[0] {
            *self.hold.borrow_mut() = Some(resource.clone());
        }
        if let Some(token) = self.cancel_after.borrow().as_ref() {
            token.cancel(CancelReason::Caller);
        }
        Ok(Completion::Completed(
            ShareReceipt::SubmittedOutcomeUnobservable,
        ))
    }
}
#[derive(Default)]
struct Copy {
    text: RefCell<Option<String>>,
}
impl CopyTextProvider for Copy {
    fn capability(&self) -> Capability<ShareOperation> {
        capability(ShareOperation::CopyText)
    }
    fn copy_text(&self, text: &str, _: &OperationContext) -> ServiceResult<()> {
        *self.text.borrow_mut() = Some(text.into());
        Ok(Completion::Completed(()))
    }
}
fn owner() -> UiOwner {
    UiOwner::new(SurfaceId(12))
}
fn text() -> ShareRequest {
    ShareRequest::native(ShareItem::Text("中文 😀\nunchanged".into()))
}
#[test]
fn caller_selects_copy_and_native_failure_never_substitutes() {
    let owner = owner();
    let provider = Rc::new(Provider::default());
    let copy = Rc::new(Copy::default());
    let sharing = Sharing::new(provider.clone(), owner.context(None).unwrap())
        .with_copy_provider(copy.clone());
    provider.support.set(Support::Unsupported);
    assert_eq!(
        sharing
            .share(text(), sharing.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert!(copy.text.borrow().is_none());
    provider.support.set(Support::Native);
    provider.permission.set(PermissionState::Denied);
    assert_eq!(
        sharing
            .share(text(), sharing.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(copy.text.borrow().is_none());
    let mut request = text();
    request.route = ShareRoute::CopyText;
    assert_eq!(
        sharing
            .share(request, sharing.user_initiated().unwrap())
            .unwrap(),
        Completion::Completed(ShareReceipt::FallbackPerformed {
            kind: ShareFallbackKind::CopyText
        })
    );
    assert_eq!(copy.text.borrow().as_deref(), Some("中文 😀\nunchanged"));
    assert_eq!(provider.calls.get(), 0);
}
#[test]
fn invalid_context_deadline_cancel_and_cross_service_prevent_side_effects() {
    let owner = owner();
    let provider = Rc::new(Provider::default());
    let sharing = Sharing::new(provider.clone(), owner.context(None).unwrap());
    let other = Sharing::new(provider.clone(), owner.context(None).unwrap());
    assert_eq!(
        sharing
            .share(text(), other.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::ContextExpired
    );
    let token = gpui_system::CancellationToken::default();
    token.cancel(CancelReason::User);
    assert_eq!(
        sharing
            .share(
                text(),
                sharing.user_initiated().unwrap().with_cancellation(token)
            )
            .unwrap(),
        Completion::Cancelled(CancelReason::User)
    );
    assert_eq!(
        sharing
            .share(
                text(),
                sharing
                    .user_initiated()
                    .unwrap()
                    .with_deadline(Instant::now() - Duration::from_secs(1))
            )
            .unwrap_err()
            .code,
        ErrorCode::TimedOut
    );
    let interaction = sharing.user_initiated().unwrap();
    owner.advance_generation().unwrap();
    assert_eq!(
        sharing.share(text(), interaction).unwrap_err().code,
        ErrorCode::ContextExpired
    );
    assert_eq!(provider.calls.get(), 0);
}
#[test]
fn accepted_handoff_survives_later_cancellation() {
    let owner = owner();
    let provider = Rc::new(Provider::default());
    let token = gpui_system::CancellationToken::default();
    *provider.cancel_after.borrow_mut() = Some(token.clone());
    let sharing = Sharing::new(provider, owner.context(None).unwrap());
    assert_eq!(
        sharing
            .share(
                text(),
                sharing
                    .user_initiated()
                    .unwrap()
                    .with_cancellation(token.clone())
            )
            .unwrap(),
        Completion::Completed(ShareReceipt::SubmittedOutcomeUnobservable)
    );
    assert_eq!(token.reason(), Some(CancelReason::Caller));
}
#[test]
fn payload_and_anchor_validation_precedes_native_call() {
    let owner = owner();
    let provider = Rc::new(Provider::default());
    let sharing = Sharing::new(provider.clone(), owner.context(None).unwrap());
    for request in [
        ShareRequest::native(ShareItem::Text(String::new())),
        ShareRequest::native(ShareItem::Text("nul\0".into())),
        ShareRequest::native(ShareItem::Text("x".repeat(MAX_SHARE_TEXT_BYTES + 1))),
    ] {
        assert_eq!(
            sharing
                .share(request, sharing.user_initiated().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
    let anchored = Sharing::new(
        provider.clone(),
        owner
            .context(Some(LogicalRect {
                x: f32::NAN,
                y: 0.,
                width: 1.,
                height: 1.,
            }))
            .unwrap(),
    );
    assert_eq!(
        anchored.user_initiated().err().unwrap().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(provider.calls.get(), 0);
}
#[test]
fn multiple_items_require_independent_capability_and_copy_never_flattens() {
    struct Single;
    impl SharingProvider for Single {
        fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
            if operation == ShareOperation::MultipleItems {
                unsupported(operation)
            } else {
                capability(operation)
            }
        }
        fn share(&self, _: &ShareRequest, _: &OperationContext) -> ServiceResult<ShareReceipt> {
            panic!("unsupported multi share invoked")
        }
    }
    let owner = owner();
    let copy = Rc::new(Copy::default());
    let sharing = Sharing::new(Rc::new(Single), owner.context(None).unwrap())
        .with_copy_provider(copy.clone());
    let mut request = text();
    request.items.push(ShareItem::Text("second".into()));
    assert_eq!(
        sharing
            .share(request.clone(), sharing.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    request.route = ShareRoute::CopyText;
    assert_eq!(
        sharing
            .share(request, sharing.user_initiated().unwrap())
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert!(copy.text.borrow().is_none());
}
#[test]
fn authorized_export_outlives_receipt_request_and_disposed_ui() {
    use gpui_app_data::{AppData, DirectoryRoots};
    use gpui_files::{DurableGrantSupport, FakeFileProvider};
    let temp = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(temp.path())).unwrap();
    let files = FakeFileProvider::new();
    let resource = files.create_read_only_stream(
        "report.txt",
        "报告 😀".as_bytes(),
        DurableGrantSupport::Unsupported,
    );
    let prepared = match ShareResource::prepare(&app, &resource, 1024, &OperationContext::default())
        .unwrap()
    {
        Completion::Completed(v) => v,
        _ => panic!("cancelled"),
    };
    let path = prepared.export_lease().path().unwrap().to_path_buf();
    let owner = owner();
    let provider = Rc::new(Provider::default());
    let sharing = Sharing::new(provider.clone(), owner.context(None).unwrap());
    let request = ShareRequest::native(ShareItem::Resource(prepared));
    sharing
        .share(request, sharing.user_initiated().unwrap())
        .unwrap();
    owner.dispose();
    drop(sharing);
    drop(resource);
    assert_eq!(
        app.cleanup_exports(Duration::ZERO, &OperationContext::default())
            .unwrap(),
        Completion::Completed(0)
    );
    assert_eq!(std::fs::read(&path).unwrap(), "报告 😀".as_bytes());
    provider.hold.borrow_mut().take();
    assert_eq!(
        app.cleanup_exports(Duration::ZERO, &OperationContext::default())
            .unwrap(),
        Completion::Completed(1)
    );
    assert!(!path.exists());
}
#[test]
fn share_debug_redacts_payload_and_title() {
    let mut request = text();
    request.title = Some("SecretTitle".into());
    let debug = format!("{request:?}");
    assert!(!debug.contains("SecretTitle"));
    assert!(!debug.contains("unchanged"));
}
