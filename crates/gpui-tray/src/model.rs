//! Validated status-item specifications, icon-variant selection, click actions
//! and close-to-tray residency policy.
//!
//! Every value that reaches a provider is validated first. Text fields are
//! bounded UTF-8 without control or NUL characters, asset keys are portable
//! relative identifiers, and RGBA payloads must match their declared
//! dimensions exactly.

use crate::{ErrorCode, SystemError};
use gpui_lifecycle::{Appearance, PreferenceValue, SystemPreferences};
use gpui_menus::{CommandId, CommandPayload, ContextMenu, SystemActionTarget};
use gpui_system::SurfaceId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

/// Longest accepted [`StatusItemKey`] in bytes.
pub const MAX_STATUS_ITEM_KEY_BYTES: usize = 64;
/// Longest accepted title, tooltip or accessibility label in bytes.
pub const MAX_STATUS_TEXT_BYTES: usize = 256;
/// Longest accepted asset-key identifier in bytes.
pub const MAX_ICON_ASSET_BYTES: usize = 256;
/// Largest accepted RGBA width or height.
pub const MAX_ICON_DIMENSION: u32 = 256;
/// Longest accepted mobile/browser alternative identifier in bytes.
pub const MAX_ALTERNATIVE_IDENTIFIER_BYTES: usize = 128;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// Stable identity of one status item within an application.
///
/// The key is a narrow, portable identifier: 1 to 64 ASCII bytes from
/// `[a-zA-Z0-9._-]`. Display text never doubles as an identity.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatusItemKey(String);

impl StatusItemKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !Self::is_valid(&value) {
            return Err(invalid(
                "Status item key must be 1-64 ASCII alphanumeric, dot, underscore or hyphen bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn is_valid(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= MAX_STATUS_ITEM_KEY_BYTES
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Debug for StatusItemKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StatusItemKey")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for StatusItemKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for StatusItemKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for StatusItemKey {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for StatusItemKey {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for StatusItemKey {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for StatusItemKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for StatusItemKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Monotonic, service-local status-item identity. Identities are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StatusItemId(pub u64);

/// Where a status-item lease is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrayLeaseScope {
    /// Lives for the process, independent of any surface.
    Application,
    /// Bound to one live surface generation.
    Surface { surface: SurfaceId, generation: u64 },
}

impl TrayLeaseScope {
    pub const fn application() -> Self {
        Self::Application
    }

    pub const fn surface(surface: SurfaceId, generation: u64) -> Self {
        Self::Surface {
            surface,
            generation,
        }
    }

    pub const fn surface_id(self) -> Option<SurfaceId> {
        match self {
            Self::Application => None,
            Self::Surface { surface, .. } => Some(surface),
        }
    }

    pub const fn generation(self) -> Option<u64> {
        match self {
            Self::Application => None,
            Self::Surface { generation, .. } => Some(generation),
        }
    }

    pub const fn is_application(self) -> bool {
        matches!(self, Self::Application)
    }
}

/// The role a status item plays in the application's exit/restore story.
///
/// Roles are descriptive metadata. Reachability is always proven from click
/// actions or a live-resolved menu, never from a role label alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusItemRole {
    Informational,
    Restore,
    Quit,
}

impl StatusItemRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Informational => "informational",
            Self::Restore => "restore",
            Self::Quit => "quit",
        }
    }
}

/// A match rule for the OS light/dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSchemeMatch {
    Any,
    Light,
    Dark,
}

impl ColorSchemeMatch {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// A match rule for the OS high-contrast preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighContrastMatch {
    Any,
    StandardOnly,
    HighContrastOnly,
}

impl HighContrastMatch {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::StandardOnly => "standard_only",
            Self::HighContrastOnly => "high_contrast_only",
        }
    }
}

/// Discrete DPI buckets used to select a status icon without upscaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleBucket {
    X1,
    X1_25,
    X1_5,
    X2,
    X3,
}

impl ScaleBucket {
    pub const ALL: [Self; 5] = [Self::X1, Self::X1_25, Self::X1_5, Self::X2, Self::X3];

    /// The bucket as a milli-scale factor (`X1_25` is 1250).
    pub const fn as_milli(self) -> u32 {
        match self {
            Self::X1 => 1000,
            Self::X1_25 => 1250,
            Self::X1_5 => 1500,
            Self::X2 => 2000,
            Self::X3 => 3000,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::X1 => "1x",
            Self::X1_25 => "1.25x",
            Self::X1_5 => "1.5x",
            Self::X2 => "2x",
            Self::X3 => "3x",
        }
    }
}

