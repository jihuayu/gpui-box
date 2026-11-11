//! Strict URL, host, origin and callback parsing without external crates.
//!
//! These parsers are intentionally narrower than a browser URL parser. They
//! reject userinfo, fragments, control characters, whitespace, empty hosts and
//! non-ASCII authority text so authentication URLs cannot smuggle credentials
//! or implicit-flow fragments across a trust boundary.

use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

/// Maximum length accepted for an authentication URL or callback URL.
pub const MAX_AUTH_URL_LEN: usize = 4096;
/// Maximum length accepted for a redirect path.
pub const MAX_REDIRECT_PATH_LEN: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlError {
    Empty,
    Oversized,
    ControlOrWhitespace,
    NonAscii,
    Malformed,
    MissingScheme,
    InvalidScheme,
    MissingAuthority,
    UserInfo,
    Fragment,
    EmptyHost,
    InvalidHost,
    InvalidPort,
    InvalidPath,
    InvalidQuery,
    InsecureScheme,
    InsecureOrigin,
}

impl UrlError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "URL is empty",
            Self::Oversized => "URL exceeds the supported bound",
            Self::ControlOrWhitespace => "URL contains control or whitespace characters",
            Self::NonAscii => "URL authority must be ASCII",
            Self::Malformed => "URL is malformed",
            Self::MissingScheme => "URL has no scheme",
            Self::InvalidScheme => "URL scheme is invalid",
            Self::MissingAuthority => "URL requires an explicit authority",
            Self::UserInfo => "URL userinfo is not accepted",
            Self::Fragment => "URL fragments are not accepted",
            Self::EmptyHost => "URL host is empty",
            Self::InvalidHost => "URL host is invalid",
            Self::InvalidPort => "URL port is invalid",
            Self::InvalidPath => "URL path is invalid",
            Self::InvalidQuery => "URL query is invalid",
            Self::InsecureScheme => "URL must use https",
            Self::InsecureOrigin => "Origin is not a secure-context origin",
        }
    }
}

impl fmt::Display for UrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::error::Error for UrlError {}

