//! Semantic inventory, scene correlation and AccessKit conversion.
//!
//! The types in this module are serializable snapshots. They do not own native
//! handles and can therefore be audited, persisted in test evidence, or reduced
//! to a platform bridge payload without touching a native UI thread.

use crate::error::{BoundsError, SemanticError};
use crate::text::{
    ScalarSelection, TextSelectionError, TextSelectionSpan, Utf8Selection, Utf16Selection,
};
use accesskit::{Action, Node, NodeId, Rect, Role, Toggled, TreeId, TreeUpdate};
use gpui_system::SurfaceId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// A monotonically advancing semantic scene revision for one surface
/// generation. Equal or older revisions are stale within a generation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct SceneRevision(pub u64);

impl SceneRevision {
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// The surface generation and revision a semantic tree belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayedScene {
    #[serde(with = "surface_id_serde")]
    pub surface: SurfaceId,
    pub generation: u64,
    pub revision: SceneRevision,
}

impl DisplayedScene {
    pub const fn new(surface: SurfaceId, generation: u64, revision: SceneRevision) -> Self {
        Self {
            surface,
            generation,
            revision,
        }
    }

    pub const fn same_surface_and_generation(self, other: Self) -> bool {
        self.surface.0 == other.surface.0 && self.generation == other.generation
    }
}

mod surface_id_serde {
    use gpui_system::SurfaceId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(surface: &SurfaceId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(surface.0)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SurfaceId, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(SurfaceId(u64::deserialize(deserializer)?))
    }
}

/// Persistent semantic identity. The AccessKit node ID is render-local; an
/// `author_id` (when supplied by the application or AccessKit author) is the
/// stable cross-render key used for identity-retention audits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SemanticIdentity {
    pub node_id: u64,
    pub author_id: Option<String>,
}

impl SemanticIdentity {
    pub fn new(node_id: u64, author_id: Option<String>) -> Result<Self, SemanticError> {
        if author_id.as_deref().is_some_and(str::is_empty) {
            return Err(SemanticError::InvalidSnapshotIdentity(Self {
                node_id,
                author_id,
            }));
        }
        Ok(Self { node_id, author_id })
    }

    pub fn at(node_id: impl Into<u64>) -> Self {
        Self {
            node_id: node_id.into(),
            author_id: None,
        }
    }

    pub fn with_author_id(
        node_id: impl Into<u64>,
        author_id: impl Into<String>,
    ) -> Result<Self, SemanticError> {
        Self::new(node_id.into(), Some(author_id.into()))
    }

    pub fn node(&self) -> accesskit::NodeId {
        accesskit::NodeId(self.node_id)
    }

    /// Identity comparison prefers the stable author key when both sides have
    /// one. Without a stable key the render-local node ID is authoritative.
    pub fn same_identity(&self, other: &Self) -> bool {
        match (&self.author_id, &other.author_id) {
            (Some(left), Some(right)) => left == right,
            _ => self.node_id == other.node_id,
        }
    }

    pub fn key(&self) -> IdentityKey {
        match &self.author_id {
            Some(author_id) => IdentityKey::Author(author_id.clone()),
            None => IdentityKey::Node(self.node_id),
        }
    }
}

/// Stable key used by identity-retention audits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentityKey {
    Author(String),
    Node(u64),
}

impl fmt::Display for SemanticIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.author_id {
            Some(author_id) => write!(f, "#{} ({author_id})", self.node_id),
            None => write!(f, "#{}", self.node_id),
        }
    }
}

/// Application-level roles mapped from AccessKit. Unknown and presentation
/// roles are retained rather than dropped so audits never lose a focusable node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticRole {
    Window,
    Group,
    Button,
    TextInput,
    StaticText,
    CheckBox,
    Switch,
    Dialog,
    BottomSheet,
    List,
    ListItem,
    Heading,
    LiveRegion,
    ScrollView,
    Toolbar,
    Tab,
    TabList,
    Unknown,
}

