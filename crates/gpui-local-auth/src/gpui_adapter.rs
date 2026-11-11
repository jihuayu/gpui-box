//! Optional GPUI integration for [`LocalAuthService`].
//!
//! Every function must run on the owning GPUI UI thread. The adapter installs no
//! native provider and never runs a prompt on a worker thread.

use crate::{
    EventSubscription, LocalAuthEvent, LocalAuthProvider, LocalAuthService, VerificationOutcome,
    VerificationRequest,
};
use gpui::{App, Context, Task};
use gpui_system::{
    ErrorCode, OperationContext, Request, ServiceResult, SystemError, SystemServices, UiOwner,
    gpui_adapter,
};
use std::rc::Rc;

/// Register one explicit local-auth provider. Call before starting operations.
pub fn install_local_auth_service(
    services: &mut SystemServices,
    provider: Rc<dyn LocalAuthProvider>,
) -> Result<(), SystemError> {
    services.register(LocalAuthService::new(provider))
}

/// Access the installed service from GPUI's typed `SystemServices` global.
pub fn local_auth_service(cx: &App) -> Result<Rc<LocalAuthService>, SystemError> {
    let missing = SystemError::new(ErrorCode::Unavailable, "SystemServices is not installed");
    let services = cx.try_global::<SystemServices>().ok_or(missing)?;
    services.service::<LocalAuthService>()
}

/// Run a local-auth request on GPUI's local foreground executor with the runtime
/// deadline timer installed.
pub fn run_local_auth_request<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}

/// Convenience wrapper for interactive user verification.
pub fn verify_user(
    service: &LocalAuthService,
    request: VerificationRequest,
    cx: OperationContext,
    app: &App,
) -> Task<ServiceResult<VerificationOutcome>> {
    run_local_auth_request(service.verify_user(request, cx), app)
}

/// Entity-owned scope that retains local-auth observation streams and
/// invalidates the UI owner when the entity is released.
///
/// Dropping the scope drops the subscription first, then disposes the owner
/// through [`gpui_system::gpui_adapter::EntityScope`].
pub struct LocalAuthEntityScope {
    subscriptions: Vec<EventSubscription<LocalAuthEvent>>,
    scope: gpui_system::gpui_adapter::EntityScope,
}

impl LocalAuthEntityScope {
    pub fn new<T: 'static>(owner: UiOwner, cx: &Context<T>) -> Self {
        Self {
            subscriptions: Vec::new(),
            scope: gpui_system::gpui_adapter::EntityScope::new(owner, cx),
        }
    }

    pub fn owner(&self) -> &UiOwner {
        self.scope.owner()
    }

    pub fn retain(&mut self, subscription: EventSubscription<LocalAuthEvent>) {
        self.subscriptions.push(subscription);
    }

    pub fn subscribe(&mut self, service: &LocalAuthService) {
        self.retain(service.subscribe());
    }

    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }
}

#[cfg(all(
    test,
    feature = "gpui",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
mod tests {
    use super::*;
    use crate::{
        Completion, FakeLocalAuthProvider, FakeVerificationMode, LocalAuthService, SurfaceId,
        VerificationPolicy, VerificationPurpose, VerifiedFactor,
    };
    use gpui::{AppContext, TestAppContext};
    use std::{
        future::Future,
        pin::Pin,
        task::{Context as TaskContext, Poll, Waker},
    };

    struct ScopedEntity {
        _scope: LocalAuthEntityScope,
    }

    fn poll_once<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
        Pin::new(future).poll(&mut TaskContext::from_waker(Waker::noop()))
    }

    fn install(cx: &TestAppContext, fake: &Rc<FakeLocalAuthProvider>) -> Rc<LocalAuthService> {
        cx.update(|cx| {
            let mut services = SystemServices::new();
            install_local_auth_service(&mut services, fake.clone()).expect("service registration");
            gpui_system::gpui_adapter::install_system_services(services, cx);
            local_auth_service(cx).expect("installed local-auth service")
        })
    }

    #[gpui::test]
    fn service_lookup_and_foreground_verification(cx: &TestAppContext) {
        let fake = Rc::new(FakeLocalAuthProvider::new());
        fake.set_auto_verify(Some(FakeVerificationMode::Biometric));
        let service = install(cx, &fake);
        let provider = Rc::clone(service.provider());
        let expected: Rc<dyn LocalAuthProvider> = fake.clone();
        assert!(Rc::ptr_eq(&provider, &expected));

        let owner = UiOwner::new(SurfaceId(1));
        let context = OperationContext {
            ui: Some(owner.context(None).expect("current UI context")),
            ..Default::default()
        };
        let request = VerificationRequest::new(
            VerificationPolicy::BiometricOnly,
            VerificationPurpose::new("unlock vault").expect("purpose"),
        );
        let mut future = service.verify_user(request, context);
        match poll_once(&mut future) {
            Poll::Ready(Ok(Completion::Completed(VerificationOutcome::Verified(receipt)))) => {
                assert_eq!(receipt.factor(), VerifiedFactor::Biometric);
                assert_eq!(receipt.surface(), SurfaceId(1));
            }
            other => panic!("foreground verification did not complete: {other:?}"),
        }
        assert_eq!(
            service.active_prompt(),
            None,
            "a completed prompt releases the lane"
        );
    }

    #[gpui::test]
    fn entity_release_invalidates_owner_and_rejects_late_prompt(cx: &TestAppContext) {
        let fake = Rc::new(FakeLocalAuthProvider::new());
        fake.set_auto_verify(None);
        let service = LocalAuthService::new(fake.clone());
        let owner = UiOwner::new(SurfaceId(2));
        let scope_owner = owner.clone();
        let entity = cx.update(|cx| {
            cx.new(|cx| ScopedEntity {
                _scope: LocalAuthEntityScope::new(scope_owner, cx),
            })
        });
        let context = OperationContext {
            ui: Some(owner.context(None).expect("current UI context")),
            ..Default::default()
        };
        let request = VerificationRequest::new(
            VerificationPolicy::BiometricOnly,
            VerificationPurpose::new("unlock vault").expect("purpose"),
        );
        let mut future = service.verify_user(request, context);
        let pending = fake.take_pending_prompt().expect("pending fake prompt");

        cx.update(|_cx| drop(entity));
        assert!(
            owner.context(None).is_err(),
            "releasing the entity disposes the owner"
        );
        assert!(
            !pending.verify(VerifiedFactor::Biometric),
            "a released entity rejects its late prompt"
        );
        if let Poll::Ready(Ok(Completion::Completed(VerificationOutcome::Verified(_)))) =
            poll_once(&mut future)
        {
            panic!("released entity produced a verification");
        }
        assert_eq!(
            service.rejected_callbacks().len(),
            1,
            "the late callback is recorded once"
        );
    }

    #[gpui::test]
    fn service_lookup_requires_installation(cx: &TestAppContext) {
        cx.update(|cx| {
            assert!(local_auth_service(cx).is_err());
        });
    }

    #[gpui::test]
    fn capability_snapshots_do_not_prompt(cx: &TestAppContext) {
        let fake = Rc::new(FakeLocalAuthProvider::new());
        let service = install(cx, &fake);
        let capabilities = service.capabilities();
        assert_eq!(capabilities.len(), 6);
        assert_eq!(fake.pending_prompt_count(), 0);
        assert_eq!(fake.verify_requests().len(), 0);
    }
}