/// The pixel or asset payload of one icon variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum IconPayload {
    /// A portable relative asset identifier.
    AssetKey(String),
    /// Explicit premultiplied-or-straight RGBA bytes; the provider treats the
    /// bytes as an opaque image buffer.
    Rgba {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
}

impl IconPayload {
    pub fn asset_key(value: impl Into<String>) -> Result<Self, SystemError> {
        let payload = Self::AssetKey(value.into());
        payload.validate()?;
        Ok(payload)
    }

    pub fn rgba(width: u32, height: u32, bytes: Vec<u8>) -> Result<Self, SystemError> {
        let payload = Self::Rgba {
            width,
            height,
            bytes,
        };
        payload.validate()?;
        Ok(payload)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::AssetKey(value) => validate_asset_key(value),
            Self::Rgba {
                width,
                height,
                bytes,
            } => {
                if !(1..=MAX_ICON_DIMENSION).contains(width)
                    || !(1..=MAX_ICON_DIMENSION).contains(height)
                {
                    return Err(invalid("RGBA icon width and height must be within 1..=256"));
                }
                let expected = width
                    .checked_mul(*height)
                    .and_then(|pixels| pixels.checked_mul(4));
                if expected != Some(bytes.len() as u32) {
                    return Err(invalid(
                        "RGBA icon byte length must equal width * height * 4",
                    ));
                }
                Ok(())
            }
        }
    }
}

fn validate_asset_key(value: &str) -> Result<(), SystemError> {
    if value.is_empty() || value.len() > MAX_ICON_ASSET_BYTES {
        return Err(invalid(
            "Icon asset key must be 1-256 bytes and must not be empty",
        ));
    }
    if has_control(value) {
        return Err(invalid(
            "Icon asset key must not contain control or NUL characters",
        ));
    }
    if value.starts_with('/') || value.starts_with('\\') || value.contains('\\') {
        return Err(invalid(
            "Icon asset key must be a relative portable path without backslashes",
        ));
    }
    // Reject both drive-letter prefixes and any other colon. NTFS alternate
    // data streams and URI-like prefixes are not portable asset identifiers.
    if value.contains(':') {
        return Err(invalid(
            "Icon asset key must not contain a drive prefix or colon",
        ));
    }
    for segment in value.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") {
            return Err(invalid(
                "Icon asset key must not contain empty, dot or parent segments",
            ));
        }
    }
    Ok(())
}

/// One icon candidate for a theme and DPI bucket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusIconVariant {
    pub payload: IconPayload,
    pub color_scheme: ColorSchemeMatch,
    pub high_contrast: HighContrastMatch,
    pub scale: ScaleBucket,
    /// macOS monochrome template-image semantics. Other platforms may ignore it.
    pub is_template: bool,
}

impl StatusIconVariant {
    pub fn new(payload: IconPayload, color_scheme: ColorSchemeMatch, scale: ScaleBucket) -> Self {
        Self {
            payload,
            color_scheme,
            high_contrast: HighContrastMatch::Any,
            scale,
            is_template: false,
        }
    }

    pub fn with_high_contrast(mut self, high_contrast: HighContrastMatch) -> Self {
        self.high_contrast = high_contrast;
        self
    }

    pub const fn with_template(mut self, is_template: bool) -> Self {
        self.is_template = is_template;
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.payload.validate()
    }
}

/// A non-empty set of status icon variants with deterministic selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusIconSet {
    variants: Vec<StatusIconVariant>,
}

impl StatusIconSet {
    /// Build a set. At least one variant is required and every payload is
    /// validated before the set is returned.
    pub fn new(variants: impl IntoIterator<Item = StatusIconVariant>) -> Result<Self, SystemError> {
        let variants = variants.into_iter().collect::<Vec<_>>();
        if variants.is_empty() {
            return Err(invalid("A status icon set requires at least one variant"));
        }
        for variant in &variants {
            variant.validate()?;
        }
        Ok(Self { variants })
    }

    pub fn single(variant: StatusIconVariant) -> Result<Self, SystemError> {
        Self::new([variant])
    }

    pub fn variants(&self) -> &[StatusIconVariant] {
        &self.variants
    }

