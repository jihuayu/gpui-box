//! Redirect targets, exact callback matching and OAuth error classification.

use crate::{
    AuthError, AuthOrigin, CallbackUrl, Host, MAX_REDIRECT_PATH_LEN, OriginScheme, UrlError,
};
use std::{fmt, net::Ipv4Addr};

/// Loopback address allowed for RFC 8252 native-app redirects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoopbackHost {
    Ipv4,
    Ipv6,
}

impl LoopbackHost {
    pub const ALL: [Self; 2] = [Self::Ipv4, Self::Ipv6];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ipv4 => "127.0.0.1",
            Self::Ipv6 => "[::1]",
        }
    }

    pub fn as_host(self) -> Host {
        match self {
            Self::Ipv4 => Host::Ipv4(Ipv4Addr::LOCALHOST),
            Self::Ipv6 => Host::Ipv6(std::net::Ipv6Addr::LOCALHOST),
        }
    }
}

/// A validated redirect URI owned by the host application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectTarget {
    Loopback {
        host: LoopbackHost,
        /// `None` asks the browser provider to bind an ephemeral port before
        /// the final authorize URL is built.
        port: Option<u16>,
        path: String,
    },
    CustomScheme {
        scheme: String,
        host: Option<String>,
        path: String,
    },
    UniversalLink {
        host: String,
        port: Option<u16>,
        path: String,
    },
    WebSameOrigin {
        origin: AuthOrigin,
        path: String,
    },
}

impl RedirectTarget {
    pub fn loopback(host: LoopbackHost, port: Option<u16>, path: &str) -> Result<Self, AuthError> {
        validate_path(path)?;
        if port == Some(0) {
            return Err(AuthError::InvalidInput(
                "Loopback redirect port must be non-zero",
            ));
        }
        Ok(Self::Loopback {
            host,
            port,
            path: path.to_owned(),
        })
    }

    pub fn custom_scheme(scheme: &str, host: Option<&str>, path: &str) -> Result<Self, AuthError> {
        validate_path(path)?;
        if !valid_custom_scheme(scheme) || is_reserved_scheme(scheme) {
            return Err(AuthError::InvalidInput(
                "Custom redirect scheme is reserved or malformed",
            ));
        }
        let host = match host {
            Some(host) => {
                if host.is_empty()
                    || host.len() > 253
                    || host.contains(':')
                    || !host
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
                {
                    return Err(AuthError::InvalidInput(
                        "Custom redirect host must be a bounded domain",
                    ));
                }
                Some(host.to_ascii_lowercase())
            }
            None => None,
        };
        Ok(Self::CustomScheme {
            scheme: scheme.to_owned(),
            host,
            path: path.to_owned(),
        })
    }

    pub fn universal_link(host: &str, port: Option<u16>, path: &str) -> Result<Self, AuthError> {
        validate_path(path)?;
        let host_value = Host::parse(host).map_err(url_to_auth)?;
        if host_value.as_domain().is_none() || host_value.is_loopback() {
            return Err(AuthError::InvalidInput(
                "Universal link requires a non-loopback domain",
            ));
        }
        if port == Some(0) {
            return Err(AuthError::InvalidInput(
                "Universal link port must be non-zero",
            ));
        }
        Ok(Self::UniversalLink {
            host: host_value.as_domain().unwrap_or_default().to_owned(),
            port,
            path: path.to_owned(),
        })
    }

    pub fn web_same_origin(origin: AuthOrigin, path: &str) -> Result<Self, AuthError> {
        validate_path(path)?;
        if !origin.is_secure_context() {
            return Err(AuthError::InvalidInput(
                "Web redirect requires a secure-context origin",
            ));
        }
        Ok(Self::WebSameOrigin {
            origin,
            path: path.to_owned(),
        })
    }

