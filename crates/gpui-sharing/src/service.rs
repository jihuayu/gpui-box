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

/// Internal bridge constructed by a native host with the same attachment.
#[cfg(target_os = "windows")]
pub(crate) struct ClipboardCopy(pub(crate) gpui_clipboard::Clipboard);
#[cfg(target_os = "windows")]
impl CopyTextProvider for ClipboardCopy {
    fn capability(&self) -> Capability<ShareOperation> {
        let Some(c) = self
            .0
            .capabilities()
            .into_iter()
            .find(|c| c.operation == gpui_clipboard::ClipboardOperation::WriteText)
        else {
            return unsupported(ShareOperation::CopyText);
        };
        Capability {
            operation: ShareOperation::CopyText,
            support: c.support,
            availability: c.availability,
            configuration: c.configuration,
            permission: c.permission,
            evidence: c.evidence,
            constraints: c.constraints,
            observed_revision: c.observed_revision,
        }
    }
    fn copy_text(&self, text: &str, context: &OperationContext) -> ServiceResult<()> {
        let mut interaction = self
            .0
            .user_initiated()?
            .with_cancellation(context.cancellation.clone());
        if let Some(deadline) = context.deadline {
            interaction = interaction.with_deadline(deadline);
        }
        self.0
            .write_text(text, interaction)
            .map(|result| match result {
                Completion::Completed(_) => Completion::Completed(()),
                Completion::Cancelled(reason) => Completion::Cancelled(reason),
            })
    }
}
#[cfg(all(test, target_os = "windows"))]
mod copy_tests {
    use super::*;
    use gpui_clipboard::{
        Clipboard, ClipboardOperation, ClipboardProvider, ClipboardReceipt, ClipboardText,
    };
    use gpui_system::{CancelReason, SurfaceId, UiOwner};
    use std::{cell::Cell, rc::Rc};
    struct NativeCopy {
        calls: Cell<usize>,
        cancel_after: gpui_system::CancellationToken,
    }
    impl ClipboardProvider for NativeCopy {
        fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
            vec![Capability {
                operation: ClipboardOperation::WriteText,
                support: Support::Native,
                availability: Availability::Available,
                configuration: Configuration::Ready,
                permission: PermissionState::NotRequired,
                evidence: Evidence::Compiled,
                constraints: vec![],
                observed_revision: 1,
            }]
        }
        fn read_text(&self, _: &OperationContext) -> ServiceResult<ClipboardText> {
            unreachable!()
        }
        fn write_text(
            &self,
            text: &str,
            context: &OperationContext,
        ) -> ServiceResult<ClipboardReceipt> {
            context.check_at(Instant::now())?;
            if let Some(reason) = context.cancellation.reason() {
                return Ok(Completion::Cancelled(reason));
            }
            assert_eq!(text, "中文 😀");
            self.calls.set(self.calls.get() + 1);
            self.cancel_after.cancel(CancelReason::Caller);
            Ok(Completion::Completed(ClipboardReceipt {
                sequence: Some(12),
            }))
        }
    }
    #[test]
    fn production_clipboard_bridge_preserves_cancellation_deadline_and_unicode() {
        let owner = UiOwner::new(SurfaceId(1));
        let token = gpui_system::CancellationToken::default();
        let provider = Rc::new(NativeCopy {
            calls: Cell::new(0),
            cancel_after: token.clone(),
        });
        let clipboard = Clipboard::new(provider.clone(), owner.context(None).unwrap()).unwrap();
        let bridge = ClipboardCopy(clipboard);
        let mut context = OperationContext {
            ui: Some(owner.context(None).unwrap()),
            deadline: Some(Instant::now() - std::time::Duration::from_secs(1)),
            ..Default::default()
        };
        assert_eq!(
            bridge.copy_text("中文 😀", &context).unwrap_err().code,
            ErrorCode::TimedOut
        );
        context.deadline = None;
        context.cancellation.cancel(CancelReason::User);
        assert_eq!(
            bridge.copy_text("中文 😀", &context).unwrap(),
            Completion::Cancelled(CancelReason::User)
        );
        assert_eq!(provider.calls.get(), 0);
        context.cancellation = token;
        assert_eq!(
            bridge.copy_text("中文 😀", &context).unwrap(),
            Completion::Completed(())
        );
        assert_eq!(provider.calls.get(), 1);
    }
}