    pub fn len(&self) -> usize {
        self.variants.len()
    }

    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.variants.is_empty() {
            return Err(invalid("A status icon set requires at least one variant"));
        }
        for variant in &self.variants {
            variant.validate()?;
        }
        Ok(())
    }

    /// Select the best variant for the current preferences and milli-scale.
    ///
    /// The ranking is deterministic:
    /// 1. The requested high-contrast class, then `Any`, then the other class.
    /// 2. The requested light/dark scheme, then `Any`, then the other scheme.
    /// 3. The smallest bucket at or above the requested scale, otherwise the
    ///    largest available bucket below it (never upscale blurrily).
    /// 4. Declaration order.
    ///
    /// A zero milli-scale is invalid host input; it is treated as the smallest
    /// positive scale (`1`) instead of panicking or selecting nondeterministically.
    pub fn select_best(
        &self,
        preferences: &SystemPreferences,
        scale_factor_milli: u32,
    ) -> &StatusIconVariant {
        debug_assert!(!self.variants.is_empty());
        let scale = scale_factor_milli.max(1);
        let desired_contrast = match preferences.high_contrast {
            PreferenceValue::Known(true) => HighContrastMatch::HighContrastOnly,
            PreferenceValue::Known(false) => HighContrastMatch::StandardOnly,
            PreferenceValue::Unavailable => HighContrastMatch::Any,
        };
        let desired_scheme = match preferences.appearance {
            PreferenceValue::Known(Appearance::Dark) => ColorSchemeMatch::Dark,
            PreferenceValue::Known(Appearance::Light) => ColorSchemeMatch::Light,
            PreferenceValue::Unavailable => ColorSchemeMatch::Any,
        };
        self.variants
            .iter()
            .enumerate()
            .min_by_key(|(index, variant)| {
                (
                    contrast_rank(desired_contrast, variant.high_contrast),
                    scheme_rank(desired_scheme, variant.color_scheme),
                    scale_rank(scale, variant.scale.as_milli()),
                    *index,
                )
            })
            .map(|(_, variant)| variant)
            .expect("a validated icon set is never empty")
    }
}

fn contrast_rank(desired: HighContrastMatch, actual: HighContrastMatch) -> u8 {
    if actual == desired {
        return 0;
    }
    match desired {
        HighContrastMatch::Any => match actual {
            HighContrastMatch::Any => 0,
            HighContrastMatch::StandardOnly => 1,
            HighContrastMatch::HighContrastOnly => 2,
        },
        HighContrastMatch::StandardOnly | HighContrastMatch::HighContrastOnly => {
            if actual == HighContrastMatch::Any {
                1
            } else {
                2
            }
        }
    }
}

fn scheme_rank(desired: ColorSchemeMatch, actual: ColorSchemeMatch) -> u8 {
    if actual == desired {
        return 0;
    }
    match desired {
        ColorSchemeMatch::Any => match actual {
            ColorSchemeMatch::Any => 0,
            ColorSchemeMatch::Light => 1,
            ColorSchemeMatch::Dark => 2,
        },
        ColorSchemeMatch::Light | ColorSchemeMatch::Dark => {
            if actual == ColorSchemeMatch::Any {
                1
            } else {
                2
            }
        }
    }
}

fn scale_rank(requested: u32, actual: u32) -> (u8, i64) {
    if actual >= requested {
        (0, i64::from(actual - requested))
    } else {
        // Prefer the largest available bucket below the request.
        (1, i64::from(requested - actual))
    }
}

/// One status-item creation or replacement request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusItemSpec {
    pub key: StatusItemKey,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub accessibility_label: Option<String>,
    pub icon: StatusIconSet,
    pub menu: Option<ContextMenu>,
    pub click_actions: BTreeMap<TrayClickTrigger, TrayClickAction>,
    pub role: Option<StatusItemRole>,
    pub visible: bool,
}

