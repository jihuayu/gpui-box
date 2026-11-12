//! Client configuration and credential-boundary validation.
//!
//! A push client only ever receives public material: an opaque application
//! identifier, public sender/team/package identifiers and a VAPID
//! application-server **public** key. Server credentials (APNs `.p8` private
//! keys, FCM service-account JSON, WNS client secrets and VAPID private keys)
//! are rejected deterministically so they cannot be smuggled into client state.

use gpui_system::{ErrorCode, RecoveryAction, RetryAdvice, SystemError};
use std::fmt;

/// Largest accepted activation route in bytes.
pub const MAX_ACTIVATION_ROUTE_BYTES: usize = 1024;
/// Exact length of an uncompressed P-256 public key.
pub const VAPID_PUBLIC_KEY_BYTES: usize = 65;
/// Largest accepted client material blob, which is scanned but never retained
/// beyond the configuration value the host supplied.
pub const MAX_CLIENT_MATERIAL_BYTES: usize = 8 * 1024;

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn vapid_error() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::InvalidInput,
        "The VAPID application-server key must be a 65-byte uncompressed P-256 public point",
    );
    error.recovery = Some(RecoveryAction::RepairConfiguration);
    error.retry = RetryAdvice::ModifyRequest;
    error
}

fn private_key_error() -> SystemError {
    let mut error = SystemError::new(
        ErrorCode::InvalidInput,
        "Private and service-account push keys must never be configured on a client",
    );
    error.recovery = Some(RecoveryAction::RepairConfiguration);
    error.retry = RetryAdvice::ModifyRequest;
    error
}

fn identifier(value: &str, max: usize, extra: &[u8]) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.is_ascii()
        && !value.chars().any(char::is_control)
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || extra.contains(&byte)
        })
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

const PRIVATE_KEY_MARKERS: [&[u8]; 5] = [
    b"-----BEGIN PRIVATE KEY-----",
    b"-----BEGIN RSA PRIVATE KEY-----",
    b"-----BEGIN EC PRIVATE KEY-----",
    b"-----BEGIN ENCRYPTED PRIVATE KEY-----",
    b"-----BEGIN OPENSSH PRIVATE KEY-----",
];

fn looks_like_private_key(material: &[u8]) -> bool {
    PRIVATE_KEY_MARKERS
        .iter()
        .any(|marker| contains_bytes(material, marker))
}

/// Reject any material that is, or contains, a server-side push credential.
///
/// Detection is deterministic and does not attempt to parse the credential: a
/// PEM private-key header, an FCM service-account JSON body, a WNS client secret
/// or a VAPID private scalar is refused before it reaches client state.
pub fn reject_server_credentials(material: &[u8]) -> Result<(), SystemError> {
    if material.is_empty() {
        return Ok(());
    }
    if material.len() > MAX_CLIENT_MATERIAL_BYTES {
        return Err(invalid("Client push material exceeds its size bound"));
    }
    if looks_like_private_key(material) {
        return Err(private_key_error());
    }
    if let Ok(text) = std::str::from_utf8(material) {
        let lowered = text.to_ascii_lowercase();
        const MARKERS: [&str; 4] = [
            "\"private_key\"",
            "\"client_secret\"",
            "service_account",
            "-----begin",
        ];
        if MARKERS.iter().any(|marker| lowered.contains(marker)) {
            return Err(private_key_error());
        }
    }
    Ok(())
}

/// Validate a VAPID application-server **public** key supplied as raw bytes.
///
/// Accepted form: exactly 65 bytes, `0x04 || X || Y`, with non-zero X and Y
/// coordinates. A 32-byte scalar is a private key and is rejected.
pub fn validate_vapid_public_key(material: &[u8]) -> Result<(), SystemError> {
    if looks_like_private_key(material) {
        return Err(private_key_error());
    }
    if material.len() == 32 {
        return Err(private_key_error());
    }
    if material.len() != VAPID_PUBLIC_KEY_BYTES || material[0] != 0x04 {
        return Err(vapid_error());
    }
    let x = &material[1..33];
    let y = &material[33..65];
    if x.iter().all(|byte| *byte == 0) || y.iter().all(|byte| *byte == 0) {
        return Err(vapid_error());
    }
    Ok(())
}

