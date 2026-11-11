use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Unsupported,
    Unavailable,
    PermissionDenied,
    InvalidConfiguration,
    InvalidInput,
    ContextExpired,
    Busy,
    TimedOut,
    ResourceExpired,
    PlatformFailure,
    IntegrityFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    OpenSettings,
    ReselectResource,
    Authenticate,
    ProbeAgain,
    RepairConfiguration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryAdvice {
    Never,
    Retry,
    AfterConditionsChange,
    ModifyRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagnosticId(pub u64);

/// Public-safe error information. Messages must be static and must not contain
/// paths, URLs, account identifiers, credentials, or native exception payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemError {
    pub code: ErrorCode,
    pub message: &'static str,
    pub recovery: Option<RecoveryAction>,
    pub retry: RetryAdvice,
    pub diagnostic_id: Option<DiagnosticId>,
}

impl SystemError {
    pub fn new(code: ErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message,
            recovery: None,
            retry: RetryAdvice::Never,
            diagnostic_id: None,
        }
    }
}

impl fmt::Display for SystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl Error for SystemError {}

/// Structured diagnostics deliberately have no arbitrary message/payload field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticEvent {
    pub request: crate::RequestId,
    pub phase: DiagnosticPhase,
    pub code: Option<ErrorCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticPhase {
    Started,
    Completed,
    Cancelled,
    Rejected,
    LateCallbackDiscarded,
}
