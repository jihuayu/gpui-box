//! Strict, credential-free endpoint origins.
//!
//! An [`EndpointOrigin`] is a scheme, a normalized host and a non-zero port.
//! Paths, query strings, fragments and userinfo are rejected, so an origin can
//! be logged or retained without leaking a session token or an account name.

use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

/// Transport schemes accepted in an endpoint origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginScheme {
    Http,
    Https,
    Ws,
    Wss,
}

impl OriginScheme {
    pub const ALL: [Self; 4] = [Self::Http, Self::Https, Self::Ws, Self::Wss];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Ws => "ws",
            Self::Wss => "wss",
        }
    }

    pub const fn default_port(self) -> u16 {
        match self {
            Self::Http | Self::Ws => 80,
            Self::Https | Self::Wss => 443,
        }
    }

    pub const fn is_secure(self) -> bool {
        matches!(self, Self::Https | Self::Wss)
    }

    pub fn parse(value: &str) -> Result<Self, SystemError> {
        match value.to_ascii_lowercase().as_str() {
            "http" => Ok(Self::Http),
            "https" => Ok(Self::Https),
            "ws" => Ok(Self::Ws),
            "wss" => Ok(Self::Wss),
            _ => Err(invalid(
                "Only http, https, ws and wss origins are supported",
            )),
        }
    }
}

impl fmt::Display for OriginScheme {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for OriginScheme {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// A normalized host: a DNS name, an IPv4 literal or a bracketed IPv6 literal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EndpointHost {
    Domain(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

impl fmt::Display for EndpointHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Domain(host) => formatter.write_str(host),
            Self::Ipv4(address) => write!(formatter, "{address}"),
            Self::Ipv6(address) => write!(formatter, "[{address}]"),
        }
    }
}

/// A validated origin without a path, query, fragment or userinfo component.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndpointOrigin {
    scheme: OriginScheme,
    host: String,
    port: u16,
}

impl EndpointOrigin {
    /// Build an origin from explicit parts. IPv6 hosts must be bracketed.
    pub fn new(scheme: OriginScheme, host: &str, port: u16) -> Result<Self, SystemError> {
        if port == 0 {
            return Err(invalid("An origin port must be non-zero"));
        }
        let host = normalize_host(host)?.to_string();
        Ok(Self { scheme, host, port })
    }

    /// Build an origin and use the scheme's default port.
    pub fn with_default_port(scheme: OriginScheme, host: &str) -> Result<Self, SystemError> {
        Self::new(scheme, host, scheme.default_port())
    }

    pub fn https(host: &str) -> Result<Self, SystemError> {
        Self::with_default_port(OriginScheme::Https, host)
    }

    pub fn http(host: &str) -> Result<Self, SystemError> {
        Self::with_default_port(OriginScheme::Http, host)
    }

    pub fn ws(host: &str) -> Result<Self, SystemError> {
        Self::with_default_port(OriginScheme::Ws, host)
    }

    pub fn wss(host: &str) -> Result<Self, SystemError> {
        Self::with_default_port(OriginScheme::Wss, host)
    }

