use crate::*;
use gpui_system::{
    Availability, Capability, Configuration, Constraint, Evidence, PermissionState, Platform,
    Support, SystemServices, UiContext,
};
use std::{rc::Rc, time::Instant};

/// UI-local provider; capability probes must never present UI. Providers must
/// validate their bound native owner again immediately before native execution.
pub trait SharingProvider {
    fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation>;
    fn share(
        &self,
        request: &ShareRequest,
        context: &OperationContext,
    ) -> ServiceResult<ShareReceipt>;
}
/// Optional explicitly selected text-copy route. Publish exactly the supplied
/// text; return only when the native clipboard accepted it. A GPUI void call
/// without an error/acceptance signal cannot implement this guarantee.
pub trait CopyTextProvider {
    fn capability(&self) -> Capability<ShareOperation>;
    fn copy_text(&self, text: &str, context: &OperationContext) -> ServiceResult<()>;
}

#[derive(Clone)]
pub struct Sharing {
    provider: Rc<dyn SharingProvider>,
    copy: Option<Rc<dyn CopyTextProvider>>,
    binding: Rc<UiContext>,
}
impl Sharing {
    pub fn new(provider: Rc<dyn SharingProvider>, ui: UiContext) -> Self {
        Self {
            provider,
            copy: None,
            binding: Rc::new(ui),
        }
    }
    /// Call directly from a foreground user action. The host attests the user
    /// gesture; native providers independently verify foreground visibility.
    pub fn user_initiated(&self) -> Result<ShareInteraction, SystemError> {
        let context = OperationContext {
            ui: Some((*self.binding).clone()),
            ..Default::default()
        };
        validate_context(&context)?;
        Ok(ShareInteraction {
            context,
            binding: self.binding.clone(),
        })
    }
    pub fn with_copy_provider(mut self, provider: Rc<dyn CopyTextProvider>) -> Self {
        self.copy = Some(provider);
        self
    }
    pub fn install(self, services: &mut SystemServices) -> Result<(), SystemError> {
        services.register(self)
    }
    pub fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        if operation == ShareOperation::CopyText {
            self.copy
                .as_ref()
                .map_or_else(|| unsupported(operation), |provider| provider.capability())
        } else {
            self.provider.capability(operation)
        }
    }
    pub fn capabilities(&self) -> Vec<Capability<ShareOperation>> {
        ShareOperation::ALL
            .into_iter()
            .map(|operation| self.capability(operation))
            .collect()
    }
    /// Call from the actual foreground user action with its current logical UI
    /// context. Success survives cancellation after a synchronous OS handoff.
    /// A native sheet's later lifecycle is owned by the provider, not this receipt.
    pub fn share(
        &self,
        request: ShareRequest,
        interaction: ShareInteraction,
    ) -> ServiceResult<ShareReceipt> {
        if !Rc::ptr_eq(&self.binding, &interaction.binding) {
            return Err(error(
                ErrorCode::ContextExpired,
                "Share interaction belongs to another service",
            ));
        }
        let context = interaction.context;
        validate_context(&context)?;
        if let Some(reason) = context.cancellation.reason() {
            return Ok(Completion::Cancelled(reason));
        }
        request.validate()?;
        match request.route {
            ShareRoute::Native => {
                for operation in request.operations() {
                    self.provider.capability(operation).check()?;
                }
                validate_context(&context)?;
                if let Some(reason) = context.cancellation.reason() {
                    return Ok(Completion::Cancelled(reason));
                }
                self.provider.share(&request, &context)
            }
            ShareRoute::CopyText => {
                let text = request.copy_text()?;
                let provider = self.copy.as_ref().ok_or_else(|| {
                    error(
                        ErrorCode::Unsupported,
                        "No explicit copy provider is installed",
                    )
                })?;
                provider.capability().check()?;
                validate_context(&context)?;
                if let Some(reason) = context.cancellation.reason() {
                    return Ok(Completion::Cancelled(reason));
                }
                provider
                    .copy_text(text, &context)
                    .map(|completion| match completion {
                        Completion::Completed(()) => {
                            Completion::Completed(ShareReceipt::FallbackPerformed {
                                kind: ShareFallbackKind::CopyText,
                            })
                        }
                        Completion::Cancelled(reason) => Completion::Cancelled(reason),
                    })
            }
        }
    }
}
fn validate_context(context: &OperationContext) -> Result<(), SystemError> {
    let ui = context.require_ui()?;
    context.check_at(Instant::now())?;
    if ui.anchor.is_some_and(|a| {
        !a.x.is_finite()
            || !a.y.is_finite()
            || !a.width.is_finite()
            || !a.height.is_finite()
            || a.width < 0.0
            || a.height < 0.0
    }) {
        return Err(error(
            ErrorCode::InvalidInput,
            "Share anchor geometry is invalid",
        ));
    }
    Ok(())
}
pub fn unsupported(operation: ShareOperation) -> Capability<ShareOperation> {
    Capability {
        operation,
        support: Support::Unsupported,
        availability: Availability::Unavailable,
        configuration: Configuration::Ready,
        permission: PermissionState::NotRequired,
        evidence: Evidence::ContractOnly,
        constraints: vec![Constraint::ForegroundOwner],
        observed_revision: 0,
    }
}
pub struct UnavailableSharingProvider;
impl SharingProvider for UnavailableSharingProvider {
    fn capability(&self, operation: ShareOperation) -> Capability<ShareOperation> {
        unsupported(operation)
    }
    fn share(&self, _: &ShareRequest, _: &OperationContext) -> ServiceResult<ShareReceipt> {
        Err(error(
            ErrorCode::Unavailable,
            "No sharing provider is installed",
        ))
    }
}
/// Provider implementation presence, separate from device/runtime qualification.
pub fn native_provider_available(platform: Platform) -> bool {
    platform == Platform::Windows && cfg!(target_os = "windows")
}

/// Single-use user-action attestation tied to one service and UI generation.
/// It cannot keep the native window alive or be reused after recreation.
pub struct ShareInteraction {
    context: OperationContext,
    binding: Rc<UiContext>,
}
impl ShareInteraction {
    pub fn with_cancellation(mut self, token: gpui_system::CancellationToken) -> Self {
        self.context.cancellation = token;
        self
    }
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.context.deadline = Some(deadline);
        self
    }
}
