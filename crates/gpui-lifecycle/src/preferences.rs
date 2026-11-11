use gpui_system::{ErrorCode, SystemError};

/// A platform preference that may be unavailable on this host.
///
/// Unavailable is explicit: callers must choose their own documented fallback
/// instead of treating a missing signal as a default OS value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferenceValue<T> {
    /// The host reported this value.
    Known(T),
    /// The host cannot report this value on the current platform.
    Unavailable,
}

impl<T> PreferenceValue<T> {
    /// Whether the host reported a value.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Known(_))
    }

    /// Borrow the reported value.
    pub fn available(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unavailable => None,
        }
    }

    /// Consume the preference and return a reported value.
    pub fn into_available(self) -> Option<T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unavailable => None,
        }
    }
}

// A derived default would require `T: Default`, which an availability value
// deliberately does not impose.
#[allow(clippy::derivable_impls)]
impl<T> Default for PreferenceValue<T> {
    fn default() -> Self {
        Self::Unavailable
    }
}

/// The OS-level light or dark appearance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// Light appearance.
    #[default]
    Light,
    /// Dark appearance.
    Dark,
}

/// A validated, finite text scale factor.
///
/// Values outside `0.25..=8.0` are rejected instead of being silently clamped,
/// so a malformed platform signal cannot collapse or explode layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextScale(f32);

impl TextScale {
    /// Smallest accepted scale.
    pub const MIN: f32 = 0.25;
    /// Largest accepted scale.
    pub const MAX: f32 = 8.0;

    /// Validates a host-reported scale factor.
    pub fn new(value: f32) -> Result<Self, SystemError> {
        if value.is_finite() && (Self::MIN..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Text scale must be finite and within 0.25..=8.0",
            ))
        }
    }

    /// The validated scale factor.
    pub fn get(self) -> f32 {
        self.0
    }
}

impl Default for TextScale {
    fn default() -> Self {
        Self(1.0)
    }
}

/// A validated BCP-47-style locale identifier.
///
/// Only ASCII alphanumeric subtags separated by `-` are accepted. The
/// identifier is retained as an owned string so it can cross a thread or be
/// persisted without borrowing native state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LocaleTag(String);

impl LocaleTag {
    /// Longest accepted identifier.
    pub const MAX_LEN: usize = 63;
    /// Longest accepted subtag.
    pub const MAX_SUBTAG_LEN: usize = 8;

    /// Validates a locale identifier.
    pub fn parse(value: &str) -> Result<Self, SystemError> {
        if Self::is_valid(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Locale tag must be a BCP-47 ASCII identifier",
            ))
        }
    }

    /// The validated identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        if value.is_empty() || value.len() > Self::MAX_LEN {
            return false;
        }
        value.split('-').all(Self::is_valid_subtag)
    }

    fn is_valid_subtag(part: &str) -> bool {
        !part.is_empty()
            && part.len() <= Self::MAX_SUBTAG_LEN
            && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }
}

/// Host-reported system preferences with explicit availability.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemPreferences {
    /// Light or dark appearance.
    pub appearance: PreferenceValue<Appearance>,
    /// Text scale factor.
    pub text_scale: PreferenceValue<TextScale>,
    /// Preferred locale.
    pub locale: PreferenceValue<LocaleTag>,
    /// Whether motion should be reduced.
    pub reduced_motion: PreferenceValue<bool>,
    /// Whether high contrast is requested.
    pub high_contrast: PreferenceValue<bool>,
}

impl SystemPreferences {
    /// The reported appearance or the supplied fallback.
    pub fn appearance_or(&self, fallback: Appearance) -> Appearance {
        match &self.appearance {
            PreferenceValue::Known(value) => *value,
            PreferenceValue::Unavailable => fallback,
        }
    }

    /// The reported text scale or the supplied fallback.
    pub fn text_scale_or(&self, fallback: TextScale) -> TextScale {
        match &self.text_scale {
            PreferenceValue::Known(value) => *value,
            PreferenceValue::Unavailable => fallback,
        }
    }

    /// The reported reduced-motion flag or the supplied fallback.
    pub fn reduced_motion_or(&self, fallback: bool) -> bool {
        match &self.reduced_motion {
            PreferenceValue::Known(value) => *value,
            PreferenceValue::Unavailable => fallback,
        }
    }

    /// The reported high-contrast flag or the supplied fallback.
    pub fn high_contrast_or(&self, fallback: bool) -> bool {
        match &self.high_contrast {
            PreferenceValue::Known(value) => *value,
            PreferenceValue::Unavailable => fallback,
        }
    }
}
