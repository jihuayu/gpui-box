//! Typed payloads, integer-coordinate drag geometry, modifiers and scoped drop
//! target/source specifications.
//!
//! Binary payloads implement a custom [`fmt::Debug`] that prints dimensions and
//! byte counts only. No payload or diagnostic ever contains a native path, a
//! `file://` URL or a `content://` URI.

use crate::format::{FormatClass, FormatDescriptor, PayloadOffer, negotiate_format};
use crate::stream::LazyRepresentation;
use gpui_files::{FileResource, ResourceId};
use gpui_system::{SurfaceId, SystemError};
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

/// Plain text is bounded to one MiB, identical to `gpui-clipboard`.
pub const MAX_TEXT_BYTES: usize = gpui_clipboard::MAX_TEXT_BYTES;
/// Rich text representations are bounded to four MiB.
pub const MAX_RICH_TEXT_BYTES: usize = 4 * 1024 * 1024;
/// Encoded or raw image representations are bounded to sixteen MiB.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Vendor/custom representations are bounded to four MiB.
pub const MAX_CUSTOM_BYTES: usize = 4 * 1024 * 1024;
/// One transfer may reference at most 256 opaque resources.
pub const MAX_TRANSFERRED_FILES: usize = 256;
/// One handoff or drag source may advertise at most 32 formats.
pub const MAX_OFFERED_FORMATS: usize = 32;
/// A drag preview label is bounded to 256 bytes.
pub const MAX_PREVIEW_LABEL_BYTES: usize = 256;
/// A portable preview asset key is bounded to 256 bytes.
pub const MAX_PREVIEW_ASSET_BYTES: usize = 256;
/// The maximum number of simultaneous consumer stream leases.
pub const MAX_ACTIVE_STREAM_LEASES: usize = 64;

/// A monotonic, service-local clipboard handoff identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HandoffId(pub u64);

/// A monotonic, service-local drag session identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DragSessionId(pub u64);

/// A monotonic, service-local drop target identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DropTargetId(pub u64);

macro_rules! display_id {
    ($type:ty, $name:literal) => {
        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!($name, "-{}"), self.0)
            }
        }
    };
}

display_id!(HandoffId, "handoff");
display_id!(DragSessionId, "drag");
display_id!(DropTargetId, "target");

/// A validated raw RGBA8 image buffer.
#[derive(Clone, PartialEq, Eq)]
pub struct RgbaImageBuffer {
    /// Pixel width; always nonzero.
    pub width: u32,
    /// Pixel height; always nonzero.
    pub height: u32,
    /// Exactly `width * height * 4` non-premultiplied RGBA bytes.
    pub bytes: Arc<[u8]>,
}

impl RgbaImageBuffer {
    /// Validate dimensions and a checked `width * height * 4` byte length.
    pub fn new(width: u32, height: u32, bytes: impl Into<Arc<[u8]>>) -> Result<Self, SystemError> {
        let bytes = bytes.into();
        if width == 0 || height == 0 {
            return Err(crate::error::invalid(
                "RGBA image width and height must be nonzero",
            ));
        }
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| crate::error::invalid("RGBA image dimensions overflow"))?;
        if expected > MAX_IMAGE_BYTES as u64 || bytes.len() as u64 != expected {
            return Err(crate::error::invalid(
                "RGBA image byte length must equal width * height * 4 within the image bound",
            ));
        }
        Ok(Self {
            width,
            height,
            bytes,
        })
    }

    pub fn from_vec(width: u32, height: u32, bytes: Vec<u8>) -> Result<Self, SystemError> {
        Self::new(width, height, bytes)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn descriptor(&self) -> FormatDescriptor {
        FormatDescriptor::rgba8()
    }
}

impl fmt::Debug for RgbaImageBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RgbaImageBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// A validated typed payload. Bytes are never printed by `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub enum TypedPayload {
    PlainText(String),
    Html(String),
    Rtf(String),
    Markdown(String),
    Png(Vec<u8>),
    Jpeg(Vec<u8>),
    Webp(Vec<u8>),
    Rgba8(RgbaImageBuffer),
    Files(Vec<FileResource>),
    Custom {
        descriptor: FormatDescriptor,
        bytes: Vec<u8>,
    },
}

impl TypedPayload {
    pub fn plain_text(text: impl Into<String>) -> Result<Self, SystemError> {
        Self::PlainText(text.into()).validated()
    }

    pub fn html(text: impl Into<String>) -> Result<Self, SystemError> {
        Self::Html(text.into()).validated()
    }

    pub fn rtf(text: impl Into<String>) -> Result<Self, SystemError> {
        Self::Rtf(text.into()).validated()
    }