/// Validate a VAPID public key supplied as raw 65 bytes or as base64url text.
pub fn validate_vapid_public_key_text(text: &str) -> Result<(), SystemError> {
    if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
        return Err(vapid_error());
    }
    if looks_like_private_key(text.as_bytes()) {
        return Err(private_key_error());
    }
    // A raw 65-byte key cannot be represented as ASCII text, so text input is
    // always base64url (standard base64 is tolerated on decode).
    let decoded = base64url_decode(text).ok_or_else(vapid_error)?;
    validate_vapid_public_key(&decoded)
}

/// Strict base64/base64url decoder.
///
/// Padding is optional but must be canonical when present: `=` may only appear
/// as a trailing run of one or two characters, a padded value must be a multiple
/// of four characters, a one-character trailing quantum is truncated and
/// rejected, and unused trailing bits must be zero so two different encodings
/// cannot decode to the same bytes.
fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let padding = bytes.iter().rev().take_while(|byte| **byte == b'=').count();
    if padding > 2 {
        return None;
    }
    let body = bytes.get(..bytes.len() - padding)?;
    if body.contains(&b'=') {
        return None;
    }
    if padding > 0 && !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::with_capacity(bytes.len() * 3 / 4 + 1);
    let mut accumulator: u32 = 0;
    let mut bits: u32 = 0;
    for byte in body {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        accumulator = ((accumulator << 6) | u32::from(value)) & 0xffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((accumulator >> bits) & 0xff) as u8);
        }
    }
    // A single trailing character carries six bits that cannot form a byte.
    if bits >= 6 {
        return None;
    }
    if bits > 0 && (accumulator & ((1u32 << bits) - 1)) != 0 {
        return None;
    }
    Some(output)
}

/// Validate a bounded activation deep-link or route.
///
/// Executable and inline-data schemes are always refused. An `https` route must
/// carry an authority; any other scheme must be a syntactically valid scheme.
pub fn validate_activation_route(route: &str) -> Result<(), SystemError> {
    let invalid_route = || invalid("Push activation route is empty, oversized or unsafe");
    if route.is_empty()
        || route.len() > MAX_ACTIVATION_ROUTE_BYTES
        || route.trim() != route
        || route.chars().any(char::is_control)
        || route.contains("..")
    {
        return Err(invalid_route());
    }
    if let Some((scheme, rest)) = route.split_once(':') {
        let scheme = scheme.to_ascii_lowercase();
        if matches!(
            scheme.as_str(),
            "javascript" | "data" | "file" | "vbscript" | "shell" | "blob"
        ) {
            return Err(invalid_route());
        }
        if scheme == "https" {
            if !rest.starts_with("//") || rest.len() <= 2 {
                return Err(invalid_route());
            }
        } else if scheme.is_empty()
            || scheme.len() > 32
            || !scheme.as_bytes()[0].is_ascii_alphabetic()
            || !scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        {
            return Err(invalid_route());
        }
    }
    Ok(())
}

/// Point-in-time browser capabilities. Every field is an observation, never a
/// durable grant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WebPushHostState {
    pub secure_context: bool,
    pub service_worker_registered: bool,
    pub push_manager_supported: bool,
    pub notification_api_supported: bool,
    pub vapid_public_key_configured: bool,
}

impl WebPushHostState {
    pub const fn complete() -> Self {
        Self {
            secure_context: true,
            service_worker_registered: true,
            push_manager_supported: true,
            notification_api_supported: true,
            vapid_public_key_configured: true,
        }
    }

