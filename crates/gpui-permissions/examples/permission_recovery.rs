//! Runnable headless recovery sample: first request, denial, settings handoff,
//! simulated OS toggle and resume reconciliation. No native dialog is shown.

use gpui_permissions::*;
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn poll_ready<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

fn expect_outcome(request: &mut Request<PermissionOutcome>) -> PermissionOutcome {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Completed(outcome))) => outcome,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => panic!("request cancelled: {reason:?}"),
        Poll::Ready(Err(error)) => panic!("permission error: {error}"),
        Poll::Pending => panic!("permission request did not complete"),
    }
}

fn expect_change(
    subscription: &EventSubscription<PermissionChange>,
) -> Result<PermissionChange, SystemError> {
    let mut next = subscription.try_next()?;
    match poll_ready(&mut next) {
        Poll::Ready(Some(EventEnvelope {
            payload: Event::Value(change),
            ..
        })) => Ok(change),
        Poll::Ready(Some(EventEnvelope {
            payload: Event::ResyncRequired,
            ..
        })) => Err(SystemError::new(
            ErrorCode::ResourceExpired,
            "Permission stream requires a snapshot resync",
        )),
        Poll::Ready(None) => Err(SystemError::new(
            ErrorCode::Unavailable,
            "Permission stream is closed",
        )),
        Poll::Pending => Err(SystemError::new(
            ErrorCode::Unavailable,
            "Permission change is not ready",
        )),
    }
}

fn main() -> Result<(), SystemError> {
    let owner = UiOwner::new(SurfaceId(1));
    let context = OperationContext {
        ui: Some(owner.context(None)?),
        ..Default::default()
    };
    let kind = PermissionKind::Notifications;
    let fake =
        Rc::new(FakePermissionProvider::new().with_status(kind, PermissionStatus::NotDetermined));
    let service = PermissionService::new(fake.clone());
    let changes = service.subscribe();

    let mut first = service.request_permission(PermissionRequest::new(kind), context.clone());
    if poll_ready(&mut first).is_ready() {
        return Err(SystemError::new(
            ErrorCode::PlatformFailure,
            "Prompt completed before the native callback",
        ));
    }
    fake.deny_next(true);
    let first = expect_outcome(&mut first);
    println!(
        "first request: {:?}, prompted={}, recovery={:?}",
        first.status,
        first.prompted,
        first.recovery()
    );
    expect_change(&changes)?;

    let mut second = service.request_permission(PermissionRequest::new(kind), context.clone());
    if poll_ready(&mut second).is_ready() {
        return Err(SystemError::new(
            ErrorCode::PlatformFailure,
            "Repeat prompt completed before the native callback",
        ));
    }
    fake.deny_next(false);
    let second = expect_outcome(&mut second);
    println!(
        "repeat request: {:?}, settings required={}",
        second.status,
        second.requires_settings()
    );
    expect_change(&changes)?;

    let mut handoff = service.open_settings(SettingsTarget::NotificationSettings, context.clone());
    match poll_ready(&mut handoff) {
        Poll::Ready(Ok(Completion::Completed(receipt))) => {
            println!("settings handoff: {receipt:?}");
        }
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                match reason {
                    CancelReason::User => "Settings handoff was cancelled",
                    _ => "Settings handoff was interrupted",
                },
            ));
        }
        Poll::Ready(Err(error)) => return Err(error),
        Poll::Pending => {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                "Settings handoff did not complete",
            ));
        }
    }

    // The user returns from OS settings. Only a fresh provider probe is
    // authoritative; the handoff receipt itself never claimed a toggle.
    fake.set_status(kind, PermissionStatus::Granted);
    let reconciliation = service.reconcile_on_resume()?;
    println!(
        "resume changes: {}, revision={}",
        reconciliation.changes.len(),
        reconciliation.revision
    );
    let granted = expect_change(&changes)?;
    println!(
        "resume delta: {:?} -> {:?}",
        granted.previous, granted.current
    );

    let mut query = service.query(kind, context);
    match poll_ready(&mut query) {
        Poll::Ready(Ok(Completion::Completed(snapshot))) => {
            println!("authoritative status: {:?}", snapshot.status);
            if !snapshot.allows_use() {
                return Err(SystemError::new(
                    ErrorCode::PermissionDenied,
                    "Expected granted notification permission",
                ));
            }
        }
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                match reason {
                    CancelReason::User => "Permission query was cancelled",
                    _ => "Permission query was interrupted",
                },
            ));
        }
        Poll::Ready(Err(error)) => return Err(error),
        Poll::Pending => {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                "Permission query did not complete",
            ));
        }
    }
    Ok(())
}