impl StatusItemSpec {
    pub fn new(key: StatusItemKey, icon: StatusIconSet) -> Self {
        Self {
            key,
            title: None,
            tooltip: None,
            accessibility_label: None,
            icon,
            menu: None,
            click_actions: BTreeMap::new(),
            role: None,
            visible: true,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_accessibility_label(mut self, label: impl Into<String>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }

    pub fn with_icon(mut self, icon: StatusIconSet) -> Self {
        self.icon = icon;
        self
    }

    pub fn with_menu(mut self, menu: ContextMenu) -> Self {
        self.menu = Some(menu);
        self
    }

    pub fn with_click_action(mut self, trigger: TrayClickTrigger, action: TrayClickAction) -> Self {
        self.click_actions.insert(trigger, action);
        self
    }

    pub fn without_click_action(mut self, trigger: TrayClickTrigger) -> Self {
        self.click_actions.remove(&trigger);
        self
    }

    pub fn with_role(mut self, role: StatusItemRole) -> Self {
        self.role = Some(role);
        self
    }

    pub const fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Validate every field and compute the effective accessibility label.
    ///
    /// An omitted label falls back to a non-blank tooltip, then a non-blank
    /// title. If all three are absent or blank the item is rejected because it
    /// would be unlabeled to assistive technology.
    pub fn validate(&self) -> Result<ValidatedStatusItemSpec, SystemError> {
        let title = normalize_optional_text(self.title.as_deref(), "title")?;
        let tooltip = normalize_optional_text(self.tooltip.as_deref(), "tooltip")?;
        let accessibility_label = match self.accessibility_label.as_deref() {
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(invalid("An explicit accessibility label must not be blank"));
                }
                validate_text(trimmed, "accessibility label")?;
                trimmed.to_owned()
            }
            None => tooltip.clone().or_else(|| title.clone()).ok_or_else(|| {
                invalid("A status item requires an accessibility label, tooltip or title")
            })?,
        };
        self.icon.validate()?;
        for action in self.click_actions.values() {
            action.validate()?;
        }
        Ok(ValidatedStatusItemSpec {
            key: self.key.clone(),
            title,
            tooltip,
            accessibility_label,
            icon: self.icon.clone(),
            menu: self.menu.clone(),
            click_actions: self.click_actions.clone(),
            role: self.role,
            visible: self.visible,
        })
    }
}

fn normalize_optional_text(
    value: Option<&str>,
    field: &'static str,
) -> Result<Option<String>, SystemError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    validate_text(trimmed, field)?;
    Ok(Some(trimmed.to_owned()))
}

fn validate_text(value: &str, field: &'static str) -> Result<(), SystemError> {
    if value.len() > MAX_STATUS_TEXT_BYTES {
        return Err(invalid(match field {
            "title" => "Status item title exceeds 256 bytes",
            "tooltip" => "Status item tooltip exceeds 256 bytes",
            _ => "Status item accessibility label exceeds 256 bytes",
        }));
    }
    if has_control(value) {
        return Err(invalid(match field {
            "title" => "Status item title contains a control or NUL character",
            "tooltip" => "Status item tooltip contains a control or NUL character",
            _ => "Status item accessibility label contains a control or NUL character",
        }));
    }
    Ok(())
}

/// A fully validated status-item specification. Provider requests never carry
/// unvalidated text, icons, or actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedStatusItemSpec {
    pub key: StatusItemKey,
    pub title: Option<String>,
    pub tooltip: Option<String>,
    pub accessibility_label: String,
    pub icon: StatusIconSet,
    pub menu: Option<ContextMenu>,
    pub click_actions: BTreeMap<TrayClickTrigger, TrayClickAction>,
    pub role: Option<StatusItemRole>,
    pub visible: bool,
}

impl ValidatedStatusItemSpec {
    pub fn select_icon(
        &self,
        preferences: &SystemPreferences,
        scale_factor_milli: u32,
    ) -> &StatusIconVariant {
        self.icon.select_best(preferences, scale_factor_milli)
    }
}

/// The physical pointer gesture that opened or activated a status item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayClickTrigger {
    PrimaryClick,
    SecondaryClick,
    DoubleClick,
    MiddleClick,
}

impl TrayClickTrigger {
    pub const ALL: [Self; 4] = [
        Self::PrimaryClick,
        Self::SecondaryClick,
        Self::DoubleClick,
        Self::MiddleClick,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PrimaryClick => "primary_click",
            Self::SecondaryClick => "secondary_click",
            Self::DoubleClick => "double_click",
            Self::MiddleClick => "middle_click",
        }
    }
}

/// The deterministic effect of a click trigger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TrayClickAction {
    /// Present the attached menu through the tray provider.
    OpenMenu,
    /// Ask the host to restore and focus the primary surface.
    RestoreOrFocusApp,
    /// Invoke a typed command through `gpui-menus`.
    InvokeCommand {
        command_id: CommandId,
        payload: CommandPayload,
    },
    /// Dispatch a validated system action through `gpui-menus`.
    DispatchSystemAction { action_id: String },
}

