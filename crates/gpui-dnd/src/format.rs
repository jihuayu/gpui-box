//! Canonical MIME/UTI format identities, typed payload bounds and deterministic
//! clipboard/drag negotiation.
//!
//! Both [`MimeType`] and [`UtiIdentifier`] validate on construction *and* on
//! deserialization. A deserializer can therefore never introduce a value that
//! [`MimeType::new`] or [`UtiIdentifier::new`] would reject.

use crate::error::invalid;
use gpui_system::SystemError;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
};
use std::{fmt, str::FromStr};

/// Maximum canonical MIME identity length.
pub const MAX_MIME_TYPE_BYTES: usize = 128;
/// Maximum canonical UTI length.
pub const MAX_UTI_BYTES: usize = 128;

/// The canonical UTF-8 plain-text MIME identity used by this crate.
pub const PLAIN_TEXT_MIME: &str = "text/plain;charset=utf-8";
/// The canonical UTF-8 plain-text UTI.
pub const PLAIN_TEXT_UTI: &str = "public.utf8-plain-text";
/// The legacy bare plain-text essence accepted during negotiation.
pub const PLAIN_TEXT_ESSENCE: &str = "text/plain";
/// The canonical HTML MIME identity.
pub const HTML_MIME: &str = "text/html";
/// The canonical HTML UTI.
pub const HTML_UTI: &str = "public.html";
/// The canonical RTF MIME identity.
pub const RTF_MIME: &str = "text/rtf";
/// The canonical RTF UTI.
pub const RTF_UTI: &str = "public.rtf";
/// The canonical Markdown MIME identity.
pub const MARKDOWN_MIME: &str = "text/markdown";
/// The canonical Markdown UTI.
pub const MARKDOWN_UTI: &str = "net.daringfireball.markdown";
/// The canonical PNG MIME identity.
pub const PNG_MIME: &str = "image/png";
/// The canonical PNG UTI.
pub const PNG_UTI: &str = "public.png";
/// The canonical JPEG MIME identity.
pub const JPEG_MIME: &str = "image/jpeg";
/// The canonical JPEG UTI.
pub const JPEG_UTI: &str = "public.jpeg";
/// The canonical WebP MIME identity.
pub const WEBP_MIME: &str = "image/webp";
/// The canonical WebP UTI.
pub const WEBP_UTI: &str = "org.webmproject.webp";
/// The canonical raw-RGBA MIME identity.
pub const RGBA8_MIME: &str = "image/x-rgba";
/// The canonical raw-RGBA UTI.
pub const RGBA8_UTI: &str = "org.gpui-box.rgba8";
/// The canonical file-resource list MIME identity.
pub const FILE_RESOURCES_MIME: &str = "text/uri-list";
/// The canonical file-resource list UTI.
pub const FILE_RESOURCES_UTI: &str = "public.file-url";