    pub fn markdown(text: impl Into<String>) -> Result<Self, SystemError> {
        Self::Markdown(text.into()).validated()
    }

    pub fn png(bytes: impl Into<Vec<u8>>) -> Result<Self, SystemError> {
        Self::Png(bytes.into()).validated()
    }

    pub fn jpeg(bytes: impl Into<Vec<u8>>) -> Result<Self, SystemError> {
        Self::Jpeg(bytes.into()).validated()
    }

    pub fn webp(bytes: impl Into<Vec<u8>>) -> Result<Self, SystemError> {
        Self::Webp(bytes.into()).validated()
    }

    pub fn rgba8(image: RgbaImageBuffer) -> Result<Self, SystemError> {
        Self::Rgba8(image).validated()
    }

    pub fn files(files: impl IntoIterator<Item = FileResource>) -> Result<Self, SystemError> {
        Self::Files(files.into_iter().collect()).validated()
    }

    pub fn custom(
        descriptor: FormatDescriptor,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, SystemError> {
        if !descriptor.is_custom() {
            return Err(crate::error::invalid(
                "Custom payloads require an application/vnd.* descriptor",
            ));
        }
        Self::Custom {
            descriptor,
            bytes: bytes.into(),
        }
        .validated()
    }

    /// The canonical descriptor for this payload.
    pub fn descriptor(&self) -> FormatDescriptor {
        match self {
            Self::PlainText(_) => FormatDescriptor::plain_text(),
            Self::Html(_) => FormatDescriptor::html(),
            Self::Rtf(_) => FormatDescriptor::rtf(),
            Self::Markdown(_) => FormatDescriptor::markdown(),
            Self::Png(_) => FormatDescriptor::png(),
            Self::Jpeg(_) => FormatDescriptor::jpeg(),
            Self::Webp(_) => FormatDescriptor::webp(),
            Self::Rgba8(_) => FormatDescriptor::rgba8(),
            Self::Files(_) => FormatDescriptor::file_resources(),
            Self::Custom { descriptor, .. } => descriptor.clone(),
        }
    }

    pub fn class(&self) -> FormatClass {
        self.descriptor().class
    }

    /// Bytes actually held, excluding resource handles.
    pub fn byte_len(&self) -> usize {
        match self {
            Self::PlainText(value)
            | Self::Html(value)
            | Self::Rtf(value)
            | Self::Markdown(value) => value.len(),
            Self::Png(bytes) | Self::Jpeg(bytes) | Self::Webp(bytes) => bytes.len(),
            Self::Rgba8(image) => image.len(),
            Self::Files(files) => files.len(),
            Self::Custom { bytes, .. } => bytes.len(),
        }
    }

    /// Text for plain or rich-text variants.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::PlainText(value)
            | Self::Html(value)
            | Self::Rtf(value)
            | Self::Markdown(value) => Some(value),
            _ => None,
        }
    }

    /// Opaque file handles for a `Files` payload.
    pub fn as_files(&self) -> Option<&[FileResource]> {
        match self {
            Self::Files(files) => Some(files),
            _ => None,
        }
    }

    pub fn file_ids(&self) -> Vec<ResourceId> {
        self.as_files()
            .map(|files| files.iter().map(FileResource::id).collect())
            .unwrap_or_default()
    }

    /// Whether this payload can satisfy a promised descriptor.
    pub fn conforms_to(&self, descriptor: &FormatDescriptor) -> bool {
        self.descriptor().matches(descriptor)
    }

    /// Validate the payload's hard bounds.
    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::PlainText(value) => validate_text_payload(value, MAX_TEXT_BYTES),
            Self::Html(value) | Self::Rtf(value) | Self::Markdown(value) => {
                validate_text_payload(value, MAX_RICH_TEXT_BYTES)
            }
            Self::Png(bytes) | Self::Jpeg(bytes) | Self::Webp(bytes) => {
                validate_binary_payload(bytes, MAX_IMAGE_BYTES)
            }
            Self::Rgba8(image) => {
                // The constructor enforces dimensions and exact length. This
                // re-check catches a future field-mutated or deserialized value.
                let expected = u64::from(image.width)
                    .checked_mul(u64::from(image.height))
                    .and_then(|pixels| pixels.checked_mul(4));
                if image.width == 0
                    || image.height == 0
                    || expected != Some(image.bytes.len() as u64)
                    || image.bytes.len() > MAX_IMAGE_BYTES
                {
                    return Err(crate::error::invalid(
                        "RGBA image dimensions and byte length are inconsistent",
                    ));
                }
                Ok(())
            }
            Self::Files(files) => {
                if files.is_empty() || files.len() > MAX_TRANSFERRED_FILES {
                    return Err(crate::error::invalid(
                        "File payloads require 1 through 256 opaque resources",
                    ));
                }
                Ok(())
            }
            Self::Custom { descriptor, bytes } => {
                if !descriptor.is_custom() {
                    return Err(crate::error::invalid(
                        "Custom payload descriptor must be application/vnd.*",
                    ));
                }
                validate_binary_payload(bytes, MAX_CUSTOM_BYTES)
            }
        }
    }

    fn validated(self) -> Result<Self, SystemError> {
        self.validate()?;
        Ok(self)
    }

    /// Release every opaque resource reachable from this payload.
    pub fn release_resources(&self) {
        if let Self::Files(files) = self {
            for file in files {
                file.release();
            }
        }
    }
}

