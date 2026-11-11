use gpui_system::*;
use std::{
    cell::Cell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn poll<T: Future + Unpin>(future: &mut T, wakes: &Arc<WakeCount>) -> Poll<T::Output> {
    let waker = Waker::from(wakes.clone());
    Pin::new(future).poll(&mut Context::from_waker(&waker))
}

fn capability() -> Capability<()> {
    Capability {
        operation: (),
        support: Support::Native,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::Granted,
        evidence: Evidence::ContractOnly,
        constraints: Vec::new(),
        observed_revision: 1,
    }
}

// This fake probes conditions again when executing, so authorization snapshots
// are demonstrably not durable grants and probing has no side effects.
struct FakeProvider(Cell<PermissionState>);
impl FakeProvider {
    fn capabilities(&self) -> Capability<()> {
        Capability {
            permission: self.0.get(),
            ..capability()
        }
    }
    fn execute(&self, cx: OperationContext) -> Request<&'static str> {
        let (request, callback) = request(cx);
        callback.complete(
            self.capabilities()
                .check()
                .map(|_| Completion::Completed("done")),
        );
        request
    }
}

#[test]
fn execution_rechecks_conditions_and_registry_is_typed() {
    let mut services = SystemServices::new();
    assert_eq!(
        services.service::<FakeProvider>().err().unwrap().code,
        ErrorCode::Unavailable
    );
    services
        .register(FakeProvider(Cell::new(PermissionState::Granted)))
        .unwrap();
    assert!(
        services
            .register(FakeProvider(Cell::new(PermissionState::Granted)))
            .is_err()
    );
    let provider = services.service::<FakeProvider>().unwrap();
    assert!(provider.capabilities().check().is_ok());
    provider.0.set(PermissionState::Denied);
    let mut request = provider.execute(OperationContext::default());
    let result = poll(&mut request, &Arc::default());
    assert!(matches!(
        result,
        Poll::Ready(Err(SystemError {
            code: ErrorCode::PermissionDenied,
            ..
        }))
    ));
    services.override_provider(FakeProvider(Cell::new(PermissionState::Granted)));
    assert_eq!(
        services.service::<FakeProvider>().unwrap().0.get(),
        PermissionState::Granted
    );
    for (capability, code) in [
        (
            Capability {
                support: Support::FallbackAvailable,
                ..capability()
            },
            ErrorCode::Unsupported,
        ),
        (
            Capability {
                support: Support::Unsupported,
                ..capability()
            },
            ErrorCode::Unsupported,
        ),
        (
            Capability {
                availability: Availability::Unavailable,
                ..capability()
            },
            ErrorCode::Unavailable,
        ),
        (
            Capability {
                configuration: Configuration::Required,
                ..capability()
            },
            ErrorCode::InvalidConfiguration,
        ),
    ] {
        assert_eq!(capability.check().unwrap_err().code, code);
    }
}

#[test]
fn request_callbacks_are_once_and_cancellation_wakes() {
    let cx = OperationContext::default();
    let cancellation = cx.cancellation.clone();
    let (mut request, callback) = request::<u32>(cx);
    let wakes = Arc::default();
    assert!(poll(&mut request, &wakes).is_pending());
    cancellation.cancel(CancelReason::Caller);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(callback.complete(Ok(Completion::Completed(42))));
    assert!(!callback.complete(Ok(Completion::Completed(99))));
    assert_eq!(
        poll(&mut request, &wakes),
        Poll::Ready(Ok(Completion::Cancelled(CancelReason::Caller)))
    );
    assert!(poll(&mut request, &wakes).is_pending());
    assert!(!callback.complete(Ok(Completion::Completed(1))));
}

#[test]
fn cancellation_before_registration_and_request_drop_are_safe() {
    let cx = OperationContext::default();
    cx.cancellation.cancel(CancelReason::User);
    let (mut cancelled, _) = request::<u32>(cx);
    assert_eq!(
        poll(&mut cancelled, &Arc::default()),
        Poll::Ready(Ok(Completion::Cancelled(CancelReason::User)))
    );
    let (abandoned, callback) = request::<u32>(OperationContext::default());
    drop(abandoned);
    assert!(!callback.complete(Ok(Completion::Completed(1))));
}