impl SemanticRole {
    pub fn from_accesskit(role: Role) -> Self {
        match role {
            Role::Window | Role::Application | Role::RootWebArea => Self::Window,
            Role::Group
            | Role::GenericContainer
            | Role::Pane
            | Role::Section
            | Role::Region
            | Role::Main
            | Role::Navigation
            | Role::Form
            | Role::Article
            | Role::Complementary
            | Role::Banner
            | Role::ContentInfo
            | Role::Header
            | Role::Footer => Self::Group,
            Role::Button | Role::DefaultButton => Self::Button,
            Role::TextInput
            | Role::MultilineTextInput
            | Role::SearchInput
            | Role::EmailInput
            | Role::NumberInput
            | Role::PasswordInput
            | Role::PhoneNumberInput
            | Role::UrlInput
            | Role::DateInput
            | Role::DateTimeInput
            | Role::WeekInput
            | Role::MonthInput
            | Role::TimeInput
            | Role::EditableComboBox => Self::TextInput,
            Role::Label | Role::Paragraph | Role::TextRun => Self::StaticText,
            Role::CheckBox | Role::MenuItemCheckBox => Self::CheckBox,
            Role::Switch => Self::Switch,
            Role::Dialog | Role::AlertDialog => Self::Dialog,
            Role::List | Role::ListBox => Self::List,
            Role::ListItem | Role::ListBoxOption | Role::TreeItem => Self::ListItem,
            Role::Heading => Self::Heading,
            Role::Status
            | Role::Alert
            | Role::Log
            | Role::Timer
            | Role::Marquee
            | Role::ProgressIndicator => Self::LiveRegion,
            Role::ScrollView => Self::ScrollView,
            Role::Toolbar | Role::MenuBar => Self::Toolbar,
            Role::Tab => Self::Tab,
            Role::TabList => Self::TabList,
            _ => Self::Unknown,
        }
    }

    pub const fn accesskit_role(self) -> Role {
        match self {
            Self::Window => Role::Window,
            Self::Group => Role::Group,
            Self::Button => Role::Button,
            Self::TextInput => Role::TextInput,
            Self::StaticText => Role::Label,
            Self::CheckBox => Role::CheckBox,
            Self::Switch => Role::Switch,
            Self::Dialog | Self::BottomSheet => Role::Dialog,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Heading => Role::Heading,
            Self::LiveRegion => Role::Status,
            Self::ScrollView => Role::ScrollView,
            Self::Toolbar => Role::Toolbar,
            Self::Tab => Role::Tab,
            Self::TabList => Role::TabList,
            Self::Unknown => Role::Unknown,
        }
    }

    pub const fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::TextInput
                | Self::CheckBox
                | Self::Switch
                | Self::Tab
                | Self::ListItem
        )
    }

    pub const fn is_modal(self) -> bool {
        matches!(self, Self::Dialog | Self::BottomSheet)
    }
}

/// Live-region priority. AccessKit's `Live::Off` maps to `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LivePriority {
    Polite,
    Assertive,
}

impl LivePriority {
    pub const fn from_accesskit(live: accesskit::Live) -> Option<Self> {
        match live {
            accesskit::Live::Off => None,
            accesskit::Live::Polite => Some(Self::Polite),
            accesskit::Live::Assertive => Some(Self::Assertive),
        }
    }

    pub const fn accesskit_live(self) -> accesskit::Live {
        match self {
            Self::Polite => accesskit::Live::Polite,
            Self::Assertive => accesskit::Live::Assertive,
        }
    }
}

/// Tri-state checked value so mixed checkbox state is never flattened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticChecked {
    False,
    True,
    Mixed,
}

impl SemanticChecked {
    pub const fn as_bool(self) -> Option<bool> {
        match self {
            Self::False => Some(false),
            Self::True => Some(true),
            Self::Mixed => None,
        }
    }

    pub const fn from_accesskit(toggled: Toggled) -> Self {
        match toggled {
            Toggled::False => Self::False,
            Toggled::True => Self::True,
            Toggled::Mixed => Self::Mixed,
        }
    }

    pub const fn accesskit_toggled(self) -> Toggled {
        match self {
            Self::False => Toggled::False,
            Self::True => Toggled::True,
            Self::Mixed => Toggled::Mixed,
        }
    }
}