impl TrayClickAction {
    pub fn invoke_command(command_id: CommandId) -> Self {
        Self::InvokeCommand {
            command_id,
            payload: CommandPayload::Unit,
        }
    }

    pub fn invoke_command_with_payload(command_id: CommandId, payload: CommandPayload) -> Self {
        Self::InvokeCommand {
            command_id,
            payload,
        }
    }

    pub fn dispatch_system_action(action_id: impl Into<String>) -> Result<Self, SystemError> {
        let action_id = action_id.into();
        SystemActionTarget::action_id(action_id.clone())?;
        Ok(Self::DispatchSystemAction { action_id })
    }

    pub fn command_id(&self) -> Option<&CommandId> {
        match self {
            Self::InvokeCommand { command_id, .. } => Some(command_id),
            Self::OpenMenu | Self::RestoreOrFocusApp | Self::DispatchSystemAction { .. } => None,
        }
    }

    pub fn action_id(&self) -> Option<&str> {
        match self {
            Self::DispatchSystemAction { action_id } => Some(action_id),
            Self::OpenMenu | Self::RestoreOrFocusApp | Self::InvokeCommand { .. } => None,
        }
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if let Self::DispatchSystemAction { action_id } = self {
            SystemActionTarget::action_id(action_id.clone())?;
        }
        Ok(())
    }
}

/// A non-native status surface for Android, iOS and the Web.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum MobileAlternativeSurface {
    /// No alternative is configured.
    #[default]
    None,
    /// An ongoing Android notification or equivalent host notification.
    OngoingNotification {
        channel_key: String,
        notification_key: String,
    },
    /// An in-app strip rendered by the application itself.
    InAppStatusStrip,
    /// A home-screen, quick-settings or launcher shortcut.
    QuickSettingsOrShortcut { shortcut_id: String },
}

impl MobileAlternativeSurface {
    pub fn ongoing_notification(
        channel_key: impl Into<String>,
        notification_key: impl Into<String>,
    ) -> Result<Self, SystemError> {
        let surface = Self::OngoingNotification {
            channel_key: channel_key.into(),
            notification_key: notification_key.into(),
        };
        surface.validate()?;
        Ok(surface)
    }

    pub fn shortcut(shortcut_id: impl Into<String>) -> Result<Self, SystemError> {
        let surface = Self::QuickSettingsOrShortcut {
            shortcut_id: shortcut_id.into(),
        };
        surface.validate()?;
        Ok(surface)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::None | Self::InAppStatusStrip => Ok(()),
            Self::OngoingNotification {
                channel_key,
                notification_key,
            } => {
                validate_alternative_identifier(channel_key, "notification channel")?;
                validate_alternative_identifier(notification_key, "notification")
            }
            Self::QuickSettingsOrShortcut { shortcut_id } => {
                validate_alternative_identifier(shortcut_id, "shortcut")
            }
        }
    }

    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Whether the alternative is meaningful on a target. The Web has no
    /// ongoing-notification or OS quick-settings surface, and iOS has no
    /// ongoing notification; configuration on those targets resolves to
    /// `UnsupportedNone` instead of pretending the surface exists.
    pub const fn is_applicable_to(&self, platform: crate::PlatformTarget) -> bool {
        use crate::PlatformTarget;
        match (platform, self) {
            (_, Self::None) => false,
            (PlatformTarget::Android, _) => true,
            (PlatformTarget::Ios, Self::OngoingNotification { .. }) => false,
            (PlatformTarget::Ios, _) => true,
            (PlatformTarget::Web, Self::InAppStatusStrip) => true,
            (PlatformTarget::Web, _) => false,
            (PlatformTarget::Windows | PlatformTarget::MacOs | PlatformTarget::Linux, _) => true,
        }
    }
}

fn validate_alternative_identifier(value: &str, field: &'static str) -> Result<(), SystemError> {
    if value.trim().is_empty() || value.len() > MAX_ALTERNATIVE_IDENTIFIER_BYTES {
        return Err(invalid(match field {
            "notification channel" => "Notification channel key must be 1-128 bytes",
            "notification" => "Notification key must be 1-128 bytes",
            _ => "Shortcut identifier must be 1-128 bytes",
        }));
    }
    if has_control(value) {
        return Err(invalid(match field {
            "notification channel" => "Notification channel key contains a control character",
            "notification" => "Notification key contains a control character",
            _ => "Shortcut identifier contains a control character",
        }));
    }
    Ok(())
}

