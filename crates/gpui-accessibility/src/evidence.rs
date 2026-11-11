//! Separate automation and real assistive-reader acceptance channels.
//!
//! Programmatic harness checks are useful but are *not* an assistive-reader
//! session. `AutomationEvidence` captures deterministic tree/action/audit
//! checks; `AssistiveReaderSessionEvidence` captures a human or device session
//! with Narrator, NVDA, VoiceOver, Orca or TalkBack. An acceptance report can
//! pass its automation gate while its reader gate remains explicitly pending.

use crate::error::EvidenceError;
use crate::platform::{AccessibilityOperation, AccessibilityPlatform};
use crate::scene::{DisplayedScene, SceneRevision, SemanticIdentity, SemanticTreeSnapshot};
use std::collections::BTreeMap;

/// Reader products covered by the acceptance policy. `Other` is used for a
/// browser/screen-reader combination that has no dedicated variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssistiveReader {
    Narrator,
    Nvda,
    VoiceOverMacOs,
    VoiceOverIos,
    Orca,
    TalkBack,
    Other(&'static str),
}

impl AssistiveReader {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Narrator => "Narrator",
            Self::Nvda => "NVDA",
            Self::VoiceOverMacOs => "VoiceOver (macOS)",
            Self::VoiceOverIos => "VoiceOver (iOS)",
            Self::Orca => "Orca",
            Self::TalkBack => "TalkBack",
            Self::Other(name) => name,
        }
    }

    /// The platform this reader product belongs to. `Other` is unconstrained so
    /// a browser/screen-reader pair can be recorded on Web.
    pub const fn platform(self) -> Option<AccessibilityPlatform> {
        match self {
            Self::Narrator | Self::Nvda => Some(AccessibilityPlatform::Windows),
            Self::VoiceOverMacOs => Some(AccessibilityPlatform::MacOs),
            Self::VoiceOverIos => Some(AccessibilityPlatform::Ios),
            Self::Orca => Some(AccessibilityPlatform::Linux),
            Self::TalkBack => Some(AccessibilityPlatform::Android),
            Self::Other(_) => None,
        }
    }
}

/// Scenarios a real reader session must cover before acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AssistiveReaderScenario {
    KeyboardTraversal,
    ReaderTraversal,
    CjkTextSelection,
    DynamicVirtualList,
    ModalFocusIsolation,
    LiveAnnouncements,
}

impl AssistiveReaderScenario {
    pub const ALL: [Self; 6] = [
        Self::KeyboardTraversal,
        Self::ReaderTraversal,
        Self::CjkTextSelection,
        Self::DynamicVirtualList,
        Self::ModalFocusIsolation,
        Self::LiveAnnouncements,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::KeyboardTraversal => "keyboard traversal",
            Self::ReaderTraversal => "reader traversal",
            Self::CjkTextSelection => "CJK selection",
            Self::DynamicVirtualList => "dynamic virtual list",
            Self::ModalFocusIsolation => "modal focus isolation",
            Self::LiveAnnouncements => "live announcements",
        }
    }
}

/// Session outcome as reported by the human/device run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistiveReaderSessionStatus {
    Passed,
    Failed,
    Incomplete,
}

/// Programmatic tree/action/selection/audit evidence. It deliberately has no
/// reader, device, OS or human-session fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationEvidence {
    pub platform: AccessibilityPlatform,
    pub operation: AccessibilityOperation,
    pub suite: String,
    pub test: String,
    pub command: String,
    pub tool: Option<String>,
    pub tool_version: Option<String>,
    pub scene: Option<DisplayedScene>,
    pub snapshot: Option<SnapshotSummary>,
    pub passed: bool,
}

impl AutomationEvidence {
    pub fn new(
        platform: AccessibilityPlatform,
        operation: AccessibilityOperation,
        suite: impl Into<String>,
        test: impl Into<String>,
        command: impl Into<String>,
        passed: bool,
    ) -> Self {
        Self {
            platform,
            operation,
            suite: suite.into(),
            test: test.into(),
            command: command.into(),
            tool: None,
            tool_version: None,
            scene: None,
            snapshot: None,
            passed,
        }
    }

    pub fn with_scene(mut self, scene: DisplayedScene) -> Self {
        self.scene = Some(scene);
        self
    }