/// Semantic state used by audits. `checked`, `expanded` and `live` preserve
/// absent versus false/off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticStates {
    pub focused: bool,
    pub focusable: bool,
    pub disabled: bool,
    pub hidden: bool,
    pub selected: bool,
    pub checked: Option<SemanticChecked>,
    pub expanded: Option<bool>,
    pub modal: bool,
    pub live: Option<LivePriority>,
    pub read_only: bool,
}

/// Relationships are stored as persistent identities so a relation survives a
/// render-local node-ID change when the author supplied a stable ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticRelationships {
    pub parent: Option<SemanticIdentity>,
    pub children: Vec<SemanticIdentity>,
    pub labelled_by: Vec<SemanticIdentity>,
    pub described_by: Vec<SemanticIdentity>,
    pub active_descendant: Option<SemanticIdentity>,
    pub controls: Vec<SemanticIdentity>,
}

impl SemanticRelationships {
    pub fn child_ids(&self) -> Vec<u64> {
        self.children.iter().map(|child| child.node_id).collect()
    }
}

/// Finite semantic bounds in physical pixels. `x1 >= x0` and `y1 >= y0` are
/// required; NaN and infinity are rejected at construction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SemanticBounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl SemanticBounds {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Self, BoundsError> {
        if !x0.is_finite() || !y0.is_finite() || !x1.is_finite() || !y1.is_finite() {
            return Err(BoundsError::NonFinite);
        }
        if x1 < x0 || y1 < y0 {
            return Err(BoundsError::Inverted);
        }
        Ok(Self { x0, y0, x1, y1 })
    }

    pub fn from_origin_size(x: f64, y: f64, width: f64, height: f64) -> Result<Self, BoundsError> {
        if !width.is_finite() || !height.is_finite() {
            return Err(BoundsError::NonFinite);
        }
        if width < 0.0 || height < 0.0 {
            return Err(BoundsError::Inverted);
        }
        Self::new(x, y, x + width, y + height)
    }

    pub fn from_accesskit_rect(rect: Rect) -> Result<Self, BoundsError> {
        Self::new(rect.x0, rect.y0, rect.x1, rect.y1)
    }

    pub const fn width(self) -> f64 {
        self.x1 - self.x0
    }

    pub const fn height(self) -> f64 {
        self.y1 - self.y0
    }

    pub const fn area(self) -> f64 {
        self.width() * self.height()
    }

    pub fn is_empty(self) -> bool {
        self.area() == 0.0
    }

    pub fn contains_point(self, x: f64, y: f64) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// True when the rectangles overlap, including a shared edge or a
    /// zero-area rectangle. Used to distinguish on-window and offscreen items.
    pub fn intersects(self, other: Self) -> bool {
        self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
    }

    pub fn accesskit_rect(self) -> Rect {
        Rect::new(self.x0, self.y0, self.x1, self.y1)
    }
}

/// Actions accepted by the semantic service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticAction {
    Click,
    Focus,
    SetValue,
    SetTextSelection,
    ScrollIntoView,
    Dismiss,
}

impl SemanticAction {
    pub const fn from_accesskit(action: Action) -> Option<Self> {
        match action {
            Action::Click => Some(Self::Click),
            Action::Focus => Some(Self::Focus),
            Action::SetValue => Some(Self::SetValue),
            Action::SetTextSelection => Some(Self::SetTextSelection),
            Action::ScrollIntoView => Some(Self::ScrollIntoView),
            _ => None,
        }
    }

    pub const fn accesskit_action(self) -> Option<Action> {
        match self {
            Self::Click => Some(Action::Click),
            Self::Focus => Some(Action::Focus),
            Self::SetValue => Some(Action::SetValue),
            Self::SetTextSelection => Some(Action::SetTextSelection),
            Self::ScrollIntoView => Some(Action::ScrollIntoView),
            Self::Dismiss => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Click => "Click",
            Self::Focus => "Focus",
            Self::SetValue => "SetValue",
            Self::SetTextSelection => "SetTextSelection",
            Self::ScrollIntoView => "ScrollIntoView",
            Self::Dismiss => "Dismiss",
        }
    }
}

impl fmt::Display for SemanticAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Typed action payload. `None` is required for click/focus/scroll/dismiss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticActionPayload {
    None,
    Value(String),
    TextSelection(TextSelectionSpan),
}

