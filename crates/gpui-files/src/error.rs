//! Typed, static, path-free error construction shared by the crate.

use gpui_system::{
    Completion, ErrorCode, OperationContext, RecoveryAction, RetryAdvice, ServiceResult,
    SystemError,
};
use std::{io, time::Instant};

/// Default streaming chunk bound (64 KiB).
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
pub(crate) const MAX_CHUNK_SIZE: usize = 1024 * 1024;
pub(crate) const MAX_GRANT_BYTES: usize = 16 * 1024;

pub(crate) fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}

pub(crate) fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}

pub(crate) fn invalid_input(message: &'static str) -> SystemError {
    error(ErrorCode::InvalidInput, message)
}

pub(crate) fn background_required() -> SystemError {
    error(
        ErrorCode::InvalidInput,
        "File streaming requires a background context without a UI attachment",
    )
}

pub(crate) fn revoked() -> SystemError {
    let mut error = error(
        ErrorCode::PermissionDenied,
        "Resource access was released or is no longer authorized",
    );
    error.recovery = Some(RecoveryAction::ReselectResource);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn destination_exists() -> SystemError {
    let mut error = error(
        ErrorCode::InvalidInput,
        "A resource already exists at the selected destination",
    );
    error.retry = RetryAdvice::ModifyRequest;
    error
}

pub(crate) fn revision_conflict() -> SystemError {
    let mut error = error(
        ErrorCode::ResourceExpired,
        "The resource changed since the expected revision",
    );
    error.recovery = Some(RecoveryAction::ReselectResource);
    error.retry = RetryAdvice::ModifyRequest;
    error
}

pub(crate) fn unsupported_atomic() -> SystemError {
    error(
        ErrorCode::Unsupported,
        "This provider stream cannot guarantee atomic replacement",
    )
}

pub(crate) fn unsupported_grant() -> SystemError {
    error(
        ErrorCode::Unsupported,
        "This provider does not support durable resource grants",
    )
}

pub(crate) fn busy(message: &'static str) -> SystemError {
    let mut error = error(ErrorCode::Busy, message);
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn provider_io_failure() -> SystemError {
    let mut error = error(ErrorCode::PlatformFailure, "Provider stream I/O failed");
    error.retry = RetryAdvice::AfterConditionsChange;
    error
}

pub(crate) fn io_error(cause: io::Error) -> SystemError {
    match cause.kind() {
        io::ErrorKind::PermissionDenied => revoked(),
        io::ErrorKind::NotFound => {
            let mut error = error(
                ErrorCode::ResourceExpired,
                "The resource is missing or was moved",
            );
            error.recovery = Some(RecoveryAction::ReselectResource);
            error.retry = RetryAdvice::AfterConditionsChange;
            error
        }
        io::ErrorKind::WouldBlock => busy("Resource I/O is temporarily unavailable"),
        io::ErrorKind::StorageFull => {
            let mut error = error(
                ErrorCode::PlatformFailure,
                "The destination has insufficient storage",
            );
            error.retry = RetryAdvice::AfterConditionsChange;
            error
        }
        _ => {
            let mut error = error(ErrorCode::PlatformFailure, "File I/O failed");
            error.retry = RetryAdvice::AfterConditionsChange;
            error
        }
    }
}

/// Enforce the blocking-I/O context contract used by `gpui-app-data`.
pub(crate) fn check_background<T>(
    context: &OperationContext,
) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(background_required());
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}

pub(crate) fn validated_chunk_size(value: usize) -> Result<usize, SystemError> {
    if value == 0 || value > MAX_CHUNK_SIZE {
        return Err(invalid_input("Chunk size must be between 1 byte and 1 MiB"));
    }
    Ok(value)
}