fn validate_text_payload(value: &str, max: usize) -> Result<(), SystemError> {
    if value.len() > max {
        return Err(crate::error::invalid("Text payload exceeds its byte bound"));
    }
    if value.contains('\0') {
        return Err(crate::error::invalid(
            "Text payload must not contain a NUL character",
        ));
    }
    Ok(())
}

fn validate_binary_payload(bytes: &[u8], max: usize) -> Result<(), SystemError> {
    if bytes.is_empty() || bytes.len() > max {
        return Err(crate::error::invalid(
            "Binary payload must be non-empty and within its byte bound",
        ));
    }
    Ok(())
}

impl fmt::Debug for TypedPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlainText(value) => formatter
                .debug_struct("PlainText")
                .field("descriptor", &FormatDescriptor::plain_text())
                .field("bytes", &value.len())
                .finish(),
            Self::Html(value) => formatter
                .debug_struct("Html")
                .field("bytes", &value.len())
                .finish(),
            Self::Rtf(value) => formatter
                .debug_struct("Rtf")
                .field("bytes", &value.len())
                .finish(),
            Self::Markdown(value) => formatter
                .debug_struct("Markdown")
                .field("bytes", &value.len())
                .finish(),
            Self::Png(bytes) => formatter
                .debug_struct("Png")
                .field("bytes", &bytes.len())
                .finish(),
            Self::Jpeg(bytes) => formatter
                .debug_struct("Jpeg")
                .field("bytes", &bytes.len())
                .finish(),
            Self::Webp(bytes) => formatter
                .debug_struct("Webp")
                .field("bytes", &bytes.len())
                .finish(),
            Self::Rgba8(image) => formatter.debug_tuple("Rgba8").field(image).finish(),
            Self::Files(files) => formatter
                .debug_struct("Files")
                .field("count", &files.len())
                .field(
                    "ids",
                    &files.iter().map(FileResource::id).collect::<Vec<_>>(),
                )
                .finish(),
            Self::Custom { descriptor, bytes } => formatter
                .debug_struct("Custom")
                .field("descriptor", descriptor)
                .field("bytes", &bytes.len())
                .finish(),
        }
    }
}

/// An integer milli-pixel drag coordinate.
///
/// Integer storage makes `Eq`, `Ord` and `Hash` total: `NaN` and infinities can
/// never exist in a live drag point.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct DragPoint {
    pub x_milli: i64,
    pub y_milli: i64,
}

impl DragPoint {
    pub const fn from_milli(x_milli: i64, y_milli: i64) -> Self {
        Self { x_milli, y_milli }
    }

    /// Convert logical pixels into integer milli-pixels.
    pub fn from_logical(x: f64, y: f64) -> Result<Self, SystemError> {
        Ok(Self {
            x_milli: logical_to_milli(x)?,
            y_milli: logical_to_milli(y)?,
        })
    }

    pub fn to_logical(self) -> (f64, f64) {
        (self.x_milli as f64 / 1000.0, self.y_milli as f64 / 1000.0)
    }
}

fn logical_to_milli(value: f64) -> Result<i64, SystemError> {
    if !value.is_finite() {
        return Err(crate::error::invalid(
            "Logical drag coordinates must be finite",
        ));
    }
    let scaled = value * 1000.0;
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(crate::error::invalid(
            "Logical drag coordinate is outside the milli-pixel range",
        ));
    }
    Ok(scaled.round() as i64)
}

/// A validated logical rectangle in integer milli-pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct LogicalRect {
    pub x_milli: i64,
    pub y_milli: i64,
    pub width_milli: i64,
    pub height_milli: i64,
}