    /// The configured redirect URI. A loopback target without a resolved port
    /// returns an error; providers must resolve ephemeral ports first.
    pub fn redirect_uri(&self) -> Result<String, AuthError> {
        match self {
            Self::Loopback { host, port, path } => {
                let port = port.ok_or(AuthError::InvalidConfiguration(
                    "Loopback redirect requires a resolved port",
                ))?;
                Ok(format!("http://{}:{port}{path}", host.as_str()))
            }
            Self::CustomScheme { scheme, host, path } => match host {
                Some(host) => Ok(format!("{scheme}://{host}{path}")),
                None => Ok(format!("{scheme}:{path}")),
            },
            Self::UniversalLink { host, port, path } => match port {
                Some(port) => Ok(format!("https://{host}:{port}{path}")),
                None => Ok(format!("https://{host}{path}")),
            },
            Self::WebSameOrigin { origin, path } => Ok(format!("{origin}{path}")),
        }
    }

    /// Replace an unresolved loopback port with the provider's resolved port.
    pub fn resolve_loopback_port(&self, port: u16) -> Result<Self, AuthError> {
        if port == 0 {
            return Err(AuthError::InvalidInput(
                "Resolved loopback port must be non-zero",
            ));
        }
        match self {
            Self::Loopback { host, path, .. } => Self::loopback(*host, Some(port), path),
            _ => Err(AuthError::InvalidConfiguration(
                "Only loopback redirects resolve ephemeral ports",
            )),
        }
    }

    pub const fn is_loopback(&self) -> bool {
        matches!(self, Self::Loopback { .. })
    }

    pub const fn is_web(&self) -> bool {
        matches!(self, Self::WebSameOrigin { .. })
    }

    /// Exact structural match against a parsed callback. Fragment and userinfo
    /// rejection happens during parsing, before this comparison.
    pub fn matches_callback(&self, callback: &CallbackUrl) -> bool {
        match self {
            Self::Loopback { host, port, path } => {
                callback.scheme() == "http"
                    && callback.host() == Some(&host.as_host())
                    && callback.port() == *port
                    && callback.path() == path
            }
            Self::CustomScheme { scheme, host, path } => {
                callback.scheme() == scheme
                    && callback.host().map(Host::as_display).as_deref() == host.as_deref()
                    && callback.port().is_none()
                    && callback.path() == path
            }
            Self::UniversalLink { host, port, path } => {
                callback.scheme() == "https"
                    && callback.host().and_then(Host::as_domain) == Some(host.as_str())
                    && callback.port().or(Some(443)) == port.or(Some(443))
                    && callback.path() == path
            }
            Self::WebSameOrigin { origin, path } => {
                let expected_scheme = match origin.scheme() {
                    OriginScheme::Https => "https",
                    OriginScheme::Http => "http",
                };
                callback.scheme() == expected_scheme
                    && callback.host() == Some(origin.host())
                    && callback.port().or(Some(origin.effective_port()))
                        == Some(origin.effective_port())
                    && callback.path() == path
            }
        }
    }
}

fn validate_path(path: &str) -> Result<(), AuthError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > MAX_REDIRECT_PATH_LEN
        || path.contains(['?', '#'])
        || path
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(AuthError::InvalidInput(
            "Redirect path must be an exact, bounded absolute path",
        ));
    }
    Ok(())
}

fn valid_custom_scheme(scheme: &str) -> bool {
    if scheme.is_empty()
        || scheme.len() > 63
        || scheme.bytes().any(|byte| byte.is_ascii_uppercase())
        || !(scheme.contains('.') || scheme.contains('-'))
    {
        return false;
    }
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

fn is_reserved_scheme(scheme: &str) -> bool {
    matches!(
        scheme,
        "http"
            | "https"
            | "javascript"
            | "data"
            | "file"
            | "about"
            | "blob"
            | "mailto"
            | "ws"
            | "wss"
    )
}

fn url_to_auth(error: UrlError) -> AuthError {
    match error {
        UrlError::InsecureScheme | UrlError::InsecureOrigin => {
            AuthError::InvalidInput("Redirect origin must be a secure context")
        }
        UrlError::Oversized => AuthError::InvalidInput("Redirect URL exceeds the bound"),
        _ => AuthError::InvalidInput("Redirect host is malformed"),
    }
}

/// Known OAuth 2.0 authorization error codes. Unknown values are collapsed so
/// untrusted server text never reaches a public error or event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthErrorCode {
    AccessDenied,
    InvalidRequest,
    UnauthorizedClient,
    UnsupportedResponseType,
    InvalidScope,
    ServerError,
    TemporarilyUnavailable,
    InteractionRequired,
    LoginRequired,
    AccountSelectionRequired,
    ConsentRequired,
    Other,
}