/// Split a MIME identity at unquoted semicolons. Quoted parameter values may
/// contain `;` and escaped characters.
fn split_mime_parameters(value: &str) -> Result<Vec<&str>, SystemError> {
    let bytes = value.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if quoted => escaped = true,
            b'"' => quoted = !quoted,
            b';' if !quoted => {
                parts.push(&value[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return Err(invalid("MIME quoted parameter value is not terminated"));
    }
    parts.push(&value[start..]);
    Ok(parts)
}

fn is_token_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_token(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(is_token_character)
}

/// A validated, canonical, lowercase MIME identity.
///
/// The canonical representation lowercases the primary type, subtype and
/// parameter names, sorts parameters by name and re-quotes values only when a
/// value is not a valid RFC token. Duplicate parameter names, whitespace,
/// non-ASCII bytes, control bytes and NUL are rejected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MimeType(String);

impl MimeType {
    /// Validate and canonicalize a MIME identity.
    pub fn new(value: impl AsRef<str>) -> Result<Self, SystemError> {
        Self::parse(value.as_ref())
    }

    /// Validate and canonicalize a borrowed MIME identity.
    pub fn parse(value: &str) -> Result<Self, SystemError> {
        if value.is_empty() || value.len() > MAX_MIME_TYPE_BYTES {
            return Err(invalid("MIME type must be 1 through 128 ASCII bytes"));
        }
        if !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(invalid(
                "MIME type must be ASCII without whitespace, control or NUL bytes",
            ));
        }
        let mut parts = split_mime_parameters(value)?.into_iter();
        let essence = parts
            .next()
            .expect("split always yields at least one segment");
        let (media_type, subtype) = essence.split_once('/').ok_or_else(|| {
            invalid("MIME type requires exactly one slash between type and subtype")
        })?;
        if subtype.contains('/') {
            return Err(invalid("MIME type must contain exactly one slash"));
        }
        if !is_token(media_type) || !is_token(subtype) {
            return Err(invalid("MIME type and subtype must be valid RFC tokens"));
        }

        let mut parameters: Vec<(String, String)> = Vec::new();
        for raw in parts {
            if raw.is_empty() {
                return Err(invalid("MIME parameters must not be empty"));
            }
            let (key, raw_value) = raw
                .split_once('=')
                .ok_or_else(|| invalid("MIME parameters require key=value"))?;
            if !is_token(key) {
                return Err(invalid("MIME parameter name must be a valid RFC token"));
            }
            let key = key.to_ascii_lowercase();
            if parameters.iter().any(|(existing, _)| existing == &key) {
                return Err(invalid("MIME parameter names must be unique"));
            }
            let value = canonical_parameter_value(raw_value)?;
            let value = if key == "charset" {
                value.to_ascii_lowercase()
            } else {
                value
            };
            parameters.push((key, value));
        }
        parameters.sort_by(|left, right| left.0.cmp(&right.0));

        let mut canonical = format!(
            "{}/{}",
            media_type.to_ascii_lowercase(),
            subtype.to_ascii_lowercase()
        );
        for (key, value) in parameters {
            canonical.push(';');
            canonical.push_str(&key);
            canonical.push('=');
            if is_token(&value) {
                canonical.push_str(&value);
            } else {
                canonical.push('"');
                for character in value.chars() {
                    if matches!(character, '"' | '\\') {
                        canonical.push('\\');
                    }
                    canonical.push(character);
                }
                canonical.push('"');
            }
        }
        if canonical.len() > MAX_MIME_TYPE_BYTES {
            return Err(invalid(
                "Canonical MIME type exceeds 128 ASCII bytes after quoting",
            ));
        }
        Ok(Self(canonical))
    }

    /// The canonical string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `type/subtype` prefix, without parameters.
    pub fn essence(&self) -> &str {
        self.0
            .find(';')
            .map_or(self.0.as_str(), |index| &self.0[..index])
    }

    /// The primary type (`text`, `image`, `application`, ...).
    pub fn media_type(&self) -> &str {
        self.essence()
            .split_once('/')
            .map_or(self.essence(), |(media_type, _)| media_type)
    }

    /// The subtype (`plain`, `png`, `vnd.example+json`, ...).
    pub fn subtype(&self) -> &str {
        self.essence()
            .split_once('/')
            .map_or("", |(_, subtype)| subtype)
    }

    /// Look up a parameter value by case-insensitive name.
    pub fn parameter(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        split_mime_parameters(&self.0)
            .ok()?
            .into_iter()
            .skip(1)
            .find_map(|raw| {
                let (key, value) = raw.split_once('=')?;
                if key == name {
                    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                        Some(&value[1..value.len() - 1])
                    } else {
                        Some(value)
                    }
                } else {
                    None
                }
            })
    }

    /// Whether two identities share a canonical essence or are identical.
    /// Negotiation additionally accepts a UTI identity match.
    pub fn matches_essence(&self, other: &Self) -> bool {
        self == other || self.essence() == other.essence()
    }

    /// Whether this is the plain or richly parameterized UTF-8 text identity.
    pub fn is_plain_text(&self) -> bool {
        self.essence() == PLAIN_TEXT_ESSENCE
    }
}

