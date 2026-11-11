//! Blocking private storage primitives. Run on a background executor, never the UI thread.
//! Mobile hosts inject private roots; this crate does not guess a mobile container.
mod directories;
mod settings;
pub use directories::*;
pub use settings::*;

use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
use std::{io, time::Instant};

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}
fn io_error(cause: io::Error) -> SystemError {
    let code = match cause.kind() {
        io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
        io::ErrorKind::WouldBlock => ErrorCode::Busy,
        _ => ErrorCode::PlatformFailure,
    };
    let mut error = error(code, "Private storage I/O failed");
    error.retry = gpui_system::RetryAdvice::AfterConditionsChange;
    error
}
fn check<T>(context: &OperationContext) -> Result<Option<Completion<T>>, SystemError> {
    if context.ui.is_some() {
        return Err(error(
            ErrorCode::InvalidInput,
            "Storage requires a background context without a UI attachment",
        ));
    }
    context.check_at(Instant::now())?;
    Ok(context.cancellation.reason().map(Completion::Cancelled))
}
fn completed<T>(value: T) -> ServiceResult<T> {
    Ok(Completion::Completed(value))
}