/// A normalized host: a DNS name, IPv4 literal or bracketed IPv6 literal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Host {
    Domain(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

impl Host {
    pub fn parse(value: &str) -> Result<Self, UrlError> {
        if value.is_empty() {
            return Err(UrlError::EmptyHost);
        }
        if value.starts_with('[') {
            let inner = value
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .ok_or(UrlError::InvalidHost)?;
            if inner.contains('%') {
                return Err(UrlError::InvalidHost);
            }
            let address = Ipv6Addr::from_str(inner).map_err(|_| UrlError::InvalidHost)?;
            return Ok(Self::Ipv6(address));
        }
        if value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
            && value.contains('.')
        {
            let address = Ipv4Addr::from_str(value).map_err(|_| UrlError::InvalidHost)?;
            return Ok(Self::Ipv4(address));
        }
        if !valid_dns_name(value) {
            return Err(UrlError::InvalidHost);
        }
        Ok(Self::Domain(value.to_ascii_lowercase()))
    }

    pub fn as_display(&self) -> String {
        match self {
            Self::Domain(name) => name.clone(),
            Self::Ipv4(address) => address.to_string(),
            Self::Ipv6(address) => format!("[{address}]"),
        }
    }

    pub fn is_loopback(&self) -> bool {
        match self {
            Self::Domain(name) => name == "localhost",
            Self::Ipv4(address) => address.is_loopback(),
            Self::Ipv6(address) => address.is_loopback(),
        }
    }

    pub fn as_domain(&self) -> Option<&str> {
        match self {
            Self::Domain(name) => Some(name),
            Self::Ipv4(_) | Self::Ipv6(_) => None,
        }
    }

    /// Effective-domain comparison used by WebAuthn RP ID matching.
    pub fn matches_rp_id(&self, rp_id: &str) -> bool {
        let Some(domain) = self.as_domain() else {
            return false;
        };
        domain == rp_id
            || domain
                .strip_suffix(rp_id)
                .is_some_and(|prefix| prefix.ends_with('.'))
    }
}

fn valid_dns_name(value: &str) -> bool {
    if value.is_empty() || value.len() > 253 || !value.is_ascii() {
        return false;
    }
    if value.eq_ignore_ascii_case("localhost") {
        return true;
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

/// A validated origin used for WebAuthn and same-origin redirects.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthOrigin {
    scheme: OriginScheme,
    host: Host,
    port: Option<u16>,
    effective_port: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OriginScheme {
    Https,
    Http,
}

impl AuthOrigin {
    /// Parse a strict origin. `https` is required except for loopback hosts,
    /// which are secure contexts over `http` under WebAuthn/browser rules.
    pub fn parse(value: &str) -> Result<Self, UrlError> {
        validate_text(value)?;
        let (scheme, rest) = split_scheme(value)?;
        let (scheme, default_port) = match scheme.as_str() {
            "https" => (OriginScheme::Https, 443),
            "http" => (OriginScheme::Http, 80),
            _ => return Err(UrlError::InsecureOrigin),
        };
        let authority = rest.strip_prefix("//").ok_or(UrlError::MissingAuthority)?;
        if authority.is_empty() {
            return Err(UrlError::EmptyHost);
        }
        if authority.contains(['/', '?', '#']) {
            return Err(UrlError::Malformed);
        }
        let (host, port) = parse_authority(authority)?;
        if scheme == OriginScheme::Http && !host.is_loopback() {
            return Err(UrlError::InsecureOrigin);
        }
        let effective_port = port.unwrap_or(default_port);
        Ok(Self {
            scheme,
            host,
            port,
            effective_port,
        })
    }

    pub fn scheme(&self) -> OriginScheme {
        self.scheme
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub const fn effective_port(&self) -> u16 {
        self.effective_port
    }

    pub fn is_secure_context(&self) -> bool {
        self.scheme == OriginScheme::Https || self.host.is_loopback()
    }

    pub fn matches_rp_id(&self, rp_id: &str) -> bool {
        self.is_secure_context() && self.host.matches_rp_id(rp_id)
    }

    pub fn as_string(&self) -> String {
        let scheme = match self.scheme {
            OriginScheme::Https => "https",
            OriginScheme::Http => "http",
        };
        match self.port {
            Some(port) => format!("{scheme}://{}:{port}", self.host.as_display()),
            None => format!("{scheme}://{}", self.host.as_display()),
        }
    }
}

impl fmt::Display for AuthOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.as_string())
    }
}

/// A validated HTTPS authorization/token endpoint URL.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthEndpointUrl {
    raw: String,
    scheme: OriginScheme,
    host: Host,
    port: Option<u16>,
    path_and_query: String,
}

impl AuthEndpointUrl {
    /// Parse a strict endpoint. Plain `http` is accepted only for explicitly
    /// allowed loopback IdP testing.
    pub fn parse(value: &str) -> Result<Self, UrlError> {
        Self::parse_with_loopback(value, false)
    }