    /// Parse a strict `scheme://host[:port]` origin.
    ///
    /// Trailing slashes, paths, queries, fragments, userinfo, whitespace,
    /// control characters and port zero are rejected. A missing port uses the
    /// scheme default.
    pub fn parse(input: &str) -> Result<Self, SystemError> {
        if input.is_empty()
            || input
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(invalid("An origin must be nonempty and free of whitespace"));
        }
        let (scheme, authority) = input
            .split_once("://")
            .ok_or_else(|| invalid("An origin requires an explicit scheme"))?;
        if authority.is_empty() {
            return Err(invalid("An origin requires a host"));
        }
        if authority
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@' | b'\\' | b'%'))
        {
            return Err(invalid(
                "An origin cannot contain a path, query, fragment or userinfo",
            ));
        }
        let scheme = OriginScheme::parse(scheme)?;

        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let end = rest
                .find(']')
                .ok_or_else(|| invalid("An IPv6 origin requires a closing bracket"))?;
            let address = &rest[..end];
            let suffix = &rest[end + 1..];
            let address = address
                .parse::<Ipv6Addr>()
                .map_err(|_| invalid("An IPv6 literal is invalid"))?;
            let port = if suffix.is_empty() {
                scheme.default_port()
            } else {
                let value = suffix
                    .strip_prefix(':')
                    .ok_or_else(|| invalid("Only a port may follow a bracketed IPv6 host"))?;
                parse_port(value)?
            };
            (EndpointHost::Ipv6(address).to_string(), port)
        } else {
            if authority.matches(':').count() > 1 {
                return Err(invalid("IPv6 literals must be bracketed"));
            }
            let (host, port) = match authority.rsplit_once(':') {
                Some((host, port)) => (host, parse_port(port)?),
                None => (authority, scheme.default_port()),
            };
            (normalize_host(host)?.to_string(), port)
        };

        Ok(Self { scheme, host, port })
    }

    /// Strip a path, query and fragment from an absolute URL and return only
    /// its validated origin.
    ///
    /// Userinfo is rejected rather than stripped: a URL carrying `user:pass@`
    /// is never treated as a safe portal origin. Query tokens are discarded
    /// before the origin is constructed, so they cannot be retained or logged.
    pub fn sanitize_url_origin(input: &str) -> Result<Self, SystemError> {
        if input.is_empty()
            || input
                .chars()
                .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(invalid("A URL must be nonempty and free of whitespace"));
        }
        let (scheme, rest) = input
            .split_once("://")
            .ok_or_else(|| invalid("A URL requires an explicit scheme"))?;
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..end];
        if authority.is_empty() || authority.contains('@') {
            return Err(invalid(
                "A portal URL origin cannot contain userinfo or an empty host",
            ));
        }
        Self::parse(&format!("{scheme}://{authority}"))
    }

    /// Alias for [`Self::sanitize_url_origin`].
    pub fn sanitize_url(input: &str) -> Result<Self, SystemError> {
        Self::sanitize_url_origin(input)
    }

    pub const fn scheme(&self) -> OriginScheme {
        self.scheme
    }

    /// The canonical host, including brackets around an IPv6 literal.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn host_string(&self) -> String {
        self.host.clone()
    }

    pub fn host_kind(&self) -> EndpointHost {
        if let Some(inner) = self
            .host
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            && let Ok(address) = inner.parse::<Ipv6Addr>()
        {
            return EndpointHost::Ipv6(address);
        }
        if let Ok(address) = self.host.parse::<Ipv4Addr>() {
            return EndpointHost::Ipv4(address);
        }
        EndpointHost::Domain(self.host.clone())
    }

    pub fn ipv4(&self) -> Option<Ipv4Addr> {
        self.host.parse().ok()
    }

    pub fn ipv6(&self) -> Option<Ipv6Addr> {
        self.host
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .and_then(|inner| inner.parse().ok())
    }

    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Canonical string with the port always present.
    pub fn origin_string(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    pub fn is_loopback(&self) -> bool {
        match self.host_kind() {
            EndpointHost::Domain(host) => host == "localhost" || host.ends_with(".localhost"),
            EndpointHost::Ipv4(address) => address.is_loopback(),
            EndpointHost::Ipv6(address) => address.is_loopback(),
        }
    }
}

impl fmt::Display for EndpointOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}://{}", self.scheme, self.host)?;
        if self.port != self.scheme.default_port() {
            write!(formatter, ":{}", self.port)?;
        }
        Ok(())
    }
}

impl fmt::Debug for EndpointOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EndpointOrigin")
            .field("scheme", &self.scheme)
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

impl FromStr for EndpointOrigin {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<&str> for EndpointOrigin {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse(value)
    }
}

impl Serialize for EndpointOrigin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EndpointOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

fn parse_port(value: &str) -> Result<u16, SystemError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("An origin port must be a decimal u16"));
    }
    let port = value
        .parse::<u16>()
        .map_err(|_| invalid("An origin port must fit a u16"))?;
    if port == 0 {
        return Err(invalid("An origin port must be non-zero"));
    }
    Ok(port)
}

fn normalize_host(value: &str) -> Result<EndpointHost, SystemError> {
    if value.is_empty() {
        return Err(invalid("An origin requires a host"));
    }
    if value.starts_with('[') {
        let Some(inner) = value
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        else {
            return Err(invalid("An IPv6 literal requires matching brackets"));
        };
        let address = inner
            .parse::<Ipv6Addr>()
            .map_err(|_| invalid("An IPv6 literal is invalid"))?;
        return Ok(EndpointHost::Ipv6(address));
    }
    if value.contains(':') {
        return Err(invalid("IPv6 literals must be bracketed"));
    }
    if let Ok(address) = value.parse::<Ipv4Addr>() {
        return Ok(EndpointHost::Ipv4(address));
    }
    let lower = value.to_ascii_lowercase();
    if lower.len() > 253 {
        return Err(invalid("A hostname exceeds 253 bytes"));
    }
    if lower.ends_with('.') {
        return Err(invalid("A hostname cannot have a trailing dot"));
    }
    for label in lower.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(invalid("A hostname label must contain 1 to 63 bytes"));
        }
        let bytes = label.as_bytes();
        if !bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(invalid(
                "A hostname label must begin and end with an ASCII alphanumeric",
            ));
        }
        if !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            return Err(invalid(
                "A hostname may contain only ASCII alphanumerics and hyphens",
            ));
        }
    }
    Ok(EndpointHost::Domain(lower))
}

/// Normalize a host independently of a port, used by bypass patterns.
pub(crate) fn normalize_bypass_host(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Ok(host) = normalize_host(value) {
        return Some(host.to_string());
    }
    // Bare IPv6 bypass entries are accepted and canonicalized with brackets.
    if value.matches(':').count() > 1
        && let Ok(address) = value.parse::<Ipv6Addr>()
    {
        return Some(format!("[{address}]"));
    }
    None
}