/// A dispatched accessibility action request. The scene revision makes every
/// request auditable even when it arrives after a re-render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticActionRequest {
    pub scene: DisplayedScene,
    pub target: SemanticIdentity,
    pub action: SemanticAction,
    pub payload: SemanticActionPayload,
}

impl SemanticActionRequest {
    pub fn click(scene: DisplayedScene, target: SemanticIdentity) -> Self {
        Self::without_payload(scene, target, SemanticAction::Click)
    }

    pub fn focus(scene: DisplayedScene, target: SemanticIdentity) -> Self {
        Self::without_payload(scene, target, SemanticAction::Focus)
    }

    pub fn scroll_into_view(scene: DisplayedScene, target: SemanticIdentity) -> Self {
        Self::without_payload(scene, target, SemanticAction::ScrollIntoView)
    }

    pub fn dismiss(scene: DisplayedScene, target: SemanticIdentity) -> Self {
        Self::without_payload(scene, target, SemanticAction::Dismiss)
    }

    pub fn set_value(
        scene: DisplayedScene,
        target: SemanticIdentity,
        value: impl Into<String>,
    ) -> Self {
        Self {
            scene,
            target,
            action: SemanticAction::SetValue,
            payload: SemanticActionPayload::Value(value.into()),
        }
    }

    pub fn set_text_selection(
        scene: DisplayedScene,
        target: SemanticIdentity,
        selection: TextSelectionSpan,
    ) -> Self {
        Self {
            scene,
            target,
            action: SemanticAction::SetTextSelection,
            payload: SemanticActionPayload::TextSelection(selection),
        }
    }

    fn without_payload(
        scene: DisplayedScene,
        target: SemanticIdentity,
        action: SemanticAction,
    ) -> Self {
        Self {
            scene,
            target,
            action,
            payload: SemanticActionPayload::None,
        }
    }

    pub fn validate(&self) -> Result<(), SemanticError> {
        let valid = matches!(
            (&self.action, &self.payload),
            (
                SemanticAction::Click
                    | SemanticAction::Focus
                    | SemanticAction::ScrollIntoView
                    | SemanticAction::Dismiss,
                SemanticActionPayload::None,
            ) | (SemanticAction::SetValue, SemanticActionPayload::Value(_))
                | (
                    SemanticAction::SetTextSelection,
                    SemanticActionPayload::TextSelection(_)
                )
        );
        if valid {
            Ok(())
        } else {
            Err(SemanticError::InvalidActionPayload {
                action: self.action,
            })
        }
    }

    pub fn selection(&self) -> Option<TextSelectionSpan> {
        match self.payload {
            SemanticActionPayload::TextSelection(selection) => Some(selection),
            _ => None,
        }
    }

    pub fn value(&self) -> Option<&str> {
        match &self.payload {
            SemanticActionPayload::Value(value) => Some(value),
            _ => None,
        }
    }
}

/// Receipt for a dispatched action. It reports local acceptance by the bridge,
/// not proof that the native platform completed the effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityActionReceipt {
    pub scene: DisplayedScene,
    pub target: SemanticIdentity,
    pub action: SemanticAction,
    pub accepted: bool,
}

/// Editable-text action request. `text` is the exact text-run value used to
/// validate the directed scalar selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelectionRequest {
    pub scene: DisplayedScene,
    pub target: SemanticIdentity,
    pub text: String,
    pub selection: TextSelectionSpan,
}

impl TextSelectionRequest {
    pub fn new(
        scene: DisplayedScene,
        target: SemanticIdentity,
        text: impl Into<String>,
        selection: TextSelectionSpan,
    ) -> Self {
        Self {
            scene,
            target,
            text: text.into(),
            selection,
        }
    }

    pub fn validate(&self) -> Result<(), TextSelectionError> {
        self.selection.validate(&self.text)
    }
}

/// Receipt carrying the same directed selection in every supported unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelectionReceipt {
    pub scene: DisplayedScene,
    pub target: SemanticIdentity,
    pub selection: TextSelectionSpan,
    pub utf8: Utf8Selection,
    pub scalar: ScalarSelection,
    pub utf16: Utf16Selection,
}