impl OAuthErrorCode {
    pub fn parse(value: &str) -> Self {
        match value {
            "access_denied" => Self::AccessDenied,
            "invalid_request" => Self::InvalidRequest,
            "unauthorized_client" => Self::UnauthorizedClient,
            "unsupported_response_type" => Self::UnsupportedResponseType,
            "invalid_scope" => Self::InvalidScope,
            "server_error" => Self::ServerError,
            "temporarily_unavailable" => Self::TemporarilyUnavailable,
            "interaction_required" => Self::InteractionRequired,
            "login_required" => Self::LoginRequired,
            "account_selection_required" => Self::AccountSelectionRequired,
            "consent_required" => Self::ConsentRequired,
            _ => Self::Other,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccessDenied => "access_denied",
            Self::InvalidRequest => "invalid_request",
            Self::UnauthorizedClient => "unauthorized_client",
            Self::UnsupportedResponseType => "unsupported_response_type",
            Self::InvalidScope => "invalid_scope",
            Self::ServerError => "server_error",
            Self::TemporarilyUnavailable => "temporarily_unavailable",
            Self::InteractionRequired => "interaction_required",
            Self::LoginRequired => "login_required",
            Self::AccountSelectionRequired => "account_selection_required",
            Self::ConsentRequired => "consent_required",
            Self::Other => "other",
        }
    }

    pub const fn is_user_cancellation(self) -> bool {
        matches!(self, Self::AccessDenied)
    }
}

/// Public-safe classification of a callback query. Values are not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackQueryKind {
    AuthorizationCode,
    OAuthError(OAuthErrorCode),
    MissingParameters,
}

/// Inspect a callback's query without copying or formatting any value.
pub fn classify_callback_query(callback: &CallbackUrl) -> CallbackQueryKind {
    if let Some(error) = callback.parameter("error") {
        return CallbackQueryKind::OAuthError(OAuthErrorCode::parse(error));
    }
    if callback.has_parameter("code") && callback.has_parameter("state") {
        CallbackQueryKind::AuthorizationCode
    } else {
        CallbackQueryKind::MissingParameters
    }
}

impl fmt::Display for OAuthErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Description of one bounded pending cold-resumable redirect. It contains no
/// code or token; the PKCE verifier lives separately in `gpui-secrets`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColdRedirectDescriptor {
    pub ceremony_id: crate::CeremonyId,
    pub redirect: RedirectTarget,
    pub state: crate::OAuthState,
    pub nonce: Option<crate::OidcNonce>,
    pub scopes: Vec<crate::AuthScope>,
    pub presentation: crate::PresentationContext,
    pub remaining_ttl: std::time::Duration,
    pub expected_account: Option<crate::AccountId>,
    pub provider_id: crate::ProviderId,
}

/// Public-safe disposition of one captured redirect URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectCaptureOutcome {
    /// The URL belongs to another subsystem or scheme and was not parsed as a
    /// callback for any active ceremony.
    IgnoredUnmatchedUri,
    /// The URL was structurally a callback but carried an unknown, stale or
    /// already-consumed state value.
    RejectedStaleOrUnknownState,
    /// The callback was malformed or contained a forbidden fragment.
    RejectedMalformedCallback,
    /// The authorization server returned a sanitized OAuth error code.
    RejectedOAuthError { code: OAuthErrorCode },
    /// The ceremony was cancelled by `error=access_denied` or user action.
    CeremonyCancelled {
        ceremony_id: crate::CeremonyId,
        reason: gpui_system::CancelReason,
    },
    /// The code is ready for background token exchange. The code itself is
    /// retained privately and is never part of this value.
    ReadyForExchange { ceremony_id: crate::CeremonyId },
    /// A cold-launched process has no hydrated ceremony for this callback.
    ColdCeremonyUnavailable,
}

impl RedirectCaptureOutcome {
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::ReadyForExchange { .. })
    }

    pub const fn is_ignored(&self) -> bool {
        matches!(self, Self::IgnoredUnmatchedUri)
    }
}