/// The status surface the app can actually use on this host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EffectiveStatusSurface {
    NativeTray,
    MobileAlternative(MobileAlternativeSurface),
    UnsupportedNone,
}

impl EffectiveStatusSurface {
    pub const fn is_native(&self) -> bool {
        matches!(self, Self::NativeTray)
    }

    pub const fn is_unsupported(&self) -> bool {
        matches!(self, Self::UnsupportedNone)
    }
}

/// Policy for closing windows and keeping the app resident in the tray.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayResidencyPolicy {
    pub window_close_policy: WindowClosePolicy,
    pub shell_absence_fallback: ShellAbsenceFallback,
    pub quit_command_ids: BTreeSet<CommandId>,
    pub restore_command_ids: BTreeSet<CommandId>,
    pub quit_system_actions: BTreeSet<String>,
    pub restore_system_actions: BTreeSet<String>,
    pub alternative_surface: MobileAlternativeSurface,
}

impl Default for TrayResidencyPolicy {
    fn default() -> Self {
        Self {
            window_close_policy: WindowClosePolicy::CloseNormally,
            shell_absence_fallback: ShellAbsenceFallback::RestorePrimarySurface,
            quit_command_ids: BTreeSet::new(),
            restore_command_ids: BTreeSet::new(),
            quit_system_actions: BTreeSet::new(),
            restore_system_actions: BTreeSet::new(),
            alternative_surface: MobileAlternativeSurface::None,
        }
    }
}

impl TrayResidencyPolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn with_window_close_policy(mut self, policy: WindowClosePolicy) -> Self {
        self.window_close_policy = policy;
        self
    }

    pub const fn with_shell_absence_fallback(mut self, fallback: ShellAbsenceFallback) -> Self {
        self.shell_absence_fallback = fallback;
        self
    }

    pub fn with_alternative_surface(mut self, surface: MobileAlternativeSurface) -> Self {
        self.alternative_surface = surface;
        self
    }

    pub fn with_quit_command(mut self, command_id: CommandId) -> Self {
        self.quit_command_ids.insert(command_id);
        self
    }

    pub fn with_restore_command(mut self, command_id: CommandId) -> Self {
        self.restore_command_ids.insert(command_id);
        self
    }

    pub fn with_quit_system_action(
        mut self,
        action_id: impl Into<String>,
    ) -> Result<Self, SystemError> {
        self.register_quit_system_action(action_id)?;
        Ok(self)
    }

    pub fn with_restore_system_action(
        mut self,
        action_id: impl Into<String>,
    ) -> Result<Self, SystemError> {
        self.register_restore_system_action(action_id)?;
        Ok(self)
    }

    pub fn register_quit_command(&mut self, command_id: CommandId) -> bool {
        self.quit_command_ids.insert(command_id)
    }

    pub fn register_restore_command(&mut self, command_id: CommandId) -> bool {
        self.restore_command_ids.insert(command_id)
    }

    pub fn register_quit_system_action(
        &mut self,
        action_id: impl Into<String>,
    ) -> Result<(), SystemError> {
        let action_id = action_id.into();
        SystemActionTarget::action_id(action_id.clone())?;
        self.quit_system_actions.insert(action_id);
        Ok(())
    }

    pub fn register_restore_system_action(
        &mut self,
        action_id: impl Into<String>,
    ) -> Result<(), SystemError> {
        let action_id = action_id.into();
        SystemActionTarget::action_id(action_id.clone())?;
        self.restore_system_actions.insert(action_id);
        Ok(())
    }

    pub fn is_quit_command(&self, command_id: &CommandId) -> bool {
        self.quit_command_ids.contains(command_id)
    }

    pub fn is_restore_command(&self, command_id: &CommandId) -> bool {
        self.restore_command_ids.contains(command_id)
    }

    pub fn is_quit_system_action(&self, action_id: &str) -> bool {
        self.quit_system_actions.contains(action_id)
    }

    pub fn is_restore_system_action(&self, action_id: &str) -> bool {
        self.restore_system_actions.contains(action_id)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.alternative_surface.validate()
    }
}