    pub const fn is_complete(self) -> bool {
        self.secure_context
            && self.service_worker_registered
            && self.push_manager_supported
            && self.notification_api_supported
            && self.vapid_public_key_configured
    }
}

/// Public client configuration. It never holds a server-side credential.
#[derive(Clone, PartialEq, Eq)]
pub struct PushClientConfiguration {
    /// Opaque packaged application/bundle identifier.
    pub application_id: String,
    /// Public FCM sender identifier.
    pub sender_id: Option<String>,
    /// Public APNs team identifier.
    pub team_id: Option<String>,
    /// Public WNS package SID.
    pub package_sid: Option<String>,
    /// VAPID application-server public key (`0x04 || X || Y`).
    pub vapid_public_key: Option<Vec<u8>>,
    /// Optional raw client material scanned for forbidden server credentials.
    pub client_material: Option<Vec<u8>>,
    /// Browser capability observation used by the Web Push policy.
    pub web: WebPushHostState,
}

impl PushClientConfiguration {
    pub fn new(application_id: impl Into<String>) -> Result<Self, SystemError> {
        let configuration = Self {
            application_id: application_id.into(),
            sender_id: None,
            team_id: None,
            package_sid: None,
            vapid_public_key: None,
            client_material: None,
            web: WebPushHostState::default(),
        };
        configuration.validate()?;
        Ok(configuration)
    }

    pub fn with_sender_id(mut self, sender_id: impl Into<String>) -> Self {
        self.sender_id = Some(sender_id.into());
        self
    }

    pub fn with_team_id(mut self, team_id: impl Into<String>) -> Self {
        self.team_id = Some(team_id.into());
        self
    }

    pub fn with_package_sid(mut self, package_sid: impl Into<String>) -> Self {
        self.package_sid = Some(package_sid.into());
        self
    }

    pub fn with_vapid_public_key(mut self, key: impl Into<Vec<u8>>) -> Self {
        self.vapid_public_key = Some(key.into());
        self.web.vapid_public_key_configured = true;
        self
    }

    pub fn with_client_material(mut self, material: impl Into<Vec<u8>>) -> Self {
        self.client_material = Some(material.into());
        self
    }

    pub fn with_web(mut self, web: WebPushHostState) -> Self {
        self.web = web;
        self
    }

    /// Validate identifiers and reject every server-side credential shape.
    pub fn validate(&self) -> Result<(), SystemError> {
        if !identifier(&self.application_id, 128, b":/@") {
            return Err(invalid(
                "Push application identity must be bounded ASCII without control characters",
            ));
        }
        for (label, value) in [
            ("sender", self.sender_id.as_deref()),
            ("team", self.team_id.as_deref()),
            ("package", self.package_sid.as_deref()),
        ] {
            if let Some(value) = value
                && !identifier(value, 128, b":/@")
            {
                return Err(match label {
                    "sender" => invalid("Push sender identity must be bounded ASCII"),
                    "team" => invalid("Push team identity must be bounded ASCII"),
                    _ => invalid("Push package identity must be bounded ASCII"),
                });
            }
        }
        if let Some(key) = &self.vapid_public_key {
            validate_vapid_public_key(key)?;
        }
        if let Some(material) = &self.client_material {
            reject_server_credentials(material)?;
        }
        Ok(())
    }
}

