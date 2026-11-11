use crate::{
    preferences::{PreferenceValue, SystemPreferences, TextScale},
    restore::VersionedSnapshot,
};
use std::collections::BTreeMap;

/// Logical insets applied to a viewport edge.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeInsets {
    /// Inset from the top edge.
    pub top: f32,
    /// Inset from the right edge.
    pub right: f32,
    /// Inset from the bottom edge.
    pub bottom: f32,
    /// Inset from the left edge.
    pub left: f32,
}

impl EdgeInsets {
    /// No inset on any edge.
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// The same inset on every edge.
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Combined left and right insets.
    pub fn horizontal(self) -> f32 {
        self.left + self.right
    }

    /// Combined top and bottom insets.
    pub fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

/// Adaptive width class derived from available logical width and text scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeClass {
    /// Narrow phone-style layout.
    Compact,
    /// Mid-size layout such as a small tablet.
    Medium,
    /// Wide desktop or large tablet layout.
    Expanded,
}

/// Logical viewport geometry, safe area and keyboard occlusion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportMetrics {
    /// Logical width in device-independent units.
    pub width: f32,
    /// Logical height in device-independent units.
    pub height: f32,
    /// Physical pixels per logical unit.
    pub scale_factor: f32,
    /// System UI and cutout insets.
    pub safe_area: EdgeInsets,
    /// Keyboard occlusion measured from each edge.
    pub keyboard: EdgeInsets,
}

impl Default for ViewportMetrics {
    fn default() -> Self {
        Self {
            width: 0.0,
            height: 0.0,
            scale_factor: 1.0,
            safe_area: EdgeInsets::ZERO,
            keyboard: EdgeInsets::ZERO,
        }
    }
}

impl ViewportMetrics {
    /// Width below which the layout is [`SizeClass::Compact`].
    pub const COMPACT_MAX_WIDTH: f32 = 600.0;
    /// Width at or above which the layout is [`SizeClass::Expanded`].
    pub const EXPANDED_MIN_WIDTH: f32 = 900.0;

    /// Builds metrics, falling back to a scale factor of one when invalid.
    pub fn new(width: f32, height: f32, scale_factor: f32) -> Self {
        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        Self {
            width,
            height,
            scale_factor,
            ..Self::default()
        }
    }

    /// Replaces both inset groups.
    pub fn with_insets(mut self, safe_area: EdgeInsets, keyboard: EdgeInsets) -> Self {
        self.safe_area = safe_area;
        self.keyboard = keyboard;
        self
    }

    /// Logical width minus safe area and keyboard occlusion.
    pub fn available_width(self) -> f32 {
        let insets = self.safe_area.horizontal() + self.keyboard.horizontal();
        (self.width - insets).max(0.0)
    }

    /// Logical height minus safe area and keyboard occlusion.
    pub fn available_height(self) -> f32 {
        let insets = self.safe_area.vertical() + self.keyboard.vertical();
        (self.height - insets).max(0.0)
    }

    /// Derives the adaptive class from effective width and text scale.
    pub fn size_class(self, text_scale: TextScale) -> SizeClass {
        let effective = self.available_width() / text_scale.get();
        if effective < Self::COMPACT_MAX_WIDTH {
            SizeClass::Compact
        } else if effective < Self::EXPANDED_MIN_WIDTH {
            SizeClass::Medium
        } else {
            SizeClass::Expanded
        }
    }
}

/// Adaptive layout state retained across resize and preference changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveLayout {
    /// Current viewport geometry.
    pub metrics: ViewportMetrics,
    /// Current adaptive width class.
    pub size_class: SizeClass,
}

impl AdaptiveLayout {
    /// Builds a layout for the supplied geometry and scale.
    pub fn new(metrics: ViewportMetrics, text_scale: TextScale) -> Self {
        Self {
            metrics,
            size_class: metrics.size_class(text_scale),
        }
    }

    /// Applies new geometry and scale, reporting a class transition.
    pub fn update(&mut self, metrics: ViewportMetrics, text_scale: TextScale) -> bool {
        let size_class = metrics.size_class(text_scale);
        let changed = size_class != self.size_class;
        self.metrics = metrics;
        self.size_class = size_class;
        changed
    }
}

/// A UTF-16 or byte selection range.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SelectionRange {
    /// Selection anchor offset.
    pub anchor: u32,
    /// Selection focus offset.
    pub focus: u32,
}

impl SelectionRange {
    /// Builds a range from raw offsets.
    pub fn new(anchor: u32, focus: u32) -> Self {
        Self { anchor, focus }
    }

    /// A collapsed caret range.
    pub fn collapsed(offset: u32) -> Self {
        Self {
            anchor: offset,
            focus: offset,
        }
    }

    /// Whether the range has no extent.
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    /// The range ordered from lower to higher offset.
    pub fn normalized(self) -> (u32, u32) {
        (self.anchor.min(self.focus), self.anchor.max(self.focus))
    }