/// What closing a window should do when it is the last visible surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowClosePolicy {
    /// Close the window and let the application quit.
    CloseNormally,
    /// Hide into tray residency only when a reachable exit/restore path exists.
    MinimizeToTrayWhenAvailable,
}

/// What to do when tray residency is requested but cannot be made safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellAbsenceFallback {
    RestorePrimarySurface,
    QuitCleanly,
}

/// Why a status item or the whole host could not satisfy residency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidencyFallbackReason {
    ShellAbsent,
    CapabilityUnavailable,
    NoHostedStatusItem,
    NoEligibleStatusItem,
    NoReachableRestoreOrQuit,
    SurfaceExpired,
    MenusUnavailable,
}

impl ResidencyFallbackReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShellAbsent => "shell_absent",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::NoHostedStatusItem => "no_hosted_status_item",
            Self::NoEligibleStatusItem => "no_eligible_status_item",
            Self::NoReachableRestoreOrQuit => "no_reachable_restore_or_quit",
            Self::SurfaceExpired => "surface_expired",
            Self::MenusUnavailable => "menus_unavailable",
        }
    }
}

/// The result of intercepting a window close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CloseInterceptionOutcome {
    /// Close the window and quit the process.
    AllowWindowCloseAndQuit,
    /// Close the window but keep the process alive for other visible surfaces.
    AllowWindowCloseKeepRunning,
    /// Hide the last window while the named status item keeps a way back.
    HideWindowToTray { item_id: StatusItemId },
    /// Refuse to hide because the shell or a tray capability is absent.
    PreventHideFallbackRestore { reason: ResidencyFallbackReason },
    /// Refuse to hide because doing so would be unreachable; quit instead.
    PreventHideFallbackQuit { reason: ResidencyFallbackReason },
}

impl CloseInterceptionOutcome {
    pub const fn is_quit(&self) -> bool {
        matches!(
            self,
            Self::AllowWindowCloseAndQuit | Self::PreventHideFallbackQuit { .. }
        )
    }
}

/// How the service must recover when hidden residency becomes unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidencyRecoveryAction {
    RestorePrimarySurface,
    RequestCleanQuit,
}

impl ResidencyRecoveryAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RestorePrimarySurface => "restore_primary_surface",
            Self::RequestCleanQuit => "request_clean_quit",
        }
    }
}

/// Why hidden residency ended unexpectedly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidencyBrokenReason {
    ShellLost,
    LastStatusItemRemoved,
    SurfaceDisposed,
    ReachabilityChanged,
    ShellRestartRecoveryFailed,
}

impl ResidencyBrokenReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ShellLost => "shell_lost",
            Self::LastStatusItemRemoved => "last_status_item_removed",
            Self::SurfaceDisposed => "surface_disposed",
            Self::ReachabilityChanged => "reachability_changed",
            Self::ShellRestartRecoveryFailed => "shell_restart_recovery_failed",
        }
    }
}

/// A stale provider callback was ignored instead of mutating native state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleCallbackReason {
    UnknownStatusItem,
    RemovedStatusItem,
    NativeTokenMismatch,
    ShellEpochMismatch,
    SurfaceExpired,
    AppQuitting,
    UiContextUnavailable,
    DisabledOrHidden,
}

impl StaleCallbackReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownStatusItem => "unknown_status_item",
            Self::RemovedStatusItem => "removed_status_item",
            Self::NativeTokenMismatch => "native_token_mismatch",
            Self::ShellEpochMismatch => "shell_epoch_mismatch",
            Self::SurfaceExpired => "surface_expired",
            Self::AppQuitting => "app_quitting",
            Self::UiContextUnavailable => "ui_context_unavailable",
            Self::DisabledOrHidden => "disabled_or_hidden",
        }
    }
}

/// The tray-specific operations in the six-platform qualification matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayOperation {
    CreateStatusItem,
    UpdateStatusItem,
    RemoveStatusItem,
    PresentStatusMenu,
    ManageCloseToTrayResidency,
}