impl TextSelectionReceipt {
    pub fn from_request(request: &TextSelectionRequest) -> Result<Self, TextSelectionError> {
        Ok(Self {
            scene: request.scene,
            target: request.target.clone(),
            selection: request.selection,
            utf8: request.selection.to_utf8(&request.text)?,
            scalar: request.selection.scalar(),
            utf16: request.selection.to_utf16(&request.text)?,
        })
    }
}

/// One semantic node in a committed scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticNodeSnapshot {
    pub identity: SemanticIdentity,
    pub role: SemanticRole,
    pub name: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub placeholder: Option<String>,
    pub states: SemanticStates,
    pub relationships: SemanticRelationships,
    pub bounds: Option<SemanticBounds>,
    pub actions: Vec<SemanticAction>,
    /// Zero-based position in the set, converted from AccessKit's one-based
    /// `position_in_set`.
    pub index_in_set: Option<usize>,
    pub size_of_set: Option<usize>,
}

impl SemanticNodeSnapshot {
    pub fn supports_action(&self, action: SemanticAction) -> bool {
        self.actions.contains(&action)
    }

    pub fn is_hidden_or_disabled(&self) -> bool {
        self.states.hidden || self.states.disabled
    }

    /// A node is audited as an interactive background control when it has
    /// focus semantics, any exposed action, or an interactive role.
    pub fn is_interactive_or_focusable(&self) -> bool {
        self.states.focusable || !self.actions.is_empty() || self.role.is_interactive()
    }

    pub fn accesskit_node(&self) -> Node {
        let mut node = Node::new(self.role.accesskit_role());
        if let Some(name) = &self.name {
            node.set_label(name.clone());
        }
        if let Some(description) = &self.description {
            node.set_description(description.clone());
        }
        if let Some(value) = &self.value {
            node.set_value(value.clone());
        }
        if let Some(placeholder) = &self.placeholder {
            node.set_placeholder(placeholder.clone());
        }
        if let Some(author_id) = &self.identity.author_id {
            node.set_author_id(author_id.clone());
        }
        if let Some(bounds) = self.bounds {
            node.set_bounds(bounds.accesskit_rect());
        }
        node.set_children(
            self.relationships
                .children
                .iter()
                .map(|child| child.node())
                .collect::<Vec<_>>(),
        );
        node.set_labelled_by(
            self.relationships
                .labelled_by
                .iter()
                .map(|node| node.node())
                .collect::<Vec<_>>(),
        );
        node.set_described_by(
            self.relationships
                .described_by
                .iter()
                .map(|node| node.node())
                .collect::<Vec<_>>(),
        );
        node.set_controls(
            self.relationships
                .controls
                .iter()
                .map(|node| node.node())
                .collect::<Vec<_>>(),
        );
        if let Some(active) = &self.relationships.active_descendant {
            node.set_active_descendant(active.node());
        }
        for action in &self.actions {
            if let Some(action) = action.accesskit_action() {
                node.add_action(action);
            }
        }
        if self.states.hidden {
            node.set_hidden();
        }
        if self.states.disabled {
            node.set_disabled();
        }
        if self.states.read_only {
            node.set_read_only();
        }
        if self.states.selected {
            node.set_selected(true);
        }
        if self.states.modal {
            node.set_modal();
        }
        if let Some(expanded) = self.states.expanded {
            node.set_expanded(expanded);
        }
        if let Some(checked) = self.states.checked {
            node.set_toggled(checked.accesskit_toggled());
        }
        if let Some(live) = self.states.live {
            node.set_live(live.accesskit_live());
        }
        if let Some(index) = self.index_in_set {
            node.set_position_in_set(index.saturating_add(1));
        }
        if let Some(size) = self.size_of_set {
            node.set_size_of_set(size);
        }
        node
    }
}

/// A complete committed semantic tree for one `DisplayedScene`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticTreeSnapshot {
    #[serde(with = "surface_id_serde")]
    pub surface: SurfaceId,
    pub generation: u64,
    pub revision: SceneRevision,
    pub root: SemanticIdentity,
    pub focus: Option<SemanticIdentity>,
    pub nodes: Vec<SemanticNodeSnapshot>,
}

impl SemanticTreeSnapshot {
    pub const fn displayed_scene(&self) -> DisplayedScene {
        DisplayedScene {
            surface: self.surface,
            generation: self.generation,
            revision: self.revision,
        }
    }