impl fmt::Debug for PushClientConfiguration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushClientConfiguration")
            .field("application_id", &self.application_id)
            .field("sender_id", &self.sender_id)
            .field("team_id", &self.team_id)
            .field("package_sid", &self.package_sid)
            .field(
                "vapid_public_key_bytes",
                &self.vapid_public_key.as_ref().map(Vec::len),
            )
            .field(
                "client_material_bytes",
                &self.client_material.as_ref().map(Vec::len),
            )
            .field("web", &self.web)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_key() -> Vec<u8> {
        let mut key = vec![0x04];
        key.extend(std::iter::repeat_n(0x11u8, 32));
        key.extend(std::iter::repeat_n(0x22u8, 32));
        key
    }

    fn base64url_encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut encoded = String::new();
        for chunk in bytes.chunks(3) {
            let second = chunk.get(1).copied().unwrap_or(0);
            let third = chunk.get(2).copied().unwrap_or(0);
            let quantum = (u32::from(chunk[0]) << 16) | (u32::from(second) << 8) | u32::from(third);
            encoded.push(ALPHABET[((quantum >> 18) & 0x3f) as usize] as char);
            encoded.push(ALPHABET[((quantum >> 12) & 0x3f) as usize] as char);
            if chunk.len() > 1 {
                encoded.push(ALPHABET[((quantum >> 6) & 0x3f) as usize] as char);
            }
            if chunk.len() > 2 {
                encoded.push(ALPHABET[(quantum & 0x3f) as usize] as char);
            }
        }
        encoded
    }

    #[test]
    fn base64_padding_and_unused_bits_are_canonical() {
        // Padding is only ever a trailing run of at most two characters.
        assert!(base64url_decode("QQ==QQ==").is_none());
        assert!(base64url_decode("ab=cd").is_none());
        assert!(base64url_decode("QQ=").is_none());
        assert!(base64url_decode("QQ===").is_none());
        // A single trailing character cannot form a byte.
        assert!(base64url_decode("Q").is_none());
        assert!(base64url_decode("QUJDQ").is_none());
        // Non-zero unused padding bits are non-canonical.
        assert!(base64url_decode("QR").is_none());
        assert_eq!(base64url_decode("QQ").as_deref(), Some(b"A".as_slice()));
        assert_eq!(base64url_decode("QQ==").as_deref(), Some(b"A".as_slice()));
        assert_eq!(base64url_decode("QUJD").as_deref(), Some(b"ABC".as_slice()));

        let key = valid_key();
        let encoded = base64url_encode(&key);
        assert!(!encoded.contains('='));
        assert_eq!(base64url_decode(&encoded).expect("decode"), key);
        assert!(validate_vapid_public_key_text(&encoded).is_ok());
        assert!(validate_vapid_public_key_text(&format!("{encoded}==")).is_err());
        assert!(validate_vapid_public_key_text("not-a-key").is_err());
    }

    #[test]
    fn accepts_a_65_byte_uncompressed_public_key() {
        assert!(validate_vapid_public_key(&valid_key()).is_ok());
        assert!(validate_activation_route("https://example.test/push").is_ok());
        assert!(validate_activation_route("inbox/thread-42").is_ok());
        assert!(validate_activation_route("gpui-box://inbox/42").is_ok());
    }

    #[test]
    fn rejects_private_scalars_private_key_pem_and_unsafe_schemes() {
        assert!(validate_vapid_public_key(&[0x11u8; 32]).is_err());
        assert!(validate_vapid_public_key(b"-----BEGIN PRIVATE KEY-----\nabc").is_err());
        assert!(validate_activation_route("javascript:alert(1)").is_err());
        assert!(validate_activation_route("file:///etc/passwd").is_err());
        assert!(validate_activation_route("../escape").is_err());
        assert!(validate_activation_route("https:").is_err());
    }

    #[test]
    fn rejects_service_account_json_and_client_secrets() {
        assert!(
            reject_server_credentials(br#"{"type":"service_account","private_key":"x"}"#).is_err()
        );
        assert!(reject_server_credentials(br#"{"client_secret":"value"}"#).is_err());
        assert!(reject_server_credentials(b"plain-client-material").is_ok());
        assert!(
            PushClientConfiguration::new("app.example")
                .expect("configuration")
                .with_client_material(br#"{"private_key":"x"}"#.to_vec())
                .validate()
                .is_err()
        );
    }

    #[test]
    fn zero_coordinates_are_not_a_valid_point() {
        let mut key = valid_key();
        for byte in &mut key[1..33] {
            *byte = 0;
        }
        assert!(validate_vapid_public_key(&key).is_err());
    }
}