impl<'de> Deserialize<'de> for LogicalRect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            x_milli: i64,
            y_milli: i64,
            width_milli: i64,
            height_milli: i64,
        }
        let wire = Wire::deserialize(deserializer)?;
        LogicalRect::new(
            wire.x_milli,
            wire.y_milli,
            wire.width_milli,
            wire.height_milli,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl LogicalRect {
    pub fn new(
        x_milli: i64,
        y_milli: i64,
        width_milli: i64,
        height_milli: i64,
    ) -> Result<Self, SystemError> {
        if width_milli <= 0 || height_milli <= 0 {
            return Err(crate::error::invalid(
                "Logical rectangle width and height must be positive",
            ));
        }
        x_milli
            .checked_add(width_milli)
            .ok_or_else(|| crate::error::invalid("Logical rectangle x extent overflows"))?;
        y_milli
            .checked_add(height_milli)
            .ok_or_else(|| crate::error::invalid("Logical rectangle y extent overflows"))?;
        Ok(Self {
            x_milli,
            y_milli,
            width_milli,
            height_milli,
        })
    }

    pub fn from_logical(x: f64, y: f64, width: f64, height: f64) -> Result<Self, SystemError> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(crate::error::invalid(
                "Logical rectangle width and height must be finite and positive",
            ));
        }
        Self::new(
            logical_to_milli(x)?,
            logical_to_milli(y)?,
            logical_to_milli(width)?,
            logical_to_milli(height)?,
        )
    }

    pub fn right_milli(self) -> i64 {
        self.x_milli.saturating_add(self.width_milli)
    }

    pub fn bottom_milli(self) -> i64 {
        self.y_milli.saturating_add(self.height_milli)
    }

    /// Half-open hit testing: the left/top edges are included, the right/bottom
    /// edges are excluded.
    pub fn contains(self, point: DragPoint) -> bool {
        point.x_milli >= self.x_milli
            && point.x_milli < self.right_milli()
            && point.y_milli >= self.y_milli
            && point.y_milli < self.bottom_milli()
    }
}

/// A permitted drag operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragEffect {
    Copy,
    Move,
    Link,
}

impl DragEffect {
    pub const ALL: [Self; 3] = [Self::Copy, Self::Move, Self::Link];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Move => "move",
            Self::Link => "link",
        }
    }
}

/// A validated non-empty set of effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AllowedEffects(u8);

impl Default for AllowedEffects {
    fn default() -> Self {
        Self::all()
    }
}

const EFFECT_COPY: u8 = 1;
const EFFECT_MOVE: u8 = 2;
const EFFECT_LINK: u8 = 4;

impl AllowedEffects {
    pub fn new(effects: impl IntoIterator<Item = DragEffect>) -> Result<Self, SystemError> {
        let mut bits = 0u8;
        for effect in effects {
            bits |= match effect {
                DragEffect::Copy => EFFECT_COPY,
                DragEffect::Move => EFFECT_MOVE,
                DragEffect::Link => EFFECT_LINK,
            };
        }
        if bits == 0 {
            return Err(crate::error::invalid(
                "At least one drag effect must be allowed",
            ));
        }
        Ok(Self(bits))
    }

    pub const fn all() -> Self {
        Self(EFFECT_COPY | EFFECT_MOVE | EFFECT_LINK)
    }

    pub const fn copy() -> Self {
        Self(EFFECT_COPY)
    }

    pub const fn move_effect() -> Self {
        Self(EFFECT_MOVE)
    }

    pub const fn link() -> Self {
        Self(EFFECT_LINK)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, effect: DragEffect) -> bool {
        self.0 & effect_bit(effect) != 0
    }

    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub fn effects(self) -> Vec<DragEffect> {
        DragEffect::ALL
            .into_iter()
            .filter(|effect| self.contains(*effect))
            .collect()
    }

    pub fn len(self) -> usize {
        self.effects().len()
    }

    /// Deterministic modifier-assisted choice within this set.
    ///
    /// `ctrl` prefers copy, `alt` link, `shift` move; the default preference is
    /// copy, then move, then link.
    pub fn choose(self, modifiers: DragModifiers) -> Option<DragEffect> {
        if modifiers.ctrl && self.contains(DragEffect::Copy) {
            return Some(DragEffect::Copy);
        }
        if modifiers.alt && self.contains(DragEffect::Link) {
            return Some(DragEffect::Link);
        }
        if modifiers.shift && self.contains(DragEffect::Move) {
            return Some(DragEffect::Move);
        }
        DragEffect::ALL
            .into_iter()
            .find(|effect| self.contains(*effect))
    }
}

const fn effect_bit(effect: DragEffect) -> u8 {
    match effect {
        DragEffect::Copy => EFFECT_COPY,
        DragEffect::Move => EFFECT_MOVE,
        DragEffect::Link => EFFECT_LINK,
    }
}