#[test]
fn generation_change_disposal_and_last_owner_drop_wake() {
    let owner = UiOwner::new(SurfaceId(12));
    let (mut old, callback) = request::<u32>(OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    });
    let wakes = Arc::default();
    assert!(poll(&mut old, &wakes).is_pending());
    owner.advance_generation().unwrap();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(callback.complete(Ok(Completion::Completed(1))));
    assert!(matches!(
        poll(&mut old, &wakes),
        Poll::Ready(Err(SystemError {
            code: ErrorCode::ContextExpired,
            ..
        }))
    ));
    let (mut pending, _) = request::<()>(OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    });
    assert!(poll(&mut pending, &wakes).is_pending());
    let cloned_owner = owner.clone();
    drop(owner);
    assert!(poll(&mut pending, &wakes).is_pending());
    drop(cloned_owner);
    assert!(matches!(
        poll(&mut pending, &wakes),
        Poll::Ready(Err(SystemError {
            code: ErrorCode::ContextExpired,
            ..
        }))
    ));
}

#[test]
fn timer_hook_expires_without_callback_and_rejects_late_result() {
    let now = Instant::now();
    let deadline = now + Duration::from_secs(5);
    let (mut request, callback) = request::<()>(OperationContext {
        deadline: Some(deadline),
        ..Default::default()
    });
    let wakes = Arc::default();
    assert!(poll(&mut request, &wakes).is_pending());
    callback.expire_at(deadline);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(!callback.complete(Ok(Completion::Completed(()))));
    assert!(matches!(
        poll(&mut request, &wakes),
        Poll::Ready(Err(SystemError {
            code: ErrorCode::TimedOut,
            ..
        }))
    ));
}

#[test]
fn callback_before_recreation_does_not_deliver_stale_success() {
    let owner = UiOwner::new(SurfaceId(1));
    let (mut request, callback) = request::<()>(OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    });
    assert!(callback.complete(Ok(Completion::Completed(()))));
    owner.dispose();
    assert!(matches!(
        poll(&mut request, &Arc::default()),
        Poll::Ready(Err(SystemError {
            code: ErrorCode::ContextExpired,
            ..
        }))
    ));
}

#[test]
fn native_callbacks_race_to_single_completion() {
    let (mut request, callback) = request::<u32>(OperationContext::default());
    let other = callback.clone();
    let first = std::thread::spawn(move || callback.complete(Ok(Completion::Completed(1))));
    let second = std::thread::spawn(move || other.complete(Ok(Completion::Completed(2))));
    assert_ne!(first.join().unwrap(), second.join().unwrap());
    assert!(matches!(
        poll(&mut request, &Arc::default()),
        Poll::Ready(Ok(Completion::Completed(1 | 2)))
    ));
}

#[test]
fn bounded_events_report_loss_and_snapshot_revision() {
    let (sender, stream) = event_channel(1, || {});
    assert!(sender.send(1));
    sender.send(2);
    assert!(stream.snapshot_required());
    let mut next = stream.try_next().unwrap();
    assert!(matches!(
        poll(&mut next, &Arc::default()),
        Poll::Ready(Some(EventEnvelope {
            payload: Event::ResyncRequired,
            ..
        }))
    ));
    let revision = sender.revision();
    sender.send(3);
    sender.send(4);
    stream.acknowledge_snapshot(revision);
    assert!(stream.snapshot_required());
    stream.acknowledge_snapshot(sender.revision());
    assert!(!stream.snapshot_required());
}

#[test]
fn stream_drop_closes_waiter_and_cleanup_can_reenter_sender() {
    let cleaned = Rc::new(Cell::new(false));
    let flag = cleaned.clone();
    let retained_sender = Rc::new(std::cell::RefCell::new(None::<EventSender<u32>>));
    let cleanup_sender = retained_sender.clone();
    let (sender, stream) = event_channel(1, move || {
        flag.set(true);
        assert!(!cleanup_sender.borrow().as_ref().unwrap().send(7));
    });
    *retained_sender.borrow_mut() = Some(sender.clone());
    let mut next = stream.try_next().unwrap();
    assert_eq!(stream.try_next().err().unwrap().code, ErrorCode::Busy);
    let wakes = Arc::default();
    assert!(poll(&mut next, &wakes).is_pending());
    drop(stream);
    assert!(cleaned.get());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&mut next, &wakes), Poll::Ready(None));
    assert!(!sender.send(2));
}

#[test]
fn producer_drop_and_pending_reader_drop_release_slots() {
    let (sender, stream) = event_channel::<()>(1, || {});
    let pending = stream.try_next().unwrap();
    drop(pending);
    let mut next = stream.try_next().unwrap();
    let wakes = Arc::default();
    assert!(poll(&mut next, &wakes).is_pending());
    drop(sender);
    assert_eq!(poll(&mut next, &wakes), Poll::Ready(None));
}

#[test]
fn diagnostics_have_only_redacted_structure() {
    let (_, callback) = request::<()>(OperationContext::default());
    let diagnostic = callback.diagnostic(DiagnosticPhase::Started);
    assert_eq!(diagnostic.request, callback.id());
    assert_eq!(diagnostic.code, None);
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|provider| !provider.native_provider_installed)
    );
}
