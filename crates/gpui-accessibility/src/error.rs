//! Typed, public-safe errors for semantic scenes, actions and evidence.
//!
//! Native adapters convert these values to [`gpui_system::SystemError`] at the
//! service boundary. Messages never contain native payloads, paths or content.

use crate::{
    AccessibilityPlatform, AssistiveReader, SceneRevision, SemanticAction, SemanticIdentity,
    TextSelectionError,
};
use gpui_system::{ErrorCode, RecoveryAction, RetryAdvice, SurfaceId, SystemError};
use std::fmt;

/// Invalid finite bounds. Rectangles are validated before entering a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundsError {
    NonFinite,
    Inverted,
}

impl fmt::Display for BoundsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => f.write_str("bounds contain NaN or infinity"),
            Self::Inverted => f.write_str("bounds are inverted or negative"),
        }
    }
}

impl std::error::Error for BoundsError {}

/// A rejected semantic scene, action, tree or tracker operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticError {
    InvalidBounds {
        node: u64,
        source: BoundsError,
    },
    MissingRoot,
    RootNotInTree {
        node: u64,
    },
    InvalidFocus {
        node: u64,
    },
    DuplicateNode {
        node: u64,
    },
    DuplicateAuthorId {
        author_id: String,
    },
    MissingRelatedNode {
        node: u64,
        related: u64,
    },
    InvalidRelationship {
        node: u64,
        related: u64,
    },
    UnreachableNode {
        node: u64,
    },
    UnknownNode {
        node: u64,
    },
    UnsupportedTree,
    StaleGeneration {
        displayed: u64,
        active: u64,
    },
    StaleRevision {
        displayed: SceneRevision,
        current: SceneRevision,
    },
    SurfaceDisposed {
        surface: SurfaceId,
    },
    SurfaceNotTracked {
        surface: SurfaceId,
    },
    ResourceLimitExceeded {
        limit: usize,
    },
    InvalidTextSelection {
        source: TextSelectionError,
    },
    InvalidActionPayload {
        action: SemanticAction,
    },
    UnsupportedAction {
        action: SemanticAction,
    },
    InvalidAnnouncement,
    NotAList {
        node: u64,
    },
    InvalidSnapshotIdentity(SemanticIdentity),
}

impl From<TextSelectionError> for SemanticError {
    fn from(source: TextSelectionError) -> Self {
        Self::InvalidTextSelection { source }
    }
}

impl fmt::Display for SemanticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBounds { node, source } => {
                write!(f, "semantic node #{node} has invalid bounds: {source}")
            }
            Self::MissingRoot => f.write_str("a semantic tree requires a root node"),
            Self::RootNotInTree { node } => {
                write!(f, "semantic root #{node} is absent from the tree")
            }
            Self::InvalidFocus { node } => {
                write!(f, "semantic focus #{node} is absent from the tree")
            }
            Self::DuplicateNode { node } => write!(f, "semantic node #{node} is duplicated"),
            Self::DuplicateAuthorId { author_id } => {
                write!(f, "semantic author identity {author_id:?} is duplicated")
            }
            Self::MissingRelatedNode { node, related } => {
                write!(
                    f,
                    "semantic node #{node} references missing node #{related}"
                )
            }
            Self::InvalidRelationship { node, related } => {
                write!(
                    f,
                    "semantic node #{node} has an inconsistent link to #{related}"
                )
            }
            Self::UnreachableNode { node } => {
                write!(f, "semantic node #{node} is unreachable from the root")
            }
            Self::UnknownNode { node } => write!(f, "semantic node #{node} is unknown"),
            Self::UnsupportedTree => {
                f.write_str("only the root AccessKit tree is supported per surface")
            }
            Self::StaleGeneration { displayed, active } => write!(
                f,
                "surface generation {displayed} is older than active generation {active}"
            ),
            Self::StaleRevision { displayed, current } => write!(
                f,
                "scene revision {:?} is not newer than current revision {:?}",
                displayed.0, current.0
            ),
            Self::SurfaceDisposed { surface } => {
                write!(f, "surface {} is disposed", surface.0)
            }
            Self::SurfaceNotTracked { surface } => {
                write!(f, "surface {} has no committed scene", surface.0)
            }
            Self::ResourceLimitExceeded { limit } => {
                write!(f, "semantic resource limit of {limit} was exceeded")
            }
            Self::InvalidTextSelection { source } => {
                write!(f, "text selection is invalid: {source}")
            }
            Self::InvalidActionPayload { action } => {
                write!(f, "action {action:?} has an incompatible payload")
            }
            Self::UnsupportedAction { action } => {
                write!(f, "action {action:?} is not supported by the target")
            }
            Self::InvalidAnnouncement => f.write_str("live announcements require non-empty text"),
            Self::NotAList { node } => {
                write!(f, "semantic node #{node} is not a list")
            }
            Self::InvalidSnapshotIdentity(identity) => {
                write!(f, "semantic snapshot identity {identity:?} is invalid")
            }
        }
    }
}