/// Keyboard modifiers observed during a drag callback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DragModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl DragModifiers {
    pub const fn new(shift: bool, ctrl: bool, alt: bool, meta: bool) -> Self {
        Self {
            shift,
            ctrl,
            alt,
            meta,
        }
    }

    pub const fn any(self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }
}

/// A portable drag preview visual.
#[derive(Clone, PartialEq, Eq)]
pub enum DragPreviewVisual {
    Rgba(RgbaImageBuffer),
    /// A relative, portable asset key. Absolute paths, drive letters,
    /// backslashes and parent segments are rejected.
    AssetKey(String),
}

impl DragPreviewVisual {
    pub fn rgba(image: RgbaImageBuffer) -> Self {
        Self::Rgba(image)
    }

    pub fn asset_key(key: impl Into<String>) -> Result<Self, SystemError> {
        let key = key.into();
        validate_asset_key(&key)?;
        Ok(Self::AssetKey(key))
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::Rgba(image) => {
                let expected = u64::from(image.width)
                    .checked_mul(u64::from(image.height))
                    .and_then(|pixels| pixels.checked_mul(4));
                if image.width == 0
                    || image.height == 0
                    || expected != Some(image.bytes.len() as u64)
                    || image.bytes.len() > MAX_IMAGE_BYTES
                {
                    return Err(crate::error::invalid(
                        "Drag preview RGBA image is inconsistent",
                    ));
                }
                Ok(())
            }
            Self::AssetKey(key) => validate_asset_key(key),
        }
    }
}

impl fmt::Debug for DragPreviewVisual {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rgba(image) => formatter.debug_tuple("Rgba").field(image).finish(),
            Self::AssetKey(key) => formatter.debug_tuple("AssetKey").field(key).finish(),
        }
    }
}

fn validate_asset_key(value: &str) -> Result<(), SystemError> {
    if value.is_empty() || value.len() > MAX_PREVIEW_ASSET_BYTES {
        return Err(crate::error::invalid(
            "Preview asset key must be 1 through 256 bytes",
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() || character == '\0')
    {
        return Err(crate::error::invalid(
            "Preview asset key must not contain control or NUL characters",
        ));
    }
    if value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(crate::error::invalid(
            "Preview asset key must be a relative portable path without drive prefixes",
        ));
    }
    for segment in value.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") {
            return Err(crate::error::invalid(
                "Preview asset key must not contain empty, dot or parent segments",
            ));
        }
    }
    Ok(())
}

/// A validated, bounded drag preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragPreview {
    pub label: String,
    pub visual: Option<DragPreviewVisual>,
}

impl DragPreview {
    pub fn new(label: impl Into<String>) -> Result<Self, SystemError> {
        let label = label.into();
        validate_preview_label(&label)?;
        Ok(Self {
            label,
            visual: None,
        })
    }

    pub fn with_visual(mut self, visual: DragPreviewVisual) -> Result<Self, SystemError> {
        visual.validate()?;
        self.visual = Some(visual);
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        validate_preview_label(&self.label)?;
        if let Some(visual) = &self.visual {
            visual.validate()?;
        }
        Ok(())
    }
}

fn validate_preview_label(value: &str) -> Result<(), SystemError> {
    if value.len() > MAX_PREVIEW_LABEL_BYTES {
        return Err(crate::error::invalid(
            "Drag preview label exceeds 256 bytes",
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() || character == '\0')
    {
        return Err(crate::error::invalid(
            "Drag preview label must not contain control or NUL characters",
        ));
    }
    Ok(())
}

/// One source representation: eager bytes or a lazily produced payload.
#[derive(Clone)]
pub enum SourceRepresentation {
    Eager(TypedPayload),
    Delayed(LazyRepresentation),
}

impl SourceRepresentation {
    pub fn descriptor(&self) -> FormatDescriptor {
        match self {
            Self::Eager(payload) => payload.descriptor(),
            Self::Delayed(representation) => representation.descriptor().clone(),
        }
    }

    pub const fn delivery(&self) -> crate::format::DeliveryMode {
        match self {
            Self::Eager(_) => crate::format::DeliveryMode::Eager,
            Self::Delayed(_) => crate::format::DeliveryMode::Delayed,
        }
    }
}

impl fmt::Debug for SourceRepresentation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eager(payload) => formatter.debug_tuple("Eager").field(payload).finish(),
            Self::Delayed(representation) => formatter
                .debug_tuple("Delayed")
                .field(representation)
                .finish(),
        }
    }
}

/// A validated drag source specification. The source scope defaults to the
/// gesture interaction's live surface and can be pinned explicitly.
#[derive(Debug, Clone)]
pub struct DragSourceSpec {
    /// `SurfaceId(0)` means "use the interaction's live surface".
    pub surface: SurfaceId,
    /// `0` means "use the interaction's live generation".
    pub generation: u64,
    pub allowed_effects: AllowedEffects,
    pub preview: DragPreview,
    pub representations: Vec<SourceRepresentation>,
}

