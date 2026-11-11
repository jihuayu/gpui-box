//! System-service contracts. The default build has no GUI or OS dependencies.
//!
//! Capability snapshots never request authorization. Providers must recheck
//! conditions at execution. UI futures are local; only explicitly `Send` data
//! may cross into a background executor. See the repository's SYSTEM_SERVICES.md.

mod capability;
mod context;
mod error;
mod events;
mod registry;
mod request;

pub use capability::*;
pub use context::*;
pub use error::*;
pub use events::*;
pub use registry::*;
pub use request::*;

#[cfg(feature = "gpui")]
pub mod gpui_adapter;

/// Normal operation completion, including user or cooperative cancellation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion<T> {
    Completed(T),
    Cancelled(CancelReason),
}

/// A cancellation is an ordinary result, while a failed operation is an error.
pub type ServiceResult<T> = Result<Completion<T>, SystemError>;

/// Why an operation was cancelled. Cancellation cannot undo a native handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    User,
    Caller,
    OwnerDisposed,
    ProcessPolicy,
}