impl std::error::Error for SemanticError {}

/// Invalid automation or assistive-reader evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    EmptyField {
        field: &'static str,
    },
    PlatformMismatch {
        expected: AccessibilityPlatform,
        actual: AccessibilityPlatform,
    },
    ReaderPlatformMismatch {
        reader: AssistiveReader,
        platform: AccessibilityPlatform,
    },
    NoScenarios,
    AutomationFailed,
    ReaderSessionFailed,
    IncompleteScenarios {
        missing: Vec<crate::AssistiveReaderScenario>,
    },
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => {
                write!(f, "evidence field {field:?} must not be empty")
            }
            Self::PlatformMismatch { expected, actual } => write!(
                f,
                "evidence platform {actual:?} does not match report platform {expected:?}"
            ),
            Self::ReaderPlatformMismatch { reader, platform } => write!(
                f,
                "assistive reader {reader:?} does not belong to platform {platform:?}"
            ),
            Self::NoScenarios => f.write_str("assistive-reader evidence requires scenarios"),
            Self::AutomationFailed => f.write_str("automation evidence reports a failure"),
            Self::ReaderSessionFailed => {
                f.write_str("assistive-reader session did not report success")
            }
            Self::IncompleteScenarios { missing } => {
                write!(
                    f,
                    "assistive-reader session is missing scenarios: {missing:?}"
                )
            }
        }
    }
}

impl std::error::Error for EvidenceError {}

impl From<SemanticError> for SystemError {
    fn from(error: SemanticError) -> Self {
        let (code, retry, recovery) = match &error {
            SemanticError::StaleGeneration { .. }
            | SemanticError::SurfaceDisposed { .. }
            | SemanticError::SurfaceNotTracked { .. } => (
                ErrorCode::ContextExpired,
                RetryAdvice::AfterConditionsChange,
                None,
            ),
            SemanticError::StaleRevision { .. } => (
                ErrorCode::ResourceExpired,
                RetryAdvice::AfterConditionsChange,
                None,
            ),
            SemanticError::ResourceLimitExceeded { .. } => (
                ErrorCode::Busy,
                RetryAdvice::AfterConditionsChange,
                Some(RecoveryAction::ProbeAgain),
            ),
            SemanticError::UnsupportedTree
            | SemanticError::UnsupportedAction { .. }
            | SemanticError::NotAList { .. } => (ErrorCode::Unsupported, RetryAdvice::Never, None),
            SemanticError::UnknownNode { .. }
            | SemanticError::MissingRoot
            | SemanticError::RootNotInTree { .. }
            | SemanticError::InvalidFocus { .. }
            | SemanticError::InvalidBounds { .. }
            | SemanticError::DuplicateNode { .. }
            | SemanticError::DuplicateAuthorId { .. }
            | SemanticError::MissingRelatedNode { .. }
            | SemanticError::InvalidRelationship { .. }
            | SemanticError::UnreachableNode { .. }
            | SemanticError::InvalidTextSelection { .. }
            | SemanticError::InvalidActionPayload { .. }
            | SemanticError::InvalidAnnouncement
            | SemanticError::InvalidSnapshotIdentity(_) => {
                (ErrorCode::InvalidInput, RetryAdvice::ModifyRequest, None)
            }
        };
        let mut system = SystemError::new(code, "Accessibility semantics were rejected");
        system.retry = retry;
        system.recovery = recovery;
        system
    }
}

impl From<BoundsError> for SystemError {
    fn from(_error: BoundsError) -> Self {
        let mut system = SystemError::new(ErrorCode::InvalidInput, "Semantic bounds are invalid");
        system.retry = RetryAdvice::ModifyRequest;
        system
    }
}

impl From<TextSelectionError> for SystemError {
    fn from(_error: TextSelectionError) -> Self {
        let mut system = SystemError::new(
            ErrorCode::InvalidInput,
            "Text selection offsets are invalid",
        );
        system.retry = RetryAdvice::ModifyRequest;
        system
    }
}

impl From<EvidenceError> for SystemError {
    fn from(error: EvidenceError) -> Self {
        let mut system = match &error {
            EvidenceError::AutomationFailed
            | EvidenceError::ReaderSessionFailed
            | EvidenceError::IncompleteScenarios { .. } => {
                SystemError::new(ErrorCode::InvalidInput, "Acceptance evidence did not pass")
            }
            EvidenceError::EmptyField { .. }
            | EvidenceError::PlatformMismatch { .. }
            | EvidenceError::ReaderPlatformMismatch { .. }
            | EvidenceError::NoScenarios => {
                SystemError::new(ErrorCode::InvalidInput, "Acceptance evidence is malformed")
            }
        };
        system.retry = RetryAdvice::ModifyRequest;
        system
    }
}
