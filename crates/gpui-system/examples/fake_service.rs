//! Runnable headless sample: discover, start, cancel and observe completion.
use gpui_system::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

struct FakeService;

fn main() -> Result<(), SystemError> {
    let mut services = SystemServices::new();
    services.register(FakeService)?;
    let _provider = services.service::<FakeService>()?;
    let capability = Capability {
        operation: "sample",
        support: Support::Conditional,
        availability: Availability::Available,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: vec![],
        observed_revision: 1,
    };
    capability.check()?;
    let context = OperationContext::default();
    let cancellation = context.cancellation.clone();
    let (mut request, callback) = request::<()>(context);
    cancellation.cancel(CancelReason::Caller);
    callback.complete(Ok(Completion::Completed(())));
    match Pin::new(&mut request).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Ok(Completion::Cancelled(CancelReason::Caller))) => {
            println!("Cancellation observed without blocking")
        }
        _ => {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                "Unexpected sample completion",
            ));
        }
    }
    Ok(())
}
