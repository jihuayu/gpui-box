//! UI-thread bridge over the existing GPUI void URL-opening operation.
use crate::{
    ActivationOperation, AllowedUrl, OpenUrlReceipt, UrlOpening, UrlOpeningProvider, UrlPolicy,
};
use gpui::{App, Task};
use gpui_system::{
    Availability, Capability, Configuration, Evidence, OperationContext, PermissionState, Request,
    ServiceResult, Support, SystemError, gpui_adapter,
};
use std::rc::Rc;
struct LegacyProvider {
    open: Box<dyn Fn(&str)>,
}
impl UrlOpeningProvider for LegacyProvider {
    fn capability(&self) -> Capability<ActivationOperation> {
        Capability {
            operation: ActivationOperation::OpenUrl,
            support: Support::Conditional,
            availability: Availability::Available,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence: Evidence::Compiled,
            constraints: vec![],
            observed_revision: 1,
        }
    }
    fn open_url(&self, url: &AllowedUrl) -> Result<OpenUrlReceipt, SystemError> {
        (self.open)(url.as_str());
        Ok(OpenUrlReceipt::AttemptedOutcomeUnknown)
    }
}
/// Explicitly bind an owning GPUI app through a local callback (typically an
/// AsyncApp update). No claim about native handler registration is inferred.
pub fn legacy_url_service(policy: UrlPolicy, open: impl Fn(&str) + 'static) -> UrlOpening {
    UrlOpening::new(
        Rc::new(LegacyProvider {
            open: Box::new(open),
        }),
        policy,
    )
}
/// Direct adapter used while an App borrow is already available. The callback
/// executes synchronously on the validated owner thread, without retained borrows.
pub fn open_url(
    input: &str,
    policy: &UrlPolicy,
    context: OperationContext,
    cx: &App,
) -> Task<ServiceResult<OpenUrlReceipt>> {
    let (request, completion) = gpui_system::request(context.clone());
    let result = (|| {
        context.require_ui()?;
        context.check_at(std::time::Instant::now())?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(gpui_system::Completion::Cancelled(reason));
        }
        let url = policy.parse(input)?;
        cx.open_url(url.as_str());
        Ok(gpui_system::Completion::Completed(
            OpenUrlReceipt::AttemptedOutcomeUnknown,
        ))
    })();
    completion.complete(result);
    run(request, cx)
}
pub fn run<T: 'static>(request: Request<T>, cx: &App) -> Task<ServiceResult<T>> {
    gpui_adapter::run_request(request, cx)
}
