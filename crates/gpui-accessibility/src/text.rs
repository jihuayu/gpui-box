//! Directed text selections across UTF-8 bytes, Unicode scalars and UTF-16.
//!
//! AccessKit and GPUI text runs address Unicode scalar positions. Windows UIA,
//! Android `AccessibilityNodeInfo`, NSString/VoiceOver and the DOM use UTF-16
//! code units. Rust string byte offsets are UTF-8. This module is the single
//! validated conversion point: every offset is checked against its unit and
//! scalar boundaries before it can reach a native API. Invalid input returns a
//! typed error and never panics or slices a string mid-character.

use serde::{Deserialize, Serialize};
use std::fmt;

/// An offset unit used by a selection value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SelectionUnit {
    Utf8,
    Scalar,
    Utf16,
}

impl SelectionUnit {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8 bytes",
            Self::Scalar => "Unicode scalars",
            Self::Utf16 => "UTF-16 code units",
        }
    }
}

/// A UTF-8 byte-offset selection. Directed: `anchor` stays fixed while `focus`
/// moves, so `anchor > focus` is a backward selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Utf8Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Utf8Selection {
    pub const fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    pub const fn is_reversed(self) -> bool {
        self.anchor > self.focus
    }
}

/// A Unicode scalar (`char`) index selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScalarSelection {
    pub anchor: usize,
    pub focus: usize,
}

impl ScalarSelection {
    pub const fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    pub const fn is_reversed(self) -> bool {
        self.anchor > self.focus
    }
}

/// A UTF-16 code-unit selection. Required by UIA, Android, NSString/VoiceOver
/// and DOM/ARIA adapters. Offsets inside a surrogate pair are rejected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Utf16Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Utf16Selection {
    pub const fn new(anchor: usize, focus: usize) -> Self {
        Self { anchor, focus }
    }

    pub const fn collapsed(offset: usize) -> Self {
        Self::new(offset, offset)
    }

    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    pub const fn is_reversed(self) -> bool {
        self.anchor > self.focus
    }
}

/// Why a selection offset is not valid for a particular text snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSelectionError {
    Utf8OutOfRange { offset: usize, byte_len: usize },
    Utf8NotCharBoundary { offset: usize },
    Utf16OutOfRange { offset: usize, utf16_len: usize },
    Utf16NotCodeUnitBoundary { offset: usize },
    ScalarOutOfRange { offset: usize, scalar_len: usize },
    AccessKitNodeMismatch { expected: u64, found: u64 },
}

impl TextSelectionError {
    pub const fn unit(self) -> SelectionUnit {
        match self {
            Self::Utf8OutOfRange { .. } | Self::Utf8NotCharBoundary { .. } => SelectionUnit::Utf8,
            Self::Utf16OutOfRange { .. } | Self::Utf16NotCodeUnitBoundary { .. } => {
                SelectionUnit::Utf16
            }
            Self::ScalarOutOfRange { .. } | Self::AccessKitNodeMismatch { .. } => {
                SelectionUnit::Scalar
            }
        }
    }
}

impl fmt::Display for TextSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8OutOfRange { offset, byte_len } => {
                write!(f, "UTF-8 byte offset {offset} exceeds {byte_len}")
            }
            Self::Utf8NotCharBoundary { offset } => {
                write!(f, "UTF-8 byte offset {offset} is inside a scalar")
            }
            Self::Utf16OutOfRange { offset, utf16_len } => {
                write!(f, "UTF-16 offset {offset} exceeds {utf16_len}")
            }
            Self::Utf16NotCodeUnitBoundary { offset } => {
                write!(f, "UTF-16 offset {offset} splits a surrogate pair")
            }
            Self::ScalarOutOfRange { offset, scalar_len } => {
                write!(f, "scalar offset {offset} exceeds {scalar_len}")
            }
            Self::AccessKitNodeMismatch { expected, found } => write!(
                f,
                "AccessKit selection targets #{found}, expected text run #{expected}"
            ),
        }
    }
}

impl std::error::Error for TextSelectionError {}

/// Canonical directed selection stored in Unicode scalar indices.
///
/// The fields are private so every value produced by this crate is validated
/// against the text it belongs to. Use the validating constructors and unit
/// conversions rather than constructing offsets by hand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelectionSpan {
    anchor: usize,
    focus: usize,
}

impl TextSelectionSpan {
    /// Validate a scalar-index selection against `text`.
    pub fn new(text: &str, anchor: usize, focus: usize) -> Result<Self, TextSelectionError> {
        let scalar_len = text.chars().count();
        for offset in [anchor, focus] {
            if offset > scalar_len {
                return Err(TextSelectionError::ScalarOutOfRange { offset, scalar_len });
            }
        }
        Ok(Self { anchor, focus })
    }

    pub fn collapsed_at(text: &str, offset: usize) -> Result<Self, TextSelectionError> {
        Self::new(text, offset, offset)
    }

    pub fn from_scalar(text: &str, selection: ScalarSelection) -> Result<Self, TextSelectionError> {
        Self::new(text, selection.anchor, selection.focus)
    }

    pub fn from_utf8(text: &str, selection: Utf8Selection) -> Result<Self, TextSelectionError> {
        let anchor = utf8_to_scalar(text, selection.anchor)?;
        let focus = utf8_to_scalar(text, selection.focus)?;
        Self::new(text, anchor, focus)
    }