    pub fn node(&self, identity: &SemanticIdentity) -> Option<&SemanticNodeSnapshot> {
        self.nodes
            .iter()
            .find(|node| node.identity.same_identity(identity))
    }

    pub fn node_by_node_id(&self, node_id: u64) -> Option<&SemanticNodeSnapshot> {
        self.nodes
            .iter()
            .find(|node| node.identity.node_id == node_id)
    }

    pub fn node_by_author_id(&self, author_id: &str) -> Option<&SemanticNodeSnapshot> {
        self.nodes.iter().find(|node| {
            node.identity
                .author_id
                .as_deref()
                .is_some_and(|candidate| candidate == author_id)
        })
    }

    pub fn contains(&self, identity: &SemanticIdentity) -> bool {
        self.node(identity).is_some()
    }

    pub fn children(&self, identity: &SemanticIdentity) -> Vec<&SemanticNodeSnapshot> {
        let Some(node) = self.node(identity) else {
            return Vec::new();
        };
        node.relationships
            .children
            .iter()
            .filter_map(|child| self.node_by_node_id(child.node_id))
            .collect()
    }

    pub fn descendants(&self, identity: &SemanticIdentity) -> BTreeSet<u64> {
        let mut visited = BTreeSet::new();
        let mut pending = VecDeque::new();
        pending.push_back(identity.node_id);
        while let Some(node_id) = pending.pop_front() {
            if !visited.insert(node_id) {
                continue;
            }
            if let Some(node) = self.node_by_node_id(node_id) {
                pending.extend(
                    node.relationships
                        .children
                        .iter()
                        .map(|child| child.node_id),
                );
            }
        }
        visited
    }

    pub fn contains_subtree_node(
        &self,
        ancestor: &SemanticIdentity,
        candidate: &SemanticIdentity,
    ) -> bool {
        let Some(candidate) = self.node(candidate) else {
            return false;
        };
        self.descendants(ancestor)
            .contains(&candidate.identity.node_id)
    }

    pub fn ancestors(&self, identity: &SemanticIdentity) -> Vec<&SemanticNodeSnapshot> {
        let mut result = Vec::new();
        let Some(mut node) = self.node(identity) else {
            return result;
        };
        let mut seen = BTreeSet::new();
        while let Some(parent) = &node.relationships.parent {
            if !seen.insert(parent.node_id) {
                break;
            }
            let Some(parent_node) = self.node_by_node_id(parent.node_id) else {
                break;
            };
            result.push(parent_node);
            node = parent_node;
        }
        result
    }

    /// A background control inherits hidden/disabled/inert state from any
    /// ancestor. This prevents a hidden modal container from producing dozens
    /// of false-positive leak reports.
    pub fn is_effectively_inert(&self, identity: &SemanticIdentity) -> bool {
        self.node(identity)
            .is_some_and(|node| node.states.hidden || node.states.disabled)
            || self
                .ancestors(identity)
                .into_iter()
                .any(|ancestor| ancestor.states.hidden || ancestor.states.disabled)
    }

    pub fn modal_roots(&self) -> Vec<&SemanticNodeSnapshot> {
        self.nodes
            .iter()
            .filter(|node| node.role.is_modal() && node.states.modal && !node.states.hidden)
            .collect()
    }

