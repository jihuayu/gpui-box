//! Payload-specific sharing with an explicit route and honest handoff receipts.
//! Native files/resources remain owned by gpui-files (#23); this slice never
//! substitutes a raw path or clipboard copy for an authorized resource.
mod model;
mod service;
pub use gpui_activation::{AllowedUrl, UrlPolicy};
pub use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult, SystemError};
pub use model::*;
pub use service::*;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub mod windows;

fn error(code: ErrorCode, message: &'static str) -> SystemError {
    SystemError::new(code, message)
}