    pub fn with_snapshot(mut self, snapshot: &SemanticTreeSnapshot) -> Self {
        self.scene = Some(snapshot.displayed_scene());
        self.snapshot = Some(SnapshotSummary::from_snapshot(snapshot));
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>, version: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self.tool_version = Some(version.into());
        self
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        for (field, value) in [
            ("suite", self.suite.as_str()),
            ("test", self.test.as_str()),
            ("command", self.command.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(EvidenceError::EmptyField { field });
            }
        }
        for (field, value) in [
            ("tool", self.tool.as_deref()),
            ("tool_version", self.tool_version.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(EvidenceError::EmptyField { field });
            }
        }
        Ok(())
    }
}

/// Compact, public-safe snapshot provenance attached to automation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub root: SemanticIdentity,
    pub focus: Option<SemanticIdentity>,
    pub node_count: usize,
    pub revision: SceneRevision,
    pub modal: Option<SemanticIdentity>,
}

impl SnapshotSummary {
    pub fn from_snapshot(snapshot: &SemanticTreeSnapshot) -> Self {
        Self {
            root: snapshot.root.clone(),
            focus: snapshot.focus.clone(),
            node_count: snapshot.nodes.len(),
            revision: snapshot.revision,
            modal: snapshot
                .modal_roots()
                .first()
                .map(|node| node.identity.clone()),
        }
    }
}

/// A real assistive-technology session. Every field describes the actual
/// reader, OS/device, channel and scenario coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistiveReaderSessionEvidence {
    pub reader: AssistiveReader,
    pub reader_version: String,
    pub platform: AccessibilityPlatform,
    pub os_version: String,
    pub device: String,
    pub channel: String,
    pub scenarios: Vec<AssistiveReaderScenario>,
    pub status: AssistiveReaderSessionStatus,
    pub recorded_at: String,
    pub evidence_reference: String,
}

impl AssistiveReaderSessionEvidence {
    pub fn new(
        reader: AssistiveReader,
        reader_version: impl Into<String>,
        platform: AccessibilityPlatform,
        status: AssistiveReaderSessionStatus,
    ) -> Self {
        Self {
            reader,
            reader_version: reader_version.into(),
            platform,
            os_version: String::new(),
            device: String::new(),
            channel: String::new(),
            scenarios: Vec::new(),
            status,
            recorded_at: String::new(),
            evidence_reference: String::new(),
        }
    }

    pub fn with_environment(
        mut self,
        os_version: impl Into<String>,
        device: impl Into<String>,
        channel: impl Into<String>,
    ) -> Self {
        self.os_version = os_version.into();
        self.device = device.into();
        self.channel = channel.into();
        self
    }

    pub fn with_scenarios(
        mut self,
        scenarios: impl IntoIterator<Item = AssistiveReaderScenario>,
    ) -> Self {
        self.scenarios = scenarios.into_iter().collect();
        self
    }

    pub fn with_reference(
        mut self,
        recorded_at: impl Into<String>,
        evidence_reference: impl Into<String>,
    ) -> Self {
        self.recorded_at = recorded_at.into();
        self.evidence_reference = evidence_reference.into();
        self
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        for (field, value) in [
            ("reader_version", self.reader_version.as_str()),
            ("os_version", self.os_version.as_str()),
            ("device", self.device.as_str()),
            ("channel", self.channel.as_str()),
            ("recorded_at", self.recorded_at.as_str()),
            ("evidence_reference", self.evidence_reference.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(EvidenceError::EmptyField { field });
            }
        }
        if let Some(expected) = self.reader.platform()
            && expected != self.platform
        {
            return Err(EvidenceError::ReaderPlatformMismatch {
                reader: self.reader,
                platform: self.platform,
            });
        }
        if self.scenarios.is_empty() {
            return Err(EvidenceError::NoScenarios);
        }
        Ok(())
    }

    pub fn missing_scenarios(&self) -> Vec<AssistiveReaderScenario> {
        AssistiveReaderScenario::ALL
            .into_iter()
            .filter(|scenario| !self.scenarios.contains(scenario))
            .collect()
    }

    pub fn covers_required_scenarios(&self) -> bool {
        self.missing_scenarios().is_empty()
    }

    pub fn is_accepted(&self) -> bool {
        self.status == AssistiveReaderSessionStatus::Passed && self.covers_required_scenarios()
    }
}

/// Automation gate state. `Recorded` may still contain a failed run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationGate {
    Pending,
    Recorded(Box<AutomationEvidence>),
}

/// Assistive-reader gate state. The variant name deliberately keeps pending
/// reader sessions distinct from a passing automation run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistiveReaderGate {
    PendingReaderSession,
    Recorded(AssistiveReaderSessionEvidence),
}

/// Overall acceptance state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceStatus {
    PendingAutomation,
    AutomationFailed,
    PendingReaderSession,
    ReaderSessionFailed,
    Accepted,
}