fn canonical_parameter_value(raw: &str) -> Result<String, SystemError> {
    if raw.is_empty() {
        return Err(invalid("MIME parameter values must not be empty"));
    }
    if raw.starts_with('"') {
        if raw.len() < 2 || !raw.ends_with('"') {
            return Err(invalid("MIME quoted parameter value is not terminated"));
        }
        let inner = &raw[1..raw.len() - 1];
        let mut value = String::with_capacity(inner.len());
        let mut escaped = false;
        for character in inner.chars() {
            if escaped {
                if character.is_ascii_control() {
                    return Err(invalid(
                        "MIME parameter values must not contain control or NUL bytes",
                    ));
                }
                value.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                return Err(invalid(
                    "MIME quoted parameter value contains an unescaped quote",
                ));
            } else if character.is_ascii_control() {
                return Err(invalid(
                    "MIME parameter values must not contain control or NUL bytes",
                ));
            } else {
                value.push(character);
            }
        }
        if escaped || value.is_empty() {
            return Err(invalid("MIME quoted parameter value is empty or malformed"));
        }
        Ok(value)
    } else {
        if !is_ascii_without_controls(raw) {
            return Err(invalid(
                "MIME parameter values must be ASCII without control or NUL bytes",
            ));
        }
        if !is_token(raw) {
            return Err(invalid(
                "Unquoted MIME parameter values may contain only RFC token characters",
            ));
        }
        Ok(raw.to_owned())
    }
}

fn is_ascii_without_controls(value: &str) -> bool {
    value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

impl fmt::Display for MimeType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for MimeType {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for MimeType {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for MimeType {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl AsRef<str> for MimeType {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for MimeType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for MimeType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MimeVisitor;
        impl Visitor<'_> for MimeVisitor {
            type Value = MimeType;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a canonical MIME type string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                MimeType::parse(value).map_err(E::custom)
            }
        }
        deserializer.deserialize_str(MimeVisitor)
    }
}

/// A validated, lowercase reverse-DNS / Apple Uniform Type Identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtiIdentifier(String);

impl UtiIdentifier {
    /// Validate and lowercase a UTI.
    pub fn new(value: impl AsRef<str>) -> Result<Self, SystemError> {
        Self::parse(value.as_ref())
    }

    /// Validate and lowercase a borrowed UTI.
    pub fn parse(value: &str) -> Result<Self, SystemError> {
        if value.is_empty() || value.len() > MAX_UTI_BYTES {
            return Err(invalid("UTI must be 1 through 128 ASCII bytes"));
        }
        if !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(invalid(
                "UTI must be ASCII without whitespace, control or NUL bytes",
            ));
        }
        let canonical = value.to_ascii_lowercase();
        if !canonical.contains('.') {
            return Err(invalid(
                "UTI must contain at least one dot-separated segment",
            ));
        }
        if canonical.starts_with('.') || canonical.ends_with('.') {
            return Err(invalid("UTI must not start or end with a dot"));
        }
        if canonical.split('.').any(str::is_empty) {
            return Err(invalid("UTI must not contain empty segments"));
        }
        if !canonical.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.')
        }) {
            return Err(invalid(
                "UTI may contain only lowercase ASCII letters, digits, dashes and dots",
            ));
        }
        Ok(Self(canonical))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UtiIdentifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for UtiIdentifier {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for UtiIdentifier {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl TryFrom<String> for UtiIdentifier {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl AsRef<str> for UtiIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for UtiIdentifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UtiIdentifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UtiVisitor;
        impl Visitor<'_> for UtiVisitor {
            type Value = UtiIdentifier;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a reverse-DNS Uniform Type Identifier")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                UtiIdentifier::parse(value).map_err(E::custom)
            }
        }
        deserializer.deserialize_str(UtiVisitor)
    }
}

/// Coarse payload family used by consumers to filter offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FormatClass {
    Text,
    RichText,
    Image,
    Files,
    Custom,
}

impl FormatClass {
    pub const ALL: [Self; 5] = [
        Self::Text,
        Self::RichText,
        Self::Image,
        Self::Files,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::RichText => "rich text",
            Self::Image => "image",
            Self::Files => "files",
            Self::Custom => "custom",
        }
    }
}

/// The standard formats understood by the coordinated contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StandardFormat {
    PlainText,
    Html,
    Rtf,
    Markdown,
    Png,
    Jpeg,
    Webp,
    Rgba8,
    FileResources,
    Custom,
}

impl StandardFormat {
    pub const ALL: [Self; 10] = [
        Self::PlainText,
        Self::Html,
        Self::Rtf,
        Self::Markdown,
        Self::Png,
        Self::Jpeg,
        Self::Webp,
        Self::Rgba8,
        Self::FileResources,
        Self::Custom,
    ];