impl DragSourceSpec {
    pub fn new(preview: DragPreview, allowed_effects: AllowedEffects) -> Self {
        Self {
            surface: SurfaceId(0),
            generation: 0,
            allowed_effects,
            preview,
            representations: Vec::new(),
        }
    }

    pub fn for_surface(mut self, surface: SurfaceId, generation: u64) -> Self {
        self.surface = surface;
        self.generation = generation;
        self
    }

    pub fn with_surface(self, surface: SurfaceId, generation: u64) -> Self {
        self.for_surface(surface, generation)
    }

    pub fn with_eager(mut self, payload: TypedPayload) -> Result<Self, SystemError> {
        payload.validate()?;
        self.representations
            .push(SourceRepresentation::Eager(payload));
        self.validate()?;
        Ok(self)
    }

    pub fn with_delayed(mut self, representation: LazyRepresentation) -> Result<Self, SystemError> {
        representation.validate()?;
        self.representations
            .push(SourceRepresentation::Delayed(representation));
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        self.preview.validate()?;
        if self.allowed_effects.is_empty() {
            return Err(crate::error::invalid(
                "A drag source requires at least one allowed effect",
            ));
        }
        if self.generation != 0 && self.surface == SurfaceId(0) {
            return Err(crate::error::invalid(
                "A drag source generation requires an explicit surface",
            ));
        }
        if self.surface != SurfaceId(0) && self.generation == 0 {
            return Err(crate::error::invalid(
                "A drag source surface requires a nonzero generation",
            ));
        }
        if self.representations.is_empty() {
            return Err(crate::error::invalid(
                "A drag source requires at least one representation",
            ));
        }
        if self.representations.len() > MAX_OFFERED_FORMATS {
            return Err(crate::error::invalid(
                "A drag source advertises too many formats",
            ));
        }
        let mut descriptors = Vec::with_capacity(self.representations.len());
        for representation in &self.representations {
            let descriptor = representation.descriptor();
            if descriptors.iter().any(|existing| existing == &descriptor) {
                return Err(crate::error::invalid(
                    "A drag source must not advertise duplicate formats",
                ));
            }
            descriptors.push(descriptor);
            match representation {
                SourceRepresentation::Eager(payload) => payload.validate()?,
                SourceRepresentation::Delayed(representation) => representation.validate()?,
            }
        }
        Ok(())
    }

    pub fn offers(&self) -> Vec<PayloadOffer> {
        self.representations
            .iter()
            .map(|representation| {
                let mut offer =
                    PayloadOffer::new(representation.descriptor(), representation.delivery());
                if let SourceRepresentation::Eager(payload) = representation {
                    offer = offer.with_estimated_bytes(payload.byte_len());
                }
                offer
            })
            .collect()
    }

    pub fn has_delayed(&self) -> bool {
        self.representations
            .iter()
            .any(|representation| matches!(representation, SourceRepresentation::Delayed(_)))
    }

    pub fn delayed_formats(&self) -> Vec<FormatDescriptor> {
        self.representations
            .iter()
            .filter_map(|representation| match representation {
                SourceRepresentation::Delayed(delayed) => Some(delayed.descriptor().clone()),
                SourceRepresentation::Eager(_) => None,
            })
            .collect()
    }

    pub fn eager_payloads(&self) -> Vec<TypedPayload> {
        self.representations
            .iter()
            .filter_map(|representation| match representation {
                SourceRepresentation::Eager(payload) => Some(payload.clone()),
                SourceRepresentation::Delayed(_) => None,
            })
            .collect()
    }

    pub fn source_scope(&self) -> Option<(SurfaceId, u64)> {
        (self.surface != SurfaceId(0)).then_some((self.surface, self.generation))
    }

    /// Release every producer-promised resource.
    pub fn release_resources(&self) {
        for representation in &self.representations {
            if let SourceRepresentation::Delayed(delayed) = representation {
                delayed.release_promised_files();
            }
            if let SourceRepresentation::Eager(payload) = representation {
                payload.release_resources();
            }
        }
    }
}

/// A scoped drop target specification.
#[derive(Clone)]
pub struct DropTargetSpec {
    pub surface: SurfaceId,
    pub generation: u64,
    pub rect: LogicalRect,
    pub accepted_formats: Vec<FormatDescriptor>,
    pub allowed_effects: AllowedEffects,
    pub z_index: i32,
    pub evaluator: Option<DropEvaluator>,
}

