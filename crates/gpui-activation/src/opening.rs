use crate::{AllowedUrl, UrlPolicy, error};
use gpui_system::{
    Availability, Capability, Completion, Configuration, ErrorCode, Evidence, OperationContext,
    PermissionState, Request, Support, SystemError, SystemServices,
};
use std::{rc::Rc, time::Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationOperation {
    OpenUrl,
    DurableInbox,
    SingleInstanceForwarding,
    NativeRegistration,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenUrlReceipt {
    /// Native provider reports submission, not browser display or navigation.
    Submitted,
    /// Legacy void GPUI handoff was attempted; acceptance cannot be observed.
    AttemptedOutcomeUnknown,
}
pub trait UrlOpeningProvider {
    fn capability(&self) -> Capability<ActivationOperation>;
    fn open_url(&self, url: &AllowedUrl) -> Result<OpenUrlReceipt, SystemError>;
}
#[derive(Clone)]
pub struct UrlOpening {
    provider: Rc<dyn UrlOpeningProvider>,
    policy: UrlPolicy,
}
impl UrlOpening {
    pub fn new(provider: Rc<dyn UrlOpeningProvider>, policy: UrlPolicy) -> Self {
        Self { provider, policy }
    }
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }
    pub fn capability(&self) -> Capability<ActivationOperation> {
        self.provider.capability()
    }
    pub fn open(&self, input: &str, context: OperationContext) -> Request<OpenUrlReceipt> {
        let (request, callback) = gpui_system::request(context.clone());
        let result = (|| {
            context.require_ui()?;
            context.check_at(Instant::now())?;
            if let Some(reason) = context.cancellation.reason() {
                return Ok(Completion::Cancelled(reason));
            }
            let url = self.policy.parse(input)?;
            self.provider.capability().check()?;
            context.check_at(Instant::now())?;
            if let Some(reason) = context.cancellation.reason() {
                return Ok(Completion::Cancelled(reason));
            }
            self.provider.open_url(&url).map(Completion::Completed)
        })();
        callback.complete(result);
        request
    }
}
/// Providers must recheck native conditions. No implicit IPC or registrations.
pub fn capabilities() -> Vec<Capability<ActivationOperation>> {
    [
        ActivationOperation::DurableInbox,
        ActivationOperation::SingleInstanceForwarding,
        ActivationOperation::NativeRegistration,
    ]
    .into_iter()
    .map(|operation| Capability {
        operation,
        support: if operation == ActivationOperation::DurableInbox {
            Support::Native
        } else {
            Support::Unsupported
        },
        availability: if operation == ActivationOperation::DurableInbox {
            Availability::Available
        } else {
            Availability::Unavailable
        },
        configuration: if operation == ActivationOperation::NativeRegistration {
            Configuration::Required
        } else {
            Configuration::Ready
        },
        permission: PermissionState::NotRequired,
        evidence: Evidence::Compiled,
        constraints: vec![],
        observed_revision: 1,
    })
    .collect()
}
pub struct UnavailableUrlProvider;
impl UrlOpeningProvider for UnavailableUrlProvider {
    fn capability(&self) -> Capability<ActivationOperation> {
        Capability {
            operation: ActivationOperation::OpenUrl,
            support: Support::Unknown,
            availability: Availability::Unavailable,
            configuration: Configuration::Required,
            permission: PermissionState::Unknown,
            evidence: Evidence::ContractOnly,
            constraints: vec![],
            observed_revision: 0,
        }
    }
    fn open_url(&self, _: &AllowedUrl) -> Result<OpenUrlReceipt, SystemError> {
        Err(error(
            ErrorCode::Unavailable,
            "No URL-opening provider is installed",
        ))
    }
}