    pub const fn class(self) -> FormatClass {
        match self {
            Self::PlainText => FormatClass::Text,
            Self::Html | Self::Rtf | Self::Markdown => FormatClass::RichText,
            Self::Png | Self::Jpeg | Self::Webp | Self::Rgba8 => FormatClass::Image,
            Self::FileResources => FormatClass::Files,
            Self::Custom => FormatClass::Custom,
        }
    }

    pub const fn is_standard(self) -> bool {
        !matches!(self, Self::Custom)
    }

    pub fn descriptor(self) -> Option<FormatDescriptor> {
        Some(match self {
            Self::PlainText => FormatDescriptor::plain_text(),
            Self::Html => FormatDescriptor::html(),
            Self::Rtf => FormatDescriptor::rtf(),
            Self::Markdown => FormatDescriptor::markdown(),
            Self::Png => FormatDescriptor::png(),
            Self::Jpeg => FormatDescriptor::jpeg(),
            Self::Webp => FormatDescriptor::webp(),
            Self::Rgba8 => FormatDescriptor::rgba8(),
            Self::FileResources => FormatDescriptor::file_resources(),
            Self::Custom => return None,
        })
    }
}

/// The canonical MIME + UTI identity of one transferable format.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatDescriptor {
    pub mime: MimeType,
    pub uti: UtiIdentifier,
    pub class: FormatClass,
    pub standard: StandardFormat,
}

impl FormatDescriptor {
    fn standard(mime: &str, uti: &str, standard: StandardFormat) -> Self {
        Self {
            mime: MimeType::parse(mime).expect("built-in MIME identities are valid"),
            uti: UtiIdentifier::parse(uti).expect("built-in UTI identities are valid"),
            class: standard.class(),
            standard,
        }
    }

    pub fn plain_text() -> Self {
        Self::standard(PLAIN_TEXT_MIME, PLAIN_TEXT_UTI, StandardFormat::PlainText)
    }

    pub fn html() -> Self {
        Self::standard(HTML_MIME, HTML_UTI, StandardFormat::Html)
    }

    pub fn rtf() -> Self {
        Self::standard(RTF_MIME, RTF_UTI, StandardFormat::Rtf)
    }

    pub fn markdown() -> Self {
        Self::standard(MARKDOWN_MIME, MARKDOWN_UTI, StandardFormat::Markdown)
    }

    pub fn png() -> Self {
        Self::standard(PNG_MIME, PNG_UTI, StandardFormat::Png)
    }

    pub fn jpeg() -> Self {
        Self::standard(JPEG_MIME, JPEG_UTI, StandardFormat::Jpeg)
    }

    pub fn webp() -> Self {
        Self::standard(WEBP_MIME, WEBP_UTI, StandardFormat::Webp)
    }

    /// Raw non-premultiplied RGBA8 pixels. `Debug`/events never expose bytes.
    pub fn rgba8() -> Self {
        Self::standard(RGBA8_MIME, RGBA8_UTI, StandardFormat::Rgba8)
    }

    /// A non-empty list of opaque `gpui-files` resources. No path or URI is
    /// ever part of this identity or its payload.
    pub fn file_resources() -> Self {
        Self::standard(
            FILE_RESOURCES_MIME,
            FILE_RESOURCES_UTI,
            StandardFormat::FileResources,
        )
    }

    /// Build a validated vendor `application/vnd.*` MIME + custom UTI identity.
    pub fn custom_vendor(mime: impl AsRef<str>, uti: impl AsRef<str>) -> Result<Self, SystemError> {
        let mime = MimeType::new(mime)?;
        if !mime.media_type().eq_ignore_ascii_case("application")
            || !mime.subtype().starts_with("vnd.")
        {
            return Err(invalid(
                "Custom formats require an application/vnd.* MIME identity",
            ));
        }
        let uti = UtiIdentifier::new(uti)?;
        Ok(Self {
            mime,
            uti,
            class: FormatClass::Custom,
            standard: StandardFormat::Custom,
        })
    }

    /// Alias for [`Self::custom_vendor`].
    pub fn custom(mime: impl AsRef<str>, uti: impl AsRef<str>) -> Result<Self, SystemError> {
        Self::custom_vendor(mime, uti)
    }

    pub fn is_custom(&self) -> bool {
        self.standard == StandardFormat::Custom
    }