    /// Parse an endpoint, optionally allowing `http` on loopback hosts.
    pub fn parse_with_loopback(value: &str, allow_loopback_http: bool) -> Result<Self, UrlError> {
        validate_text(value)?;
        let (scheme, rest) = split_scheme(value)?;
        let (scheme, secure_default) = match scheme.as_str() {
            "https" => (OriginScheme::Https, 443),
            "http" => (OriginScheme::Http, 80),
            _ => return Err(UrlError::InsecureScheme),
        };
        let authority_and_path = rest.strip_prefix("//").ok_or(UrlError::MissingAuthority)?;
        let split = authority_and_path
            .find(['/', '?'])
            .unwrap_or(authority_and_path.len());
        let (authority, remainder) = authority_and_path.split_at(split);
        if authority.is_empty() {
            return Err(UrlError::EmptyHost);
        }
        let (host, port) = parse_authority(authority)?;
        if scheme == OriginScheme::Http && !(allow_loopback_http && host.is_loopback()) {
            return Err(UrlError::InsecureScheme);
        }
        let (path, query) = match remainder.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (remainder, None),
        };
        let path = if path.is_empty() { "/" } else { path };
        validate_path(path)?;
        if let Some(query) = query {
            validate_query(query)?;
        }
        let _ = secure_default;
        let path_and_query = match query {
            Some(query) => format!("{path}?{query}"),
            None => path.to_owned(),
        };
        Ok(Self {
            raw: format!(
                "{}://{}{}",
                if scheme == OriginScheme::Https {
                    "https"
                } else {
                    "http"
                },
                authority_display(&host, port),
                path_and_query
            ),
            scheme,
            host,
            port,
            path_and_query,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn scheme(&self) -> OriginScheme {
        self.scheme
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    /// Append query parameters to the endpoint, preserving an existing query.
    pub fn with_query(&self, parameters: &[(&str, &str)]) -> String {
        let mut result = self.raw.clone();
        let mut separator = if self.path_and_query.contains('?') {
            '&'
        } else {
            '?'
        };
        for (name, value) in parameters {
            result.push(separator);
            separator = '&';
            result.push_str(&encode_query_component(name));
            result.push('=');
            result.push_str(&encode_query_component(value));
        }
        result
    }
}

impl fmt::Debug for AuthEndpointUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthEndpointUrl")
            .field("scheme", &self.scheme)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path", &self.path_and_query)
            .finish()
    }
}

/// Parsed callback URL. The query map is private and never formatted; only
/// public-safe structural accessors are exposed.
#[derive(Clone)]
pub struct CallbackUrl {
    scheme: String,
    host: Option<Host>,
    port: Option<u16>,
    path: String,
    query: Vec<(String, String)>,
}

impl CallbackUrl {
    pub fn parse(value: &str) -> Result<Self, UrlError> {
        validate_text(value)?;
        if value.contains('#') {
            return Err(UrlError::Fragment);
        }
        let (scheme, rest) = split_scheme(value)?;
        let scheme = scheme.to_ascii_lowercase();
        let (authority, remainder) = if let Some(after) = rest.strip_prefix("//") {
            let split = after.find(['/', '?']).unwrap_or(after.len());
            (Some(&after[..split]), &after[split..])
        } else {
            (None, rest)
        };
        let (host, port) = match authority {
            Some(authority) if !authority.is_empty() => {
                let (host, port) = parse_authority(authority)?;
                (Some(host), port)
            }
            Some(_) => return Err(UrlError::EmptyHost),
            None => (None, None),
        };
        let (path, query) = match remainder.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (remainder, None),
        };
        if path.is_empty() || !path.starts_with('/') {
            return Err(UrlError::InvalidPath);
        }
        validate_path(path)?;
        let mut parsed_query = Vec::new();
        if let Some(query) = query {
            validate_query(query)?;
            for pair in query.split('&').filter(|pair| !pair.is_empty()) {
                let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                let name = decode_query_component(name)?;
                let value = decode_query_component(value)?;
                if name.is_empty() || name.len() > 128 || value.len() > 4096 {
                    return Err(UrlError::InvalidQuery);
                }
                if parsed_query.len() == 32 {
                    return Err(UrlError::InvalidQuery);
                }
                parsed_query.push((name, value));
            }
        }
        Ok(Self {
            scheme,
            host,
            port,
            path: path.to_owned(),
            query: parsed_query,
        })
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn host(&self) -> Option<&Host> {
        self.host.as_ref()
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Public-safe presence check without revealing the value.
    pub fn has_parameter(&self, name: &str) -> bool {
        self.query.iter().any(|(key, _)| key == name)
    }

    pub(crate) fn parameter(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

impl fmt::Debug for CallbackUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Query values carry authorization codes and state tokens. They are
        // never included, even in a redacted-looking form that could be
        // reversed or correlated.
        formatter
            .debug_struct("CallbackUrl")
            .field("scheme", &self.scheme)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("path", &self.path)
            .field("parameter_count", &self.query.len())
            .finish()
    }
}

fn validate_text(value: &str) -> Result<(), UrlError> {
    if value.is_empty() {
        return Err(UrlError::Empty);
    }
    if value.len() > MAX_AUTH_URL_LEN {
        return Err(UrlError::Oversized);
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(UrlError::ControlOrWhitespace);
    }
    if !value.is_ascii() {
        return Err(UrlError::NonAscii);
    }
    Ok(())
}

fn split_scheme(value: &str) -> Result<(String, &str), UrlError> {
    let (scheme, rest) = value.split_once(':').ok_or(UrlError::MissingScheme)?;
    if scheme.is_empty() {
        return Err(UrlError::MissingScheme);
    }
    let mut bytes = scheme.bytes();
    if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
    {
        return Err(UrlError::InvalidScheme);
    }
    Ok((scheme.to_ascii_lowercase(), rest))
}

fn parse_authority(authority: &str) -> Result<(Host, Option<u16>), UrlError> {
    if authority.is_empty() {
        return Err(UrlError::EmptyHost);
    }
    if authority.contains('@') {
        return Err(UrlError::UserInfo);
    }
    let (host_text, port_text) = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']').ok_or(UrlError::InvalidHost)?;
        let host = &authority[..end + 2];
        let remainder = &rest[end + 1..];
        if remainder.is_empty() {
            (host, None)
        } else {
            let port = remainder.strip_prefix(':').ok_or(UrlError::InvalidHost)?;
            (host, Some(port))
        }
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() => (host, Some(port)),
            Some((_, "")) => return Err(UrlError::InvalidPort),
            Some((host, _)) => (host, None),
            None => (authority, None),
        }
    };
    let host = Host::parse(host_text)?;
    let port = match port_text {
        Some(port) => {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(UrlError::InvalidPort);
            }
            let port = port.parse::<u16>().map_err(|_| UrlError::InvalidPort)?;
            if port == 0 {
                return Err(UrlError::InvalidPort);
            }
            Some(port)
        }
        None => None,
    };
    Ok((host, port))
}

