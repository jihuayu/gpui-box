//! Keyboard chords, bindings and resolved precedence.
//!
//! Resolution order is scope specificity (focused surface/owner scope over
//! global), then explicit priority, then registration order (the later
//! registration wins an otherwise exact tie). The result is deterministic
//! across platforms and never depends on hash iteration order.

use crate::{CommandId, CommandPayload, CommandScope, ContextExpr, ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Modifier keys. `platform` is Command on macOS and the platform key
/// elsewhere; hosts map it to the native accelerator where one exists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        control: false,
        alt: false,
        shift: false,
        platform: false,
    };

    pub const CONTROL: Self = Self {
        control: true,
        alt: false,
        shift: false,
        platform: false,
    };

    pub const ALT: Self = Self {
        control: false,
        alt: true,
        shift: false,
        platform: false,
    };

    pub const SHIFT: Self = Self {
        control: false,
        alt: false,
        shift: true,
        platform: false,
    };

    pub const PLATFORM: Self = Self {
        control: false,
        alt: false,
        shift: false,
        platform: true,
    };

    pub const fn control() -> Self {
        Self {
            control: true,
            ..Self::NONE
        }
    }

    pub const fn command() -> Self {
        Self {
            platform: true,
            ..Self::NONE
        }
    }

    pub const fn shift() -> Self {
        Self {
            shift: true,
            ..Self::NONE
        }
    }

    pub const fn alt() -> Self {
        Self {
            alt: true,
            ..Self::NONE
        }
    }

    pub const fn with_control(mut self) -> Self {
        self.control = true;
        self
    }

    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn with_platform(mut self) -> Self {
        self.platform = true;
        self
    }

    pub const fn is_empty(self) -> bool {
        !self.control && !self.alt && !self.shift && !self.platform
    }
}

/// One normalized keystroke. The key is lowercased ASCII; invalid whitespace
/// and control characters are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeystrokeSpec {
    pub key: String,
    pub modifiers: Modifiers,
}

impl KeystrokeSpec {
    pub fn new(key: impl AsRef<str>, modifiers: Modifiers) -> Result<Self, SystemError> {
        let key = key.as_ref().to_ascii_lowercase();
        if key.is_empty()
            || key.len() > 32
            || !key.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Keystroke key must be 1-32 printable ASCII bytes",
            ));
        }
        Ok(Self { key, modifiers })
    }

    pub fn plain(key: impl AsRef<str>) -> Result<Self, SystemError> {
        Self::new(key, Modifiers::NONE)
    }

    pub fn with_modifiers(key: impl AsRef<str>, modifiers: Modifiers) -> Result<Self, SystemError> {
        Self::new(key, modifiers)
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    fn key_label(&self) -> String {
        let mut key = self.key.as_str();
        if key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
            return key.to_ascii_uppercase();
        }
        key = match key {
            "escape" => "Esc",
            "enter" | "return" => "Enter",
            "tab" => "Tab",
            "space" => "Space",
            "backspace" => "Backspace",
            "delete" => "Delete",
            "up" => "Up",
            "down" => "Down",
            "left" => "Left",
            "right" => "Right",
            "home" => "Home",
            "end" => "End",
            "pageup" => "Page Up",
            "pagedown" => "Page Down",
            other => other,
        };
        if key.len() > 1 {
            let mut chars = key.chars();
            if let Some(first) = chars.next() {
                let mut label = first.to_ascii_uppercase().to_string();
                label.push_str(chars.as_str());
                return label;
            }
        }
        key.to_owned()
    }
}

impl fmt::Display for KeystrokeSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.control {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.platform {
            parts.push("Cmd".to_owned());
        }
        parts.push(self.key_label());
        formatter.write_str(&parts.join("+"))
    }
}

/// A registered chord-to-command contribution.
#[derive(Debug, Clone)]
pub struct KeychordBinding {
    pub chord: KeystrokeSpec,
    pub command_id: CommandId,
    pub payload: CommandPayload,
    pub when: ContextExpr,
    pub scope: CommandScope,
    pub priority: i32,
}

impl KeychordBinding {
    pub fn new(chord: KeystrokeSpec, command: CommandId) -> Self {
        Self {
            chord,
            command_id: command,
            payload: CommandPayload::Unit,
            when: ContextExpr::Always,
            scope: CommandScope::Global,
            priority: 0,
        }
    }

    pub fn with_payload(mut self, payload: CommandPayload) -> Self {
        self.payload = payload;
        self
    }

    pub fn when(mut self, expression: ContextExpr) -> Self {
        self.when = expression;
        self
    }

    pub fn with_scope(mut self, scope: CommandScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// One winner of chord resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKeybinding {
    pub chord: KeystrokeSpec,
    pub command_id: CommandId,
    pub payload: CommandPayload,
    pub priority: i32,
    pub scope_specificity: u8,
    pub registration_order: u64,
}

impl ResolvedKeybinding {
    pub fn shortcut_hint(&self) -> String {
        self.chord.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keystrokes_normalize_and_format_deterministically() {
        let chord = KeystrokeSpec::new("S", Modifiers::command().with_shift()).expect("chord");
        assert_eq!(chord.key, "s");
        assert_eq!(chord.to_string(), "Shift+Cmd+S");
        let ctrl = KeystrokeSpec::new("enter", Modifiers::control()).expect("chord");
        assert_eq!(ctrl.to_string(), "Ctrl+Enter");
        assert!(KeystrokeSpec::new("", Modifiers::NONE).is_err());
        assert!(KeystrokeSpec::new("with space", Modifiers::NONE).is_err());
        assert!(KeystrokeSpec::new("é", Modifiers::NONE).is_err());
    }

    #[test]
    fn binding_builders_keep_reactive_policy() {
        let command = CommandId::new("file.save").expect("id");
        let chord = KeystrokeSpec::plain("s").expect("chord");
        let binding = KeychordBinding::new(chord.clone(), command.clone())
            .with_payload(CommandPayload::json(serde_json::json!({"all": true})))
            .when(ContextExpr::Always)
            .with_priority(7);
        assert_eq!(binding.chord, chord);
        assert_eq!(binding.command_id, command);
        assert_eq!(binding.priority, 7);
        assert!(!binding.payload.is_unit());
    }
}