    pub fn from_standard(standard: StandardFormat) -> Option<Self> {
        standard.descriptor()
    }

    /// Exact, canonical-MIME-essence or UTI equivalence used by negotiation.
    pub fn matches(&self, other: &Self) -> bool {
        self == other || self.mime.essence() == other.mime.essence() || self.uti == other.uti
    }

    pub fn label(&self) -> &'static str {
        match self.standard {
            StandardFormat::PlainText => "Plain text",
            StandardFormat::Html => "HTML",
            StandardFormat::Rtf => "RTF",
            StandardFormat::Markdown => "Markdown",
            StandardFormat::Png => "PNG",
            StandardFormat::Jpeg => "JPEG",
            StandardFormat::Webp => "WebP",
            StandardFormat::Rgba8 => "RGBA8",
            StandardFormat::FileResources => "Files",
            StandardFormat::Custom => "Custom",
        }
    }
}

impl fmt::Display for FormatDescriptor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} / {}", self.mime, self.uti)
    }
}

/// All ten canonical descriptors in declaration order.
pub fn standard_formats() -> Vec<FormatDescriptor> {
    StandardFormat::ALL
        .into_iter()
        .filter(|standard| standard.is_standard())
        .map(|standard| match standard {
            StandardFormat::PlainText => FormatDescriptor::plain_text(),
            StandardFormat::Html => FormatDescriptor::html(),
            StandardFormat::Rtf => FormatDescriptor::rtf(),
            StandardFormat::Markdown => FormatDescriptor::markdown(),
            StandardFormat::Png => FormatDescriptor::png(),
            StandardFormat::Jpeg => FormatDescriptor::jpeg(),
            StandardFormat::Webp => FormatDescriptor::webp(),
            StandardFormat::Rgba8 => FormatDescriptor::rgba8(),
            StandardFormat::FileResources => FormatDescriptor::file_resources(),
            StandardFormat::Custom => unreachable!("filtered above"),
        })
        .collect()
}

/// How a representation reaches a consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeliveryMode {
    /// The bytes are already available and transfer synchronously.
    Eager,
    /// The native system may request the bytes later from a lazy callback.
    Delayed,
}

impl DeliveryMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Eager => "eager",
            Self::Delayed => "delayed",
        }
    }
}

/// One advertised representation without its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadOffer {
    pub descriptor: FormatDescriptor,
    pub delivery: DeliveryMode,
    pub estimated_bytes: Option<usize>,
}

impl PayloadOffer {
    pub fn new(descriptor: FormatDescriptor, delivery: DeliveryMode) -> Self {
        Self {
            descriptor,
            delivery,
            estimated_bytes: None,
        }
    }

    pub const fn with_estimated_bytes(mut self, bytes: usize) -> Self {
        self.estimated_bytes = Some(bytes);
        self
    }
}

/// The deterministic negotiation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedFormat {
    pub offer_index: usize,
    pub descriptor: FormatDescriptor,
    pub delivery: DeliveryMode,
}