    /// Structural validation used by semantic commits and audits.
    pub fn validate(&self) -> Result<(), SemanticError> {
        if self.nodes.is_empty() {
            return Err(SemanticError::MissingRoot);
        }
        let mut node_ids = BTreeSet::new();
        let mut author_ids = BTreeSet::new();
        for node in &self.nodes {
            if !node_ids.insert(node.identity.node_id) {
                return Err(SemanticError::DuplicateNode {
                    node: node.identity.node_id,
                });
            }
            if let Some(author_id) = &node.identity.author_id
                && !author_ids.insert(author_id.clone())
            {
                return Err(SemanticError::DuplicateAuthorId {
                    author_id: author_id.clone(),
                });
            }
        }
        let root = self
            .node_by_node_id(self.root.node_id)
            .ok_or(SemanticError::RootNotInTree {
                node: self.root.node_id,
            })?;
        if root.relationships.parent.is_some() {
            return Err(SemanticError::InvalidRelationship {
                node: root.identity.node_id,
                related: root
                    .relationships
                    .parent
                    .as_ref()
                    .map_or(root.identity.node_id, |parent| parent.node_id),
            });
        }
        let focus = self.focus.as_ref().ok_or(SemanticError::InvalidFocus {
            node: self.root.node_id,
        })?;
        if self.node(focus).is_none() {
            return Err(SemanticError::InvalidFocus {
                node: focus.node_id,
            });
        }
        for node in &self.nodes {
            let is_root = node.identity.node_id == self.root.node_id;
            if !is_root {
                let parent = node.relationships.parent.as_ref().ok_or(
                    SemanticError::InvalidRelationship {
                        node: node.identity.node_id,
                        related: self.root.node_id,
                    },
                )?;
                let parent_node = self.node_by_node_id(parent.node_id).ok_or(
                    SemanticError::MissingRelatedNode {
                        node: node.identity.node_id,
                        related: parent.node_id,
                    },
                )?;
                if !parent_node
                    .relationships
                    .children
                    .iter()
                    .any(|child| child.node_id == node.identity.node_id)
                {
                    return Err(SemanticError::InvalidRelationship {
                        node: parent.node_id,
                        related: node.identity.node_id,
                    });
                }
            }
            let mut seen_children = BTreeSet::new();
            for child in &node.relationships.children {
                if child.node_id == node.identity.node_id || !seen_children.insert(child.node_id) {
                    return Err(SemanticError::InvalidRelationship {
                        node: node.identity.node_id,
                        related: child.node_id,
                    });
                }
                let child_node = self.node_by_node_id(child.node_id).ok_or(
                    SemanticError::MissingRelatedNode {
                        node: node.identity.node_id,
                        related: child.node_id,
                    },
                )?;
                if child_node
                    .relationships
                    .parent
                    .as_ref()
                    .is_none_or(|parent| parent.node_id != node.identity.node_id)
                {
                    return Err(SemanticError::InvalidRelationship {
                        node: node.identity.node_id,
                        related: child.node_id,
                    });
                }
            }
        }
        let reachable = self.descendants(&self.root);
        for node in &self.nodes {
            if !reachable.contains(&node.identity.node_id) {
                return Err(SemanticError::UnreachableNode {
                    node: node.identity.node_id,
                });
            }
        }
        Ok(())
    }

    /// Build a snapshot from AccessKit's already-validated tree representation.
    pub fn from_accesskit_update(
        scene: DisplayedScene,
        update: &TreeUpdate,
    ) -> Result<Self, SemanticError> {
        if update.tree_id != TreeId::ROOT {
            return Err(SemanticError::UnsupportedTree);
        }
        let tree = update.tree.as_ref().ok_or(SemanticError::MissingRoot)?;
        let nodes = update
            .nodes
            .iter()
            .cloned()
            .collect::<BTreeMap<NodeId, Node>>();
        Self::from_accesskit_nodes(scene, tree.root, update.focus, &nodes, None)
    }