/// Qualification report for one platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityAcceptanceReport {
    pub platform: AccessibilityPlatform,
    pub automation: AutomationGate,
    pub assistive_reader_session: AssistiveReaderGate,
}

impl AccessibilityAcceptanceReport {
    pub fn new(platform: AccessibilityPlatform) -> Self {
        Self {
            platform,
            automation: AutomationGate::Pending,
            assistive_reader_session: AssistiveReaderGate::PendingReaderSession,
        }
    }

    pub fn record_automation(&mut self, evidence: AutomationEvidence) -> Result<(), EvidenceError> {
        if evidence.platform != self.platform {
            return Err(EvidenceError::PlatformMismatch {
                expected: self.platform,
                actual: evidence.platform,
            });
        }
        evidence.validate()?;
        self.automation = AutomationGate::Recorded(Box::new(evidence));
        Ok(())
    }

    pub fn record_assistive_reader_session(
        &mut self,
        evidence: AssistiveReaderSessionEvidence,
    ) -> Result<(), EvidenceError> {
        if evidence.platform != self.platform {
            return Err(EvidenceError::PlatformMismatch {
                expected: self.platform,
                actual: evidence.platform,
            });
        }
        evidence.validate()?;
        self.assistive_reader_session = AssistiveReaderGate::Recorded(evidence);
        Ok(())
    }

    pub fn automation_passed(&self) -> bool {
        matches!(&self.automation, AutomationGate::Recorded(evidence) if evidence.passed)
    }

    pub fn reader_session_accepted(&self) -> bool {
        matches!(
            &self.assistive_reader_session,
            AssistiveReaderGate::Recorded(evidence) if evidence.is_accepted()
        )
    }

    pub fn status(&self) -> AcceptanceStatus {
        match &self.automation {
            AutomationGate::Pending => return AcceptanceStatus::PendingAutomation,
            AutomationGate::Recorded(evidence) if !evidence.passed => {
                return AcceptanceStatus::AutomationFailed;
            }
            AutomationGate::Recorded(_) => {}
        }
        match &self.assistive_reader_session {
            AssistiveReaderGate::PendingReaderSession => AcceptanceStatus::PendingReaderSession,
            AssistiveReaderGate::Recorded(evidence) if !evidence.is_accepted() => {
                AcceptanceStatus::ReaderSessionFailed
            }
            AssistiveReaderGate::Recorded(_) => AcceptanceStatus::Accepted,
        }
    }

    pub fn is_accepted(&self) -> bool {
        self.status() == AcceptanceStatus::Accepted
    }

    pub fn require_accepted(&self) -> Result<(), EvidenceError> {
        match self.status() {
            AcceptanceStatus::Accepted => Ok(()),
            AcceptanceStatus::PendingAutomation | AcceptanceStatus::AutomationFailed => {
                Err(EvidenceError::AutomationFailed)
            }
            AcceptanceStatus::PendingReaderSession | AcceptanceStatus::ReaderSessionFailed => {
                Err(EvidenceError::ReaderSessionFailed)
            }
        }
    }
}

/// Six-platform report set used by qualification dashboards and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityAcceptanceMatrix {
    reports: BTreeMap<AccessibilityPlatform, AccessibilityAcceptanceReport>,
}

impl Default for AccessibilityAcceptanceMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl AccessibilityAcceptanceMatrix {
    pub fn new() -> Self {
        let reports = AccessibilityPlatform::ALL
            .into_iter()
            .map(|platform| (platform, AccessibilityAcceptanceReport::new(platform)))
            .collect();
        Self { reports }
    }

    pub fn report(
        &self,
        platform: AccessibilityPlatform,
    ) -> Option<&AccessibilityAcceptanceReport> {
        self.reports.get(&platform)
    }

    pub fn report_mut(
        &mut self,
        platform: AccessibilityPlatform,
    ) -> &mut AccessibilityAcceptanceReport {
        self.reports
            .entry(platform)
            .or_insert_with(|| AccessibilityAcceptanceReport::new(platform))
    }

    pub fn statuses(&self) -> Vec<(AccessibilityPlatform, AcceptanceStatus)> {
        self.reports
            .iter()
            .map(|(platform, report)| (*platform, report.status()))
            .collect()
    }

    pub fn all_automation_passed(&self) -> bool {
        self.reports
            .values()
            .all(AccessibilityAcceptanceReport::automation_passed)
    }

    pub fn all_reader_sessions_pending(&self) -> bool {
        self.reports.values().all(|report| {
            report.assistive_reader_session == AssistiveReaderGate::PendingReaderSession
        })
    }
}