/// Negotiate the first target preference satisfied by any offer.
///
/// For one target preference, an eager offer wins over a delayed offer; ties
/// retain the earliest offer index. Target preferences are authoritative and
/// evaluated in declaration order.
pub fn negotiate_format(
    available_offers: &[PayloadOffer],
    target_preferences: &[FormatDescriptor],
) -> Option<NegotiatedFormat> {
    for preference in target_preferences {
        let mut best: Option<(usize, &PayloadOffer)> = None;
        for (index, offer) in available_offers.iter().enumerate() {
            if !offer.descriptor.matches(preference) {
                continue;
            }
            let replace = match best {
                None => true,
                Some((_, current)) => match (current.delivery, offer.delivery) {
                    (DeliveryMode::Delayed, DeliveryMode::Eager) => true,
                    (DeliveryMode::Eager, DeliveryMode::Delayed) => false,
                    _ => index < best.expect("checked above").0,
                },
            };
            if replace {
                best = Some((index, offer));
            }
        }
        if let Some((offer_index, offer)) = best {
            return Some(NegotiatedFormat {
                offer_index,
                descriptor: offer.descriptor.clone(),
                delivery: offer.delivery,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_canonicalizes_and_rejects_malformed_values() {
        let value = MimeType::parse("TEXT/Plain;Charset=UTF-8").expect("valid");
        assert_eq!(value.as_str(), "text/plain;charset=utf-8");
        assert_eq!(value.essence(), "text/plain");
        assert_eq!(value.media_type(), "text");
        assert_eq!(value.subtype(), "plain");
        assert_eq!(value.parameter("charset"), Some("utf-8"));
        assert!(value.matches_essence(&MimeType::parse("text/plain").expect("bare")));

        let ordered = MimeType::parse("text/plain;z=2;a=1").expect("valid");
        assert_eq!(ordered.as_str(), "text/plain;a=1;z=2");
        let quoted = MimeType::parse("text/plain;a=\"x;y\"").expect("quoted");
        assert_eq!(quoted.parameter("a"), Some("x;y"));

        for bad in [
            "",
            "text",
            "text/plain/extra",
            "/plain",
            "text/",
            "text /plain",
            "text/plain;charset",
            "text/plain;charset=utf-8;charset=latin1",
            "text/plain;bad==value",
            "text/plain;a=\"unterminated",
            "text/plain;a=\"bad\"quote\"",
            "text/plain;ü=1",
            "text/plain;a=\"\"",
            "text/plain;",
        ] {
            assert!(MimeType::parse(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(MimeType::parse(&"a".repeat(129)).is_err());
    }

    #[test]
    fn mime_deserialization_validates() {
        let value: MimeType = serde_json::from_str("\"text/plain\"").expect("valid");
        assert_eq!(value.as_str(), "text/plain");
        assert!(serde_json::from_str::<MimeType>("\"not-a-mime\"").is_err());
        let json = serde_json::to_string(&value).expect("serialize");
        assert_eq!(json, "\"text/plain\"");
    }

    #[test]
    fn uti_validation_and_deserialization() {
        let value = UtiIdentifier::parse("Public.UTF8-Plain-Text").expect("valid");
        assert_eq!(value.as_str(), "public.utf8-plain-text");
        assert_eq!(value.to_string(), "public.utf8-plain-text");
        for bad in [
            "",
            "plain",
            ".public",
            "public.",
            "public..html",
            "public html",
            "public/ht ml",
            "püblic.html",
            &"a".repeat(127),
        ] {
            assert!(
                UtiIdentifier::parse(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        let value: UtiIdentifier = serde_json::from_str("\"public.png\"").expect("valid");
        assert_eq!(value.as_str(), "public.png");
        assert!(serde_json::from_str::<UtiIdentifier>("\"public..png\"").is_err());
    }

    #[test]
    fn descriptor_table_is_complete_and_negotiation_is_deterministic() {
        assert_eq!(standard_formats().len(), 9);
        assert_eq!(FormatDescriptor::plain_text().uti.as_str(), PLAIN_TEXT_UTI);
        assert_eq!(FormatDescriptor::rgba8().class, FormatClass::Image);
        assert!(
            FormatDescriptor::custom_vendor("application/vnd.example+json", "com.example.data")
                .is_ok()
        );
        assert!(
            FormatDescriptor::custom_vendor("text/plain", "com.example.data").is_err(),
            "vendor formats require application/vnd.*"
        );

        let text = FormatDescriptor::plain_text();
        let html = FormatDescriptor::html();
        let offers = [
            PayloadOffer::new(html.clone(), DeliveryMode::Eager),
            PayloadOffer::new(text.clone(), DeliveryMode::Delayed),
            PayloadOffer::new(text.clone(), DeliveryMode::Eager),
        ];
        let negotiated = negotiate_format(&offers, &[text.clone(), html.clone()]).expect("match");
        assert_eq!(negotiated.offer_index, 2);
        assert_eq!(negotiated.delivery, DeliveryMode::Eager);
        let negotiated = negotiate_format(&offers, std::slice::from_ref(&html)).expect("match");
        assert_eq!(negotiated.delivery, DeliveryMode::Eager);
        assert_eq!(negotiate_format(&offers, &[FormatDescriptor::png()]), None);

        let bare = FormatDescriptor {
            mime: MimeType::parse("text/plain").expect("bare"),
            uti: UtiIdentifier::parse("com.example.plain").expect("uti"),
            class: FormatClass::Text,
            standard: StandardFormat::PlainText,
        };
        assert!(FormatDescriptor::plain_text().matches(&bare));
    }
}