    /// The distance between the two offsets.
    pub fn span(self) -> u32 {
        let (start, end) = self.normalized();
        end - start
    }
}

/// One text field controller's restorable draft state.
///
/// The committed value is kept separate from the in-progress draft so an
/// accidental process death cannot persist uncommitted text over real data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldDraft {
    /// Stable field controller identity.
    pub field: String,
    /// Last committed value.
    pub committed: String,
    /// Uncommitted draft text.
    pub draft: String,
    /// Selection expressed in UTF-16 offsets at the protocol boundary.
    pub selection_utf16: SelectionRange,
    /// The same selection expressed in UTF-8 byte offsets.
    pub selection_bytes: SelectionRange,
    /// Whether an IME composition is active.
    pub composing: bool,
}

impl FieldDraft {
    /// Builds an empty draft for a field.
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            ..Self::default()
        }
    }

    /// Sets committed and draft text.
    pub fn with_text(mut self, committed: impl Into<String>, draft: impl Into<String>) -> Self {
        self.committed = committed.into();
        self.draft = draft.into();
        self
    }

    /// Sets both selection representations.
    pub fn with_selection(mut self, utf16: SelectionRange, bytes: SelectionRange) -> Self {
        self.selection_utf16 = utf16;
        self.selection_bytes = bytes;
        self
    }

    /// Marks whether an IME composition is active.
    pub fn with_composition(mut self, composing: bool) -> Self {
        self.composing = composing;
        self
    }

    /// Whether the draft differs from the committed value.
    pub fn has_uncommitted_text(&self) -> bool {
        self.draft != self.committed
    }
}

/// The stable identity of a route, including its parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RouteIdentity {
    /// Route name.
    pub name: String,
    /// Sorted route parameters.
    pub params: BTreeMap<String, String>,
}

impl RouteIdentity {
    /// Builds a route with no parameters.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: BTreeMap::new(),
        }
    }

    /// Adds or replaces one parameter.
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Reads one parameter.
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
    }
}

/// Identity of the focused field controller.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FocusTarget(pub String);

impl FocusTarget {
    /// Builds a focus target from a stable controller identity.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The controller identity.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Route, focus, drafts and adaptive geometry retained across reconfiguration.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneState {
    /// Stable route identity, including parameters.
    pub route: RouteIdentity,
    /// Focused field controller, if any.
    pub focus: Option<FocusTarget>,
    /// Last host preferences.
    pub preferences: SystemPreferences,
    /// Adaptive geometry.
    pub layout: AdaptiveLayout,
    drafts: BTreeMap<String, FieldDraft>,
}

impl SceneState {
    /// Builds scene state for a route and initial viewport.
    pub fn new(route: RouteIdentity, metrics: ViewportMetrics) -> Self {
        Self {
            route,
            focus: None,
            preferences: SystemPreferences::default(),
            layout: AdaptiveLayout::new(metrics, TextScale::default()),
            drafts: BTreeMap::new(),
        }
    }

    /// All field drafts.
    pub fn drafts(&self) -> &BTreeMap<String, FieldDraft> {
        &self.drafts
    }

    /// One field draft.
    pub fn draft(&self, field: &str) -> Option<&FieldDraft> {
        self.drafts.get(field)
    }

    /// Inserts or replaces a field draft.
    pub fn set_draft(&mut self, draft: FieldDraft) {
        self.drafts.insert(draft.field.clone(), draft);
    }

    /// Removes and returns a field draft.
    pub fn remove_draft(&mut self, field: &str) -> Option<FieldDraft> {
        self.drafts.remove(field)
    }

    /// Sets or clears the focused controller.
    pub fn set_focus(&mut self, focus: Option<FocusTarget>) {
        self.focus = focus;
    }

    /// The effective text scale, falling back to one when unavailable.
    pub fn text_scale(&self) -> TextScale {
        match &self.preferences.text_scale {
            PreferenceValue::Known(scale) => *scale,
            PreferenceValue::Unavailable => TextScale::default(),
        }
    }

    /// Applies preferences and reports an adaptive class transition.
    ///
    /// Route identity, focus, drafts, selection and composition are untouched.
    pub fn apply_preferences(&mut self, preferences: SystemPreferences) -> bool {
        let text_scale = match &preferences.text_scale {
            PreferenceValue::Known(scale) => *scale,
            PreferenceValue::Unavailable => TextScale::default(),
        };
        let changed = self.layout.update(self.layout.metrics, text_scale);
        self.preferences = preferences;
        changed
    }

    /// Applies viewport geometry and reports an adaptive class transition.
    pub fn apply_viewport(&mut self, metrics: ViewportMetrics) -> bool {
        let text_scale = self.text_scale();
        self.layout.update(metrics, text_scale)
    }

    /// Restores route, focus and drafts from a versioned snapshot.
    pub fn apply_snapshot(&mut self, snapshot: &VersionedSnapshot) {
        self.route = snapshot.route.clone();
        self.focus = snapshot.focus.clone();
        self.drafts = snapshot.drafts.clone();
    }
}