impl DropTargetSpec {
    pub fn new(
        surface: SurfaceId,
        generation: u64,
        rect: LogicalRect,
        accepted_formats: impl IntoIterator<Item = FormatDescriptor>,
    ) -> Result<Self, SystemError> {
        let spec = Self {
            surface,
            generation,
            rect,
            accepted_formats: accepted_formats.into_iter().collect(),
            allowed_effects: AllowedEffects::all(),
            z_index: 0,
            evaluator: None,
        };
        spec.validate()?;
        Ok(spec)
    }

    pub fn for_rect(
        surface: SurfaceId,
        generation: u64,
        rect: LogicalRect,
        accepted_formats: impl IntoIterator<Item = FormatDescriptor>,
    ) -> Result<Self, SystemError> {
        Self::new(surface, generation, rect, accepted_formats)
    }

    pub fn with_effects(mut self, effects: AllowedEffects) -> Result<Self, SystemError> {
        if effects.is_empty() {
            return Err(crate::error::invalid(
                "A drop target requires at least one allowed effect",
            ));
        }
        self.allowed_effects = effects;
        Ok(self)
    }

    pub const fn with_z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn with_evaluator(mut self, evaluator: DropEvaluator) -> Self {
        self.evaluator = Some(evaluator);
        self
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.surface == SurfaceId(0) || self.generation == 0 {
            return Err(crate::error::invalid(
                "A drop target requires a nonzero surface and generation",
            ));
        }
        if self.accepted_formats.is_empty() {
            return Err(crate::error::invalid(
                "A drop target requires at least one accepted format",
            ));
        }
        if self.allowed_effects.is_empty() {
            return Err(crate::error::invalid(
                "A drop target requires at least one allowed effect",
            ));
        }
        if self.rect.width_milli <= 0 || self.rect.height_milli <= 0 {
            return Err(crate::error::invalid(
                "A drop target rectangle must be positive",
            ));
        }
        Ok(())
    }

    pub fn accepts(&self, descriptor: &FormatDescriptor) -> bool {
        self.accepted_formats
            .iter()
            .any(|accepted| accepted.matches(descriptor))
    }

    pub fn negotiate(
        &self,
        source_offers: &[PayloadOffer],
    ) -> Option<crate::format::NegotiatedFormat> {
        negotiate_format(source_offers, &self.accepted_formats)
    }
}

impl fmt::Debug for DropTargetSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DropTargetSpec")
            .field("surface", &self.surface)
            .field("generation", &self.generation)
            .field("rect", &self.rect)
            .field("accepted_formats", &self.accepted_formats)
            .field("allowed_effects", &self.allowed_effects)
            .field("z_index", &self.z_index)
            .field("has_evaluator", &self.evaluator.is_some())
            .finish()
    }
}

/// The data passed to a custom drop evaluator outside all service locks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropEvaluationContext {
    pub session_id: DragSessionId,
    pub target_id: DropTargetId,
    pub point: DragPoint,
    pub modifiers: DragModifiers,
    pub format: FormatDescriptor,
    pub effect: DragEffect,
    pub source_effects: AllowedEffects,
    pub target_effects: AllowedEffects,
}

/// The decision returned by a custom drop evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTargetVerdict {
    Accept,
    /// Accept only if the requested effect is within both effect sets.
    AcceptWithEffect(DragEffect),
    Reject,
}

impl DropTargetVerdict {
    pub const fn is_accept(self) -> bool {
        !matches!(self, Self::Reject)
    }
}