impl TrayOperation {
    pub const ALL: [Self; 5] = [
        Self::CreateStatusItem,
        Self::UpdateStatusItem,
        Self::RemoveStatusItem,
        Self::PresentStatusMenu,
        Self::ManageCloseToTrayResidency,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateStatusItem => "create_status_item",
            Self::UpdateStatusItem => "update_status_item",
            Self::RemoveStatusItem => "remove_status_item",
            Self::PresentStatusMenu => "present_status_menu",
            Self::ManageCloseToTrayResidency => "manage_close_to_tray_residency",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::CreateStatusItem => "CreateStatusItem",
            Self::UpdateStatusItem => "UpdateStatusItem",
            Self::RemoveStatusItem => "RemoveStatusItem",
            Self::PresentStatusMenu => "PresentStatusMenu",
            Self::ManageCloseToTrayResidency => "ManageCloseToTrayResidency",
        }
    }
}

impl fmt::Display for TrayOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(
        payload: IconPayload,
        scheme: ColorSchemeMatch,
        scale: ScaleBucket,
    ) -> StatusIconVariant {
        StatusIconVariant::new(payload, scheme, scale)
    }

    #[test]
    fn key_and_asset_validation_are_narrow() {
        assert!(StatusItemKey::new("app.status-1").is_ok());
        assert!(StatusItemKey::new("").is_err());
        assert!(StatusItemKey::new("with space").is_err());
        assert!(StatusItemKey::new("a/b").is_err());
        assert!(StatusItemKey::new("a".repeat(65)).is_err());
        assert!(IconPayload::asset_key("icons/tray.png").is_ok());
        assert!(IconPayload::asset_key("").is_err());
        assert!(IconPayload::asset_key("/icons/tray.png").is_err());
        assert!(IconPayload::asset_key("C:/icons/tray.png").is_err());
        assert!(IconPayload::asset_key("icons/../tray.png").is_err());
        assert!(IconPayload::asset_key("icons\\tray.png").is_err());
    }

    #[test]
    fn rgba_payload_matches_dimensions() {
        assert!(IconPayload::rgba(1, 1, vec![0; 4]).is_ok());
        assert!(IconPayload::rgba(0, 1, vec![0; 4]).is_err());
        assert!(IconPayload::rgba(1, 1, vec![0; 3]).is_err());
        assert!(IconPayload::rgba(257, 1, vec![0; 1028]).is_err());
        assert!(IconPayload::rgba(u32::MAX, u32::MAX, vec![0; 4]).is_err());
    }

    #[test]
    fn selection_is_contrast_then_scheme_then_scale() {
        let hc = variant(
            IconPayload::asset_key("hc").expect("asset"),
            ColorSchemeMatch::Any,
            ScaleBucket::X1,
        )
        .with_high_contrast(HighContrastMatch::HighContrastOnly);
        let light = variant(
            IconPayload::asset_key("light").expect("asset"),
            ColorSchemeMatch::Light,
            ScaleBucket::X1,
        );
        let dark = variant(
            IconPayload::asset_key("dark").expect("asset"),
            ColorSchemeMatch::Dark,
            ScaleBucket::X1,
        );
        let set = StatusIconSet::new([hc.clone(), light.clone(), dark.clone()]).expect("set");
        let mut preferences = SystemPreferences {
            high_contrast: PreferenceValue::Known(true),
            appearance: PreferenceValue::Known(Appearance::Dark),
            ..SystemPreferences::default()
        };
        assert_eq!(set.select_best(&preferences, 1000), &hc);
        preferences.high_contrast = PreferenceValue::Known(false);
        assert_eq!(set.select_best(&preferences, 1000), &dark);
        preferences.appearance = PreferenceValue::Known(Appearance::Light);
        assert_eq!(set.select_best(&preferences, 1000), &light);
    }

    #[test]
    fn selection_downscales_and_handles_zero() {
        let x1 = variant(
            IconPayload::asset_key("x1").expect("asset"),
            ColorSchemeMatch::Any,
            ScaleBucket::X1,
        );
        let x2 = variant(
            IconPayload::asset_key("x2").expect("asset"),
            ColorSchemeMatch::Any,
            ScaleBucket::X2,
        );
        let x3 = variant(
            IconPayload::asset_key("x3").expect("asset"),
            ColorSchemeMatch::Any,
            ScaleBucket::X3,
        );
        let set = StatusIconSet::new([x1.clone(), x2.clone(), x3.clone()]).expect("set");
        assert_eq!(set.select_best(&SystemPreferences::default(), 0), &x1);
        assert_eq!(set.select_best(&SystemPreferences::default(), 1300), &x2);
        assert_eq!(set.select_best(&SystemPreferences::default(), 4000), &x3);
    }
}
