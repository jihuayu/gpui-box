//! Foreground, user-initiated plain-text clipboard operations.
//! Attach one UI-local service to a live host context; reattach after recreation.
//! Providers never normalize text or route paste through a second input pipeline.
use gpui_system::{
    Availability, CancellationToken, Capability, Completion, Configuration, Constraint, ErrorCode,
    Evidence, OperationContext, PermissionState, Platform, ServiceResult, Support, SystemError,
    UiContext,
};
use std::{rc::Rc, time::Instant};

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::WindowsClipboard;

/// Both imported and exported UTF-8 text are bounded to one MiB.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOperation {
    ReadText,
    WriteText,
}

/// A clipboard without formats differs from one containing only non-text data.
/// `Text("")` means an explicitly published empty text representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardText {
    Empty,
    NoText,
    Text(String),
}

/// The OS accepted the text representation, not a paste into another application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardReceipt {
    /// A provider observation only. It does not authorize later clearing.
    pub sequence: Option<u32>,
}

/// Trusted host extension point. Every operation must probe foreground/provider
/// conditions freshly. Do not implement with a void API that silently swallows
/// errors. Recheck the supplied context immediately before any native effect; a
/// cancellation after an accepted effect must not erase the actual receipt.
/// Methods execute synchronously on the service's owning UI thread.
pub trait ClipboardProvider {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>>;
    fn read_text(&self, context: &OperationContext) -> ServiceResult<ClipboardText>;
    fn write_text(&self, text: &str, context: &OperationContext)
    -> ServiceResult<ClipboardReceipt>;
}

/// UI-local service bound to exactly the context supplied by its host.
#[derive(Clone)]
pub struct Clipboard {
    provider: Rc<dyn ClipboardProvider>,
    ui: UiContext,
    identity: Rc<()>,
}

/// Consumed once; it is a host attestation of a foreground user gesture, not an
/// OS permission grant. It contains its service's context, so callers cannot pair
/// a gesture from one logical owner with a different operation context.
pub struct ClipboardInteraction {
    identity: Rc<()>,
    context: OperationContext,
}

impl ClipboardInteraction {
    pub fn with_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.context.cancellation = cancellation;
        self
    }
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.context.deadline = Some(deadline);
        self
    }
}

impl Clipboard {
    pub fn new(provider: Rc<dyn ClipboardProvider>, ui: UiContext) -> Result<Self, SystemError> {
        if !ui.is_current() {
            return Err(error(
                ErrorCode::ContextExpired,
                "Clipboard UI context is expired",
            ));
        }
        Ok(Self {
            provider,
            ui,
            identity: Rc::new(()),
        })
    }

    /// Call only directly from a paste/copy/share user action while foreground.
    /// The native provider rechecks foreground ownership before accessing data.
    pub fn user_initiated(&self) -> Result<ClipboardInteraction, SystemError> {
        let context = OperationContext {
            ui: Some(self.ui.clone()),
            ..OperationContext::default()
        };
        context.require_ui()?;
        Ok(ClipboardInteraction {
            identity: self.identity.clone(),
            context,
        })
    }

    /// Snapshot only; observing capabilities never reads clipboard content.
    pub fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        let mut capabilities = self.provider.capabilities();
        if !self.ui.is_current() {
            for capability in &mut capabilities {
                capability.availability = Availability::Unavailable;
            }
        }
        capabilities
    }

    pub fn read_text(&self, interaction: ClipboardInteraction) -> ServiceResult<ClipboardText> {
        if let Some(cancelled) = self.check(&interaction)? {
            return Ok(Completion::Cancelled(cancelled));
        }
        let result = self.provider.read_text(&interaction.context)?;
        if let Completion::Completed(ClipboardText::Text(text)) = &result {
            validate_text(text)?;
        }
        // A synchronous native access may reenter. Once performed, cancellation
        // cannot undo it; report its actual result rather than a fictitious cancel.
        Ok(result)
    }

    pub fn write_text(
        &self,
        text: &str,
        interaction: ClipboardInteraction,
    ) -> ServiceResult<ClipboardReceipt> {
        if let Some(cancelled) = self.check(&interaction)? {
            return Ok(Completion::Cancelled(cancelled));
        }
        validate_text(text)?;
        let receipt = self.provider.write_text(text, &interaction.context)?;
        Ok(receipt)
    }

    fn check(
        &self,
        interaction: &ClipboardInteraction,
    ) -> Result<Option<gpui_system::CancelReason>, SystemError> {
        if !Rc::ptr_eq(&self.identity, &interaction.identity) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Clipboard interaction belongs to another service",
            ));
        }
        interaction.context.check_at(Instant::now())?;
        interaction.context.require_ui()?;
        Ok(interaction.context.cancellation.reason())
    }
}

fn validate_text(text: &str) -> Result<(), SystemError> {
    if text.len() > MAX_TEXT_BYTES || text.contains('\0') {
        return Err(error(
            ErrorCode::InvalidInput,
            "Clipboard text exceeds bounds or contains NUL",
        ));
    }
    Ok(())
}
fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

/// Native provider evidence must be interpreted independently from host wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardPlatform {
    pub platform: Platform,
    pub implemented: bool,
    pub evidence: Evidence,
}
pub const PLATFORMS: [ClipboardPlatform; 5] = [
    ClipboardPlatform {
        platform: Platform::Windows,
        implemented: true,
        evidence: Evidence::Compiled,
    },
    ClipboardPlatform {
        platform: Platform::MacOs,
        implemented: false,
        evidence: Evidence::ContractOnly,
    },
    ClipboardPlatform {
        platform: Platform::Linux,
        implemented: false,
        evidence: Evidence::ContractOnly,
    },
    ClipboardPlatform {
        platform: Platform::Android,
        implemented: false,
        evidence: Evidence::ContractOnly,
    },
    ClipboardPlatform {
        platform: Platform::Ios,
        implemented: false,
        evidence: Evidence::ContractOnly,
    },
];

/// An explicit unavailable service; it never silently substitutes a fallback.
pub struct UnavailableClipboard;
impl ClipboardProvider for UnavailableClipboard {
    fn capabilities(&self) -> Vec<Capability<ClipboardOperation>> {
        capabilities(
            Availability::Unavailable,
            Support::Unsupported,
            Evidence::ContractOnly,
        )
    }
    fn read_text(&self, _: &OperationContext) -> ServiceResult<ClipboardText> {
        Err(error(
            ErrorCode::Unavailable,
            "Clipboard provider is unavailable",
        ))
    }
    fn write_text(&self, _: &str, _: &OperationContext) -> ServiceResult<ClipboardReceipt> {
        Err(error(
            ErrorCode::Unavailable,
            "Clipboard provider is unavailable",
        ))
    }
}
fn capabilities(
    availability: Availability,
    support: Support,
    evidence: Evidence,
) -> Vec<Capability<ClipboardOperation>> {
    [ClipboardOperation::ReadText, ClipboardOperation::WriteText]
        .map(|operation| Capability {
            operation,
            support,
            availability,
            configuration: Configuration::Ready,
            permission: PermissionState::NotRequired,
            evidence,
            constraints: vec![
                Constraint::ForegroundOwner,
                Constraint::Provider("Explicit user interaction required; plain text only"),
            ],
            observed_revision: 0,
        })
        .into()
}

#[cfg(test)]
mod tests;
