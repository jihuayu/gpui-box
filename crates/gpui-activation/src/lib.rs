//! Validated URL opening and a bounded durable activation inbox.
//! Filesystem inbox methods are blocking background primitives; UI adapters stay local.
mod inbox;
mod model;
mod opening;
pub use inbox::*;
pub use model::*;
pub use opening::*;
#[cfg(feature = "gpui")]
pub mod gpui_adapter;

pub use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}