/// A reentrancy-safe custom target evaluator.
///
/// The callback runs outside all internal service locks/borrows. It may call
/// back into the service; the service re-validates session, surface and target
/// liveness after it returns.
pub type DropEvaluator = std::rc::Rc<dyn Fn(&DropEvaluationContext) -> DropTargetVerdict>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_and_typed_payload_bounds_and_debug_privacy() {
        let bytes = vec![7u8; 16];
        let image = RgbaImageBuffer::new(2, 2, bytes.clone()).expect("valid image");
        assert_eq!(image.width, 2);
        assert_eq!(image.bytes(), bytes);
        assert!(!format!("{image:?}").contains("7, 7, 7"));
        assert!(RgbaImageBuffer::new(0, 1, vec![0; 4]).is_err());
        assert!(RgbaImageBuffer::new(2, 2, vec![0; 15]).is_err());
        assert!(RgbaImageBuffer::new(1, 1, vec![0; 5]).is_err());

        let payload = TypedPayload::plain_text("hello").expect("text");
        assert_eq!(payload.byte_len(), 5);
        assert_eq!(payload.descriptor(), FormatDescriptor::plain_text());
        assert!(payload.conforms_to(&FormatDescriptor::plain_text()));
        assert!(TypedPayload::plain_text("a\0b").is_err());
        assert!(TypedPayload::plain_text("x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        assert!(TypedPayload::png(Vec::new()).is_err());
        assert!(TypedPayload::png(vec![0u8; MAX_IMAGE_BYTES + 1]).is_err());
        assert!(TypedPayload::html("x".repeat(MAX_RICH_TEXT_BYTES + 1)).is_err());
        assert!(TypedPayload::markdown("a\0b").is_err());

        let image_payload = TypedPayload::rgba8(image).expect("rgba payload");
        let debug = format!("{image_payload:?}");
        assert!(debug.contains("width"));
        assert!(debug.contains("bytes"));
        assert!(!debug.contains("7, 7, 7"));
    }

    #[test]
    fn drag_points_and_rects_are_total_and_validated() {
        let point = DragPoint::from_logical(1.5, -2.25).expect("finite");
        assert_eq!(point, DragPoint::from_milli(1500, -2250));
        assert_eq!(point.to_logical(), (1.5, -2.25));
        assert!(DragPoint::from_logical(f64::NAN, 0.0).is_err());
        assert!(DragPoint::from_logical(0.0, f64::INFINITY).is_err());
        assert!(DragPoint::from_logical(f64::MAX, 0.0).is_err());

        let rect = LogicalRect::new(0, 0, 1000, 2000).expect("valid rect");
        assert!(rect.contains(DragPoint::from_milli(0, 0)));
        assert!(rect.contains(DragPoint::from_milli(999, 1999)));
        assert!(!rect.contains(DragPoint::from_milli(1000, 0)));
        assert!(!rect.contains(DragPoint::from_milli(0, 2000)));
        assert!(LogicalRect::new(0, 0, 0, 1).is_err());
        assert!(LogicalRect::new(i64::MAX, 0, 1, 1).is_err());
        assert!(LogicalRect::new(0, 0, -1, 1).is_err());
    }

    #[test]
    fn effects_modifiers_and_preview_validation() {
        let allowed = AllowedEffects::new([DragEffect::Copy, DragEffect::Move]).expect("effects");
        assert!(allowed.contains(DragEffect::Copy));
        assert!(!allowed.contains(DragEffect::Link));
        assert_eq!(
            allowed.choose(DragModifiers::default()),
            Some(DragEffect::Copy)
        );
        assert_eq!(
            allowed.choose(DragModifiers {
                shift: true,
                ..DragModifiers::default()
            }),
            Some(DragEffect::Move)
        );
        assert_eq!(
            allowed.choose(DragModifiers {
                alt: true,
                ..DragModifiers::default()
            }),
            Some(DragEffect::Copy)
        );
        assert!(AllowedEffects::new([]).is_err());
        assert_eq!(
            allowed.intersection(AllowedEffects::link()),
            AllowedEffects(0)
        );

        assert!(DragPreview::new("label").is_ok());
        assert!(DragPreview::new("x".repeat(MAX_PREVIEW_LABEL_BYTES + 1)).is_err());
        assert!(DragPreview::new("bad\0label").is_err());
        for bad in [
            "/absolute/preview.png",
            "C:/preview.png",
            "preview\\image.png",
            "../preview.png",
            "a//b.png",
        ] {
            assert!(
                DragPreviewVisual::asset_key(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        assert!(DragPreviewVisual::asset_key("drag/preview.png").is_ok());
    }

    #[test]
    fn source_and_target_specs_validate_duplicates_and_effects() {
        let preview = DragPreview::new("drag").expect("preview");
        let source = DragSourceSpec::new(preview, AllowedEffects::all())
            .with_eager(TypedPayload::plain_text("hello").expect("text"))
            .expect("source");
        assert_eq!(source.offers().len(), 1);
        assert!(!source.has_delayed());
        let duplicate = source
            .clone()
            .with_eager(TypedPayload::plain_text("again").expect("text"));
        assert!(duplicate.is_err());
        assert!(
            DragSourceSpec::new(
                DragPreview::new("drag").expect("preview"),
                AllowedEffects::all()
            )
            .validate()
            .is_err()
        );

        let rect = LogicalRect::new(0, 0, 100, 100).expect("rect");
        let target = DropTargetSpec::new(SurfaceId(1), 1, rect, [FormatDescriptor::plain_text()])
            .expect("target");
        assert!(target.accepts(&FormatDescriptor::plain_text()));
        assert!(!target.accepts(&FormatDescriptor::png()));
        assert!(
            DropTargetSpec::new(SurfaceId(1), 1, rect, [FormatDescriptor::plain_text()])
                .expect("target")
                .with_effects(AllowedEffects(0))
                .is_err()
        );
    }
}