    pub(crate) fn from_accesskit_nodes(
        scene: DisplayedScene,
        root: NodeId,
        focus: NodeId,
        nodes: &BTreeMap<NodeId, Node>,
        max_nodes: Option<usize>,
    ) -> Result<Self, SemanticError> {
        if let Some(limit) = max_nodes
            && nodes.len() > limit
        {
            return Err(SemanticError::ResourceLimitExceeded { limit });
        }
        if !nodes.contains_key(&root) {
            return Err(SemanticError::RootNotInTree { node: root.0 });
        }

        let mut order = Vec::new();
        let mut parents: BTreeMap<u64, Option<NodeId>> = BTreeMap::new();
        let mut pending = vec![(root, None::<NodeId>)];
        while let Some((node_id, parent)) = pending.pop() {
            if parents.contains_key(&node_id.0) {
                return Err(SemanticError::InvalidRelationship {
                    node: parent.map_or(node_id.0, |parent| parent.0),
                    related: node_id.0,
                });
            }
            let node = nodes
                .get(&node_id)
                .ok_or(SemanticError::MissingRelatedNode {
                    node: parent.map_or(node_id.0, |parent| parent.0),
                    related: node_id.0,
                })?;
            parents.insert(node_id.0, parent);
            order.push(node_id);
            for child in node.children().iter().rev() {
                pending.push((*child, Some(node_id)));
            }
        }

        let mut identities = BTreeMap::new();
        let mut authors: BTreeMap<String, u64> = BTreeMap::new();
        for node_id in &order {
            let node = &nodes[node_id];
            let author_id = node
                .author_id()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            if let Some(author_id) = &author_id
                && authors.insert(author_id.clone(), node_id.0).is_some()
            {
                return Err(SemanticError::DuplicateAuthorId {
                    author_id: author_id.clone(),
                });
            }
            identities.insert(node_id.0, SemanticIdentity::new(node_id.0, author_id)?);
        }

        let relationship_identity = |node_id: &NodeId| {
            identities
                .get(&node_id.0)
                .cloned()
                .unwrap_or_else(|| SemanticIdentity::at(node_id.0))
        };

        let mut snapshots = Vec::with_capacity(order.len());
        for node_id in &order {
            let node = &nodes[node_id];
            let parent = parents
                .get(&node_id.0)
                .copied()
                .flatten()
                .map(|parent| relationship_identity(&parent));
            let bounds = match node.bounds() {
                Some(rect) => {
                    Some(SemanticBounds::from_accesskit_rect(rect).map_err(|source| {
                        SemanticError::InvalidBounds {
                            node: node_id.0,
                            source,
                        }
                    })?)
                }
                None => None,
            };
            let mut actions = Vec::new();
            for action in [
                Action::Click,
                Action::Focus,
                Action::SetValue,
                Action::SetTextSelection,
                Action::ScrollIntoView,
            ] {
                if node.supports_action(action)
                    && let Some(action) = SemanticAction::from_accesskit(action)
                {
                    actions.push(action);
                }
            }
            snapshots.push(SemanticNodeSnapshot {
                identity: relationship_identity(node_id),
                role: SemanticRole::from_accesskit(node.role()),
                name: node.label().map(str::to_owned),
                description: node.description().map(str::to_owned),
                value: node.value().map(str::to_owned),
                placeholder: node.placeholder().map(str::to_owned),
                states: SemanticStates {
                    focused: focus == *node_id,
                    focusable: node.supports_action(Action::Focus),
                    disabled: node.is_disabled(),
                    hidden: node.is_hidden(),
                    selected: node.is_selected().unwrap_or(false),
                    checked: node.toggled().map(SemanticChecked::from_accesskit),
                    expanded: node.is_expanded(),
                    modal: node.is_modal(),
                    live: node.live().and_then(LivePriority::from_accesskit),
                    read_only: node.is_read_only(),
                },
                relationships: SemanticRelationships {
                    parent,
                    children: node.children().iter().map(relationship_identity).collect(),
                    labelled_by: node
                        .labelled_by()
                        .iter()
                        .map(relationship_identity)
                        .collect(),
                    described_by: node
                        .described_by()
                        .iter()
                        .map(relationship_identity)
                        .collect(),
                    active_descendant: node.active_descendant().as_ref().map(relationship_identity),
                    controls: node.controls().iter().map(relationship_identity).collect(),
                },
                bounds,
                actions,
                index_in_set: node.position_in_set().map(|value| value.saturating_sub(1)),
                size_of_set: node.size_of_set(),
            });
        }

        let focus_identity = identities
            .get(&focus.0)
            .cloned()
            .ok_or(SemanticError::InvalidFocus { node: focus.0 })?;
        let root_identity = identities
            .get(&root.0)
            .cloned()
            .ok_or(SemanticError::RootNotInTree { node: root.0 })?;
        let snapshot = Self {
            surface: scene.surface,
            generation: scene.generation,
            revision: scene.revision,
            root: root_identity,
            focus: Some(focus_identity),
            nodes: snapshots,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Convert this scene back to AccessKit nodes for incremental native
    /// transport. The parent link is implicit in AccessKit children.
    pub fn to_accesskit_nodes(&self) -> BTreeMap<NodeId, Node> {
        self.nodes
            .iter()
            .map(|node| (node.identity.node(), node.accesskit_node()))
            .collect()
    }
}