fn validate_path(path: &str) -> Result<(), UrlError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.len() > MAX_REDIRECT_PATH_LEN
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'#' | b'?'))
    {
        return Err(UrlError::InvalidPath);
    }
    Ok(())
}

fn validate_query(query: &str) -> Result<(), UrlError> {
    if query.len() > MAX_AUTH_URL_LEN
        || query
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'#'))
    {
        return Err(UrlError::InvalidQuery);
    }
    Ok(())
}

fn authority_display(host: &Host, port: Option<u16>) -> String {
    match port {
        Some(port) => format!("{}:{port}", host.as_display()),
        None => host.as_display(),
    }
}

/// RFC 3986 percent-encoding for one query component.
pub fn encode_query_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn decode_query_component(value: &str) -> Result<String, UrlError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(UrlError::InvalidQuery);
                }
                let high = hex_value(bytes[index + 1]).ok_or(UrlError::InvalidQuery)?;
                let low = hex_value(bytes[index + 2]).ok_or(UrlError::InvalidQuery)?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| UrlError::InvalidQuery)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Strict RP ID validation. Raw IP literals, ports, schemes, paths and
/// uppercase labels are rejected to prevent suffix-confusion attacks.
pub fn validate_rp_id(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 253
        || !value.is_ascii()
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return false;
    }
    if value.parse::<Ipv4Addr>().is_ok() || value.parse::<Ipv6Addr>().is_ok() {
        return false;
    }
    if value.eq_ignore_ascii_case("localhost") {
        return value == "localhost";
    }
    value.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_secure_endpoints_and_rejects_unsafe_shapes() {
        let endpoint = AuthEndpointUrl::parse("https://idp.example/oauth/authorize?audience=a")
            .expect("endpoint");
        assert_eq!(endpoint.host().as_domain(), Some("idp.example"));
        assert!(
            endpoint
                .with_query(&[("state", "a b")])
                .contains("state=a%20b")
        );

        assert_eq!(
            AuthEndpointUrl::parse("http://idp.example/authorize").unwrap_err(),
            UrlError::InsecureScheme
        );
        assert_eq!(
            AuthEndpointUrl::parse("https://user:pass@idp.example/authorize").unwrap_err(),
            UrlError::UserInfo
        );
        assert_eq!(
            AuthEndpointUrl::parse("https://idp.example/authorize#fragment").unwrap_err(),
            UrlError::InvalidPath
        );
        assert_eq!(
            AuthEndpointUrl::parse("https://idp.example:99999/authorize").unwrap_err(),
            UrlError::InvalidPort
        );
        assert!(
            AuthEndpointUrl::parse_with_loopback("http://127.0.0.1:8080/authorize", true).is_ok()
        );
        assert!(
            AuthEndpointUrl::parse_with_loopback("http://idp.example/authorize", true).is_err()
        );
    }

    #[test]
    fn origins_require_secure_contexts() {
        assert!(AuthOrigin::parse("https://example.com").is_ok());
        assert!(AuthOrigin::parse("http://localhost:3000").is_ok());
        assert!(AuthOrigin::parse("http://127.0.0.1:8080").is_ok());
        assert!(AuthOrigin::parse("http://[::1]:8080").is_ok());
        assert!(AuthOrigin::parse("http://example.com").is_err());
        assert!(AuthOrigin::parse("https://example.com/path").is_err());
        assert!(AuthOrigin::parse("https://example.com:0").is_err());
    }

    #[test]
    fn rp_id_is_strict_and_suffix_safe() {
        assert!(validate_rp_id("example.com"));
        assert!(validate_rp_id("auth.example.com"));
        assert!(!validate_rp_id("Example.com"));
        assert!(!validate_rp_id("192.168.1.1"));
        assert!(!validate_rp_id("::1"));
        assert!(!validate_rp_id("-example.com"));
        assert!(!validate_rp_id("example.com."));
        let origin = AuthOrigin::parse("https://login.example.com").unwrap();
        assert!(origin.matches_rp_id("example.com"));
        assert!(!origin.matches_rp_id("ample.com"));
        assert!(
            !AuthOrigin::parse("http://localhost:3000")
                .unwrap()
                .matches_rp_id("example.com")
        );
    }

    #[test]
    fn callback_parsing_never_formats_query_secrets() {
        let callback = CallbackUrl::parse(
            "com.example.app://auth/callback?code=SECRET&state=STATE&error_description=Private",
        )
        .expect("callback");
        assert_eq!(callback.scheme(), "com.example.app");
        assert_eq!(
            callback.host().map(Host::as_display).as_deref(),
            Some("auth")
        );
        assert_eq!(callback.parameter("code"), Some("SECRET"));
        let debug = format!("{callback:?}");
        assert!(!debug.contains("SECRET"));
        assert!(!debug.contains("STATE"));
        assert!(!debug.contains("Private"));
        assert_eq!(
            CallbackUrl::parse("com.example.app://auth/callback#access_token=x").unwrap_err(),
            UrlError::Fragment
        );
    }

    #[test]
    fn ipv6_callback_authority_is_normalized() {
        let callback =
            CallbackUrl::parse("http://[::1]:49152/oauth/callback?state=x").expect("callback");
        assert_eq!(callback.host(), Some(&Host::Ipv6(Ipv6Addr::LOCALHOST)));
        assert_eq!(callback.port(), Some(49152));
    }
}
