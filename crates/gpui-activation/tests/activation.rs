use gpui_activation::*;
use gpui_app_data::{AppData, DataLocation, DataScope, DirectoryRoots};
use gpui_system::{
    Availability, Capability, Configuration, Evidence, PermissionState, Support, SurfaceId, UiOwner,
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn value<T>(r: ServiceResult<T>) -> T {
    match r.unwrap() {
        Completion::Completed(v) => v,
        _ => panic!("cancelled"),
    }
}
fn setup(capacity: usize) -> (tempfile::TempDir, AppData, ActivationInbox) {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path().join("路径 with spaces"))).unwrap();
    let inbox = value(ActivationInbox::open(
        &app,
        "test",
        InboxConfig {
            capacity,
            dedup_capacity: 2,
            retention: Duration::from_secs(60),
        },
        UrlPolicy::new(["https", "myapp"]).unwrap(),
        routes(),
        &OperationContext::default(),
    ));
    (root, app, inbox)
}
fn routes() -> RouteRegistry {
    let mut routes = RouteRegistry::default();
    routes.register("document", 1).unwrap();
    routes
}
fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}
fn event(id: &str) -> ActivationEvent {
    ActivationEvent::new(
        ActivationId::new(id).unwrap(),
        ActivationSource::ColdLaunch,
        ActivationPayload::Route(ActivationTarget::new("document", 1, BTreeMap::new()).unwrap()),
        at(100),
    )
    .unwrap()
}
fn poll<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}
#[test]
fn durable_cold_warm_replay_ack_and_namespace_ownership() {
    let (_root, app, inbox) = setup(2);
    let ctx = OperationContext::default();
    assert!(matches!(
        ActivationInbox::open(
            &app,
            "test",
            InboxConfig::default(),
            UrlPolicy::new(["https"]).unwrap(),
            routes(),
            &ctx
        ),
        Err(SystemError {
            code: ErrorCode::Busy,
            ..
        })
    ));
    value(inbox.enqueue(event("one"), at(100), &ctx));
    assert!(value(inbox.pending(at(101), &ctx)).is_empty());
    inbox.set_ready(true);
    assert_eq!(value(inbox.pending(at(101), &ctx)).len(), 1);
    assert_eq!(
        value(inbox.enqueue(event("one"), at(101), &ctx)).status,
        EnqueueStatus::Duplicate
    );
    drop(inbox);
    let inbox = value(ActivationInbox::open(
        &app,
        "test",
        InboxConfig::default(),
        UrlPolicy::new(["https", "myapp"]).unwrap(),
        routes(),
        &ctx,
    ));
    assert!(!inbox.is_ready());
    inbox.set_ready(true);
    assert_eq!(value(inbox.pending(at(102), &ctx)).len(), 1);
    value(inbox.acknowledge(&ActivationId::new("one").unwrap(), at(102), &ctx));
    drop(inbox);
    let inbox = value(ActivationInbox::open(
        &app,
        "test",
        InboxConfig::default(),
        UrlPolicy::new(["https", "myapp"]).unwrap(),
        routes(),
        &ctx,
    ));
    assert_eq!(
        value(inbox.enqueue(event("one"), at(103), &ctx)).status,
        EnqueueStatus::Duplicate
    );
    inbox.set_ready(true);
    assert!(value(inbox.pending(at(103), &ctx)).is_empty());
}
#[test]
fn bounded_queue_retention_and_backward_clock() {
    let (_root, _app, inbox) = setup(1);
    let ctx = OperationContext::default();
    value(inbox.enqueue(event("one"), at(100), &ctx));
    assert_eq!(
        inbox.enqueue(event("two"), at(100), &ctx).unwrap_err().code,
        ErrorCode::Busy
    );
    inbox.set_ready(true);
    assert_eq!(value(inbox.pending(at(90), &ctx)).len(), 1);
    assert!(value(inbox.pending(at(160), &ctx)).is_empty());
    assert_eq!(value(inbox.expire(at(160), &ctx)), 1);
    value(inbox.enqueue(event("two"), at(160), &ctx));
    assert_eq!(value(inbox.pending(at(161), &ctx)).len(), 1);
}
#[test]
fn malformed_route_url_and_redaction() {
    let policy = UrlPolicy::new(["https", "myapp"]).unwrap();
    for invalid in [
        "javascript:alert(1)",
        "https:example.com",
        "https://user:password@example.com",
        "https://example.com\n",
        "https://",
        "data:text/plain,secret",
    ] {
        assert!(policy.parse(invalid).is_err(), "accepted {invalid}");
    }
    let url = policy
        .parse("HTTPS://EXAMPLE.COM/private?token=secret")
        .unwrap();
    assert_eq!(url.as_str(), "https://example.com/private?token=secret");
    assert!(!format!("{url:?}").contains("secret"));
    assert!(ActivationTarget::new("document", 0, BTreeMap::new()).is_err());
    let target = ActivationTarget::new("document", 2, BTreeMap::new()).unwrap();
    assert!(routes().validate(&target).is_err());
    assert!(UrlPolicy::new(["file"]).is_err());
    assert!(ActivationId::new("../bad").is_err());
}
#[test]
fn invalid_route_rejected_and_cancellation_has_no_write() {
    let (_root, _app, inbox) = setup(2);
    let ctx = OperationContext::default();
    let target = ActivationTarget::new("document", 2, BTreeMap::new()).unwrap();
    let bad = ActivationEvent::new(
        ActivationId::new("bad").unwrap(),
        ActivationSource::WarmLaunch,
        ActivationPayload::Route(target),
        at(100),
    )
    .unwrap();
    assert!(inbox.enqueue(bad, at(100), &ctx).is_err());
    ctx.cancellation.cancel(gpui_system::CancelReason::Caller);
    assert!(matches!(
        inbox.enqueue(event("cancelled"), at(100), &ctx),
        Ok(Completion::Cancelled(_))
    ));
    inbox.set_ready(true);
    assert!(value(inbox.pending(at(100), &OperationContext::default())).is_empty());
}
#[test]
fn corrupt_original_preserved_until_explicit_quarantine() {
    let (_root, app, inbox) = setup(2);
    drop(inbox);
    let path = app
        .location(DataLocation::Config)
        .path()
        .join("application/settings-activation-test.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"{broken").unwrap();
    let ctx = OperationContext::default();
    assert!(matches!(
        ActivationInbox::open(
            &app,
            "test",
            InboxConfig::default(),
            UrlPolicy::new(["https"]).unwrap(),
            routes(),
            &ctx
        ),
        Err(SystemError {
            code: ErrorCode::IntegrityFailure,
            ..
        })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    value(
        app.settings(
            DataScope::Application,
            "activation-test",
            1,
            2 * 1024 * 1024,
        )
        .unwrap()
        .quarantine(&ctx),
    );
    value(ActivationInbox::open(
        &app,
        "test",
        InboxConfig::default(),
        UrlPolicy::new(["https"]).unwrap(),
        routes(),
        &ctx,
    ));
}
#[test]
fn failed_persistence_does_not_mutate_memory_or_accept_event() {
    let (_root, app, inbox) = setup(2);
    let ctx = OperationContext::default();
    let parent = app
        .location(DataLocation::Config)
        .path()
        .join("application");
    fs::create_dir_all(&parent).unwrap();
    let path = parent.join("settings-activation-test.json");
    fs::create_dir(&path).unwrap();
    assert!(inbox.enqueue(event("one"), at(100), &ctx).is_err());
    inbox.set_ready(true);
    assert!(value(inbox.pending(at(100), &ctx)).is_empty());
    fs::remove_dir(&path).unwrap();
    assert_eq!(
        value(inbox.enqueue(event("one"), at(100), &ctx)).status,
        EnqueueStatus::Queued
    );
}
struct Fake {
    calls: Cell<u32>,
    permission: Cell<PermissionState>,
    available: Cell<Availability>,
    configuration: Cell<Configuration>,
}
impl Fake {
    fn new() -> Self {
        Self {
            calls: Cell::new(0),
            permission: Cell::new(PermissionState::Granted),
            available: Cell::new(Availability::Available),
            configuration: Cell::new(Configuration::Ready),
        }
    }
}
impl UrlOpeningProvider for Fake {
    fn capability(&self) -> Capability<ActivationOperation> {
        Capability {
            operation: ActivationOperation::OpenUrl,
            support: Support::Native,
            availability: self.available.get(),
            configuration: self.configuration.get(),
            permission: self.permission.get(),
            evidence: Evidence::ContractOnly,
            constraints: vec![],
            observed_revision: 1,
        }
    }
    fn open_url(&self, _: &AllowedUrl) -> Result<OpenUrlReceipt, SystemError> {
        self.calls.set(self.calls.get() + 1);
        Ok(OpenUrlReceipt::Submitted)
    }
}
#[test]
fn provider_errors_cancellation_thread_and_stale_delivery() {
    let fake = Rc::new(Fake::new());
    let service = UrlOpening::new(fake.clone(), UrlPolicy::new(["https"]).unwrap());
    let owner = UiOwner::new(SurfaceId(1));
    let context = || OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    for (permission, available, configuration, code) in [
        (
            PermissionState::Denied,
            Availability::Available,
            Configuration::Ready,
            ErrorCode::PermissionDenied,
        ),
        (
            PermissionState::Granted,
            Availability::Unavailable,
            Configuration::Ready,
            ErrorCode::Unavailable,
        ),
        (
            PermissionState::Granted,
            Availability::Available,
            Configuration::Required,
            ErrorCode::InvalidConfiguration,
        ),
    ] {
        fake.permission.set(permission);
        fake.available.set(available);
        fake.configuration.set(configuration);
        assert!(
            matches!(poll(&mut service.open("https://example.com",context())),Poll::Ready(Err(SystemError{code:actual,..})) if actual==code)
        );
    }
    assert_eq!(fake.calls.get(), 0);
    fake.configuration.set(Configuration::Ready);
    let cancelled = context();
    cancelled
        .cancellation
        .cancel(gpui_system::CancelReason::Caller);
    assert!(matches!(
        poll(&mut service.open("https://example.com", cancelled)),
        Poll::Ready(Ok(Completion::Cancelled(_)))
    ));
    let worker_context = context();
    assert!(
        !std::thread::spawn(move || worker_context.require_ui().is_ok())
            .join()
            .unwrap()
    );
    let mut pending = service.open("https://example.com", context());
    assert_eq!(fake.calls.get(), 1);
    owner.advance_generation().unwrap();
    assert!(matches!(
        poll(&mut pending),
        Poll::Ready(Err(SystemError {
            code: ErrorCode::ContextExpired,
            ..
        }))
    ));
}
#[test]
fn capability_reentry_cancellation_prevents_native_handoff() {
    struct Reentrant {
        cancel: gpui_system::CancellationToken,
        calls: Cell<u32>,
    }
    impl UrlOpeningProvider for Reentrant {
        fn capability(&self) -> Capability<ActivationOperation> {
            self.cancel.cancel(gpui_system::CancelReason::Caller);
            Fake::new().capability()
        }
        fn open_url(&self, _: &AllowedUrl) -> Result<OpenUrlReceipt, SystemError> {
            self.calls.set(self.calls.get() + 1);
            Ok(OpenUrlReceipt::Submitted)
        }
    }
    let owner = UiOwner::new(SurfaceId(5));
    let context = OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    };
    let provider = Rc::new(Reentrant {
        cancel: context.cancellation.clone(),
        calls: Cell::new(0),
    });
    let service = UrlOpening::new(provider.clone(), UrlPolicy::new(["https"]).unwrap());
    assert!(matches!(
        poll(&mut service.open("https://example.com", context)),
        Poll::Ready(Ok(Completion::Cancelled(_)))
    ));
    assert_eq!(provider.calls.get(), 0);
}