    pub fn from_utf16(text: &str, selection: Utf16Selection) -> Result<Self, TextSelectionError> {
        let anchor = utf16_to_scalar(text, selection.anchor)?;
        let focus = utf16_to_scalar(text, selection.focus)?;
        Self::new(text, anchor, focus)
    }

    /// Build a span directly from AccessKit's scalar character positions.
    pub fn from_accesskit(
        text: &str,
        selection: &accesskit::TextSelection,
    ) -> Result<Self, TextSelectionError> {
        Self::new(
            text,
            selection.anchor.character_index,
            selection.focus.character_index,
        )
    }

    /// Convert an AccessKit selection while confirming the text-run identity.
    pub fn from_accesskit_for_node(
        text: &str,
        selection: &accesskit::TextSelection,
        expected_node: accesskit::NodeId,
    ) -> Result<Self, TextSelectionError> {
        if selection.anchor.node != expected_node || selection.focus.node != expected_node {
            let found = if selection.anchor.node != expected_node {
                selection.anchor.node.0
            } else {
                selection.focus.node.0
            };
            return Err(TextSelectionError::AccessKitNodeMismatch {
                expected: expected_node.0,
                found,
            });
        }
        Self::from_accesskit(text, selection)
    }

    pub const fn anchor(self) -> usize {
        self.anchor
    }

    pub const fn focus(self) -> usize {
        self.focus
    }

    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.focus
    }

    pub const fn is_reversed(self) -> bool {
        self.anchor > self.focus
    }

    pub const fn min(self) -> usize {
        if self.anchor < self.focus {
            self.anchor
        } else {
            self.focus
        }
    }

    pub const fn max(self) -> usize {
        if self.anchor > self.focus {
            self.anchor
        } else {
            self.focus
        }
    }

    pub fn validate(self, text: &str) -> Result<(), TextSelectionError> {
        let scalar_len = text.chars().count();
        for offset in [self.anchor, self.focus] {
            if offset > scalar_len {
                return Err(TextSelectionError::ScalarOutOfRange { offset, scalar_len });
            }
        }
        Ok(())
    }

    pub fn scalar(self) -> ScalarSelection {
        ScalarSelection::new(self.anchor, self.focus)
    }

    pub fn to_utf8(self, text: &str) -> Result<Utf8Selection, TextSelectionError> {
        self.validate(text)?;
        Ok(Utf8Selection::new(
            scalar_to_utf8(text, self.anchor)?,
            scalar_to_utf8(text, self.focus)?,
        ))
    }

    pub fn to_utf16(self, text: &str) -> Result<Utf16Selection, TextSelectionError> {
        self.validate(text)?;
        Ok(Utf16Selection::new(
            scalar_to_utf16(text, self.anchor)?,
            scalar_to_utf16(text, self.focus)?,
        ))
    }

    /// Convert to AccessKit's scalar-oriented selection for a text-run node.
    pub fn to_accesskit(self, node: accesskit::NodeId) -> accesskit::TextSelection {
        let position = |character_index| accesskit::TextPosition {
            node,
            character_index,
        };
        accesskit::TextSelection {
            anchor: position(self.anchor),
            focus: position(self.focus),
        }
    }

    /// Slice the selected text without ever splitting a scalar. Callers that
    /// already validated the span may use this for adapter payloads.
    pub fn slice(self, text: &str) -> Result<&str, TextSelectionError> {
        let start = scalar_to_utf8(text, self.min())?;
        let end = scalar_to_utf8(text, self.max())?;
        Ok(&text[start..end])
    }
}

fn utf8_to_scalar(text: &str, offset: usize) -> Result<usize, TextSelectionError> {
    let byte_len = text.len();
    if offset > byte_len {
        return Err(TextSelectionError::Utf8OutOfRange { offset, byte_len });
    }
    if !text.is_char_boundary(offset) {
        return Err(TextSelectionError::Utf8NotCharBoundary { offset });
    }
    Ok(text[..offset].chars().count())
}

fn scalar_to_utf8(text: &str, offset: usize) -> Result<usize, TextSelectionError> {
    let scalar_len = text.chars().count();
    if offset > scalar_len {
        return Err(TextSelectionError::ScalarOutOfRange { offset, scalar_len });
    }
    if offset == scalar_len {
        return Ok(text.len());
    }
    text.char_indices()
        .nth(offset)
        .map(|(byte, _)| byte)
        .ok_or(TextSelectionError::ScalarOutOfRange { offset, scalar_len })
}

fn utf16_to_scalar(text: &str, offset: usize) -> Result<usize, TextSelectionError> {
    let utf16_len: usize = text.chars().map(char::len_utf16).sum();
    if offset > utf16_len {
        return Err(TextSelectionError::Utf16OutOfRange { offset, utf16_len });
    }
    let mut units = 0usize;
    for (scalar, character) in text.chars().enumerate() {
        if units == offset {
            return Ok(scalar);
        }
        let next = units + character.len_utf16();
        if offset < next {
            return Err(TextSelectionError::Utf16NotCodeUnitBoundary { offset });
        }
        units = next;
    }
    if units == offset {
        Ok(text.chars().count())
    } else {
        Err(TextSelectionError::Utf16OutOfRange { offset, utf16_len })
    }
}

fn scalar_to_utf16(text: &str, offset: usize) -> Result<usize, TextSelectionError> {
    let scalar_len = text.chars().count();
    if offset > scalar_len {
        return Err(TextSelectionError::ScalarOutOfRange { offset, scalar_len });
    }
    Ok(text
        .chars()
        .take(offset)
        .map(char::len_utf16)
        .sum::<usize>())
}
