//! Static, public-safe error construction.
//!
//! Every message in this crate is a `&'static str`. Native paths, provider
//! URIs, document contents and exception payloads never enter an error.

use gpui_system::{
    Completion, ErrorCode, OperationContext, RecoveryAction, RetryAdvice, ServiceResult,
    SystemError,
};
use std::time::Instant;

pub(crate) fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

pub(crate) fn invalid(message: &'static str) -> SystemError {
    error(ErrorCode::InvalidInput, message)
}

pub(crate) fn unsupported(message: &'static str) -> SystemError {
    error(ErrorCode::Unsupported, message)
}

pub(crate) fn expired(message: &'static str) -> SystemError {
    let mut error = error(ErrorCode::ResourceExpired, message);
    error.recovery = Some(RecoveryAction::ProbeAgain);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn context_expired(message: &'static str) -> SystemError {
    error(ErrorCode::ContextExpired, message)
}

pub(crate) fn permission_denied(message: &'static str) -> SystemError {
    error(ErrorCode::PermissionDenied, message)
}

pub(crate) fn busy(message: &'static str) -> SystemError {
    let mut error = error(ErrorCode::Busy, message);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn integrity(message: &'static str) -> SystemError {
    error(ErrorCode::IntegrityFailure, message)
}

/// Background streaming/read contexts must not carry a UI attachment, matching
/// `gpui-files`.
pub(crate) fn check_background<T>(
    context: &OperationContext,
) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(invalid(
            "Streaming requires a background context without a UI attachment",
        ));
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

pub(crate) fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}
