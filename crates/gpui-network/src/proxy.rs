//! Proxy configuration, bypass matching and deterministic candidate ordering.

use crate::{EndpointOrigin, OriginScheme, PlatformTarget};
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, Ipv6Addr};

fn invalid(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::InvalidInput, message)
}

fn unsupported(message: &'static str) -> SystemError {
    SystemError::new(ErrorCode::Unsupported, message)
}

/// Proxy protocol for an ordered candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyScheme {
    Http,
    Https,
    Socks5,
}

impl ProxyScheme {
    pub const ALL: [Self; 3] = [Self::Http, Self::Https, Self::Socks5];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5 => "socks5",
        }
    }
}

/// One proxy in an ordered static rule set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyRule {
    pub scheme: ProxyScheme,
    pub proxy: EndpointOrigin,
    /// When present, the rule applies only to targets with this scheme.
    pub target_scheme: Option<OriginScheme>,
}

impl ProxyRule {
    pub const fn new(scheme: ProxyScheme, proxy: EndpointOrigin) -> Self {
        Self {
            scheme,
            proxy,
            target_scheme: None,
        }
    }

    pub const fn for_target_scheme(mut self, target_scheme: OriginScheme) -> Self {
        self.target_scheme = Some(target_scheme);
        self
    }

    pub fn matches_target(&self, target_scheme: OriginScheme) -> bool {
        self.target_scheme
            .is_none_or(|scheme| scheme == target_scheme)
    }

    pub fn candidate(&self) -> ProxyCandidate {
        match self.scheme {
            ProxyScheme::Http => ProxyCandidate::Http {
                proxy: self.proxy.clone(),
            },
            ProxyScheme::Https => ProxyCandidate::Https {
                proxy: self.proxy.clone(),
            },
            ProxyScheme::Socks5 => ProxyCandidate::Socks5 {
                proxy: self.proxy.clone(),
            },
        }
    }
}

/// An ordered proxy candidate. `Direct` means "connect without a proxy".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCandidate {
    Direct,
    Http { proxy: EndpointOrigin },
    Https { proxy: EndpointOrigin },
    Socks5 { proxy: EndpointOrigin },
}

impl ProxyCandidate {
    pub fn http(proxy: EndpointOrigin) -> Self {
        Self::Http { proxy }
    }

    pub fn https(proxy: EndpointOrigin) -> Self {
        Self::Https { proxy }
    }

    pub fn socks5(proxy: EndpointOrigin) -> Self {
        Self::Socks5 { proxy }
    }

    pub const fn scheme(&self) -> Option<ProxyScheme> {
        match self {
            Self::Direct => None,
            Self::Http { .. } => Some(ProxyScheme::Http),
            Self::Https { .. } => Some(ProxyScheme::Https),
            Self::Socks5 { .. } => Some(ProxyScheme::Socks5),
        }
    }

    pub const fn is_direct(&self) -> bool {
        matches!(self, Self::Direct)
    }

    pub fn proxy(&self) -> Option<&EndpointOrigin> {
        match self {
            Self::Direct => None,
            Self::Http { proxy } | Self::Https { proxy } | Self::Socks5 { proxy } => Some(proxy),
        }
    }
}

/// A deterministic bypass rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BypassPattern {
    /// `localhost`, `*.localhost`, `127.0.0.0/8` and `::1`.
    Loopback,
    Ipv4Cidr {
        network: Ipv4Addr,
        prefix_len: u8,
    },
    Ipv6Cidr {
        network: Ipv6Addr,
        prefix_len: u8,
    },
    /// A normalized DNS suffix. A leading `*.` requires a subdomain.
    DomainSuffix(String),
    ExactHost(String),
    ExactHostPort {
        host: String,
        port: u16,
    },
}

impl BypassPattern {
    pub const fn loopback() -> Self {
        Self::Loopback
    }

    pub fn ipv4_cidr(network: Ipv4Addr, prefix_len: u8) -> Result<Self, SystemError> {
        if prefix_len > 32 {
            return Err(invalid("An IPv4 CIDR prefix cannot exceed 32"));
        }
        Ok(Self::Ipv4Cidr {
            network,
            prefix_len,
        })
    }

    pub fn ipv6_cidr(network: Ipv6Addr, prefix_len: u8) -> Result<Self, SystemError> {
        if prefix_len > 128 {
            return Err(invalid("An IPv6 CIDR prefix cannot exceed 128"));
        }
        Ok(Self::Ipv6Cidr {
            network,
            prefix_len,
        })
    }

    pub fn domain_suffix(value: &str) -> Result<Self, SystemError> {
        let pattern = Self::DomainSuffix(value.to_owned());
        pattern.validate()?;
        Ok(pattern)
    }

    pub fn exact_host(value: &str) -> Result<Self, SystemError> {
        let pattern = Self::ExactHost(value.to_owned());
        pattern.validate()?;
        Ok(pattern)
    }

    pub fn exact_host_port(host: &str, port: u16) -> Result<Self, SystemError> {
        let pattern = Self::ExactHostPort {
            host: host.to_owned(),
            port,
        };
        pattern.validate()?;
        Ok(pattern)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::Loopback => Ok(()),
            Self::Ipv4Cidr { prefix_len, .. } => {
                if *prefix_len > 32 {
                    Err(invalid("An IPv4 CIDR prefix cannot exceed 32"))
                } else {
                    Ok(())
                }
            }
            Self::Ipv6Cidr { prefix_len, .. } => {
                if *prefix_len > 128 {
                    Err(invalid("An IPv6 CIDR prefix cannot exceed 128"))
                } else {
                    Ok(())
                }
            }
            Self::DomainSuffix(value) => {
                let suffix = value.strip_prefix("*.").unwrap_or(value);
                if suffix.is_empty() || suffix.len() > 253 {
                    return Err(invalid("A bypass domain suffix must be bounded"));
                }
                if suffix.contains(['/', '?', '#', '@', ':', '\\', '%'])
                    || suffix.chars().any(char::is_control)
                {
                    return Err(invalid("A bypass domain suffix is not a hostname"));
                }
                normalize_suffix(suffix)
                    .ok_or_else(|| invalid("A bypass domain suffix is not a hostname"))?;
                Ok(())
            }
            Self::ExactHost(value) => crate::origin::normalize_bypass_host(value)
                .map(|_| ())
                .ok_or_else(|| invalid("A bypass exact host is not a hostname")),
            Self::ExactHostPort { host, port } => {
                if *port == 0 {
                    return Err(invalid("A bypass exact host port must be non-zero"));
                }
                crate::origin::normalize_bypass_host(host)
                    .map(|_| ())
                    .ok_or_else(|| invalid("A bypass exact host is not a hostname"))
            }
        }
    }

    pub fn matches(&self, origin: &EndpointOrigin) -> bool {
        match self {
            Self::Loopback => origin.is_loopback(),
            Self::Ipv4Cidr {
                network,
                prefix_len,
            } => {
                if *prefix_len > 32 {
                    return false;
                }
                let Some(address) = origin.ipv4() else {
                    return false;
                };
                let mask = if *prefix_len == 0 {
                    0
                } else {
                    u32::MAX << (32 - u32::from(*prefix_len))
                };
                (u32::from(address) & mask) == (u32::from(*network) & mask)
            }
            Self::Ipv6Cidr {
                network,
                prefix_len,
            } => {
                if *prefix_len > 128 {
                    return false;
                }
                let Some(address) = origin.ipv6() else {
                    return false;
                };
                let mask = if *prefix_len == 0 {
                    0
                } else {
                    u128::MAX << (128 - u32::from(*prefix_len))
                };
                (u128::from(address) & mask) == (u128::from(*network) & mask)
            }
            Self::DomainSuffix(value) => {
                let Some(host) = domain_host(origin) else {
                    return false;
                };
                let requires_subdomain = value.starts_with("*.");
                let raw = value.strip_prefix("*.").unwrap_or(value);
                let Some(suffix) = normalize_suffix(raw) else {
                    return false;
                };
                if requires_subdomain {
                    host.len() > suffix.len() && host.ends_with(&format!(".{suffix}"))
                } else {
                    host == suffix || host.ends_with(&format!(".{suffix}"))
                }
            }
            Self::ExactHost(value) => {
                let Some(pattern) = crate::origin::normalize_bypass_host(value) else {
                    return false;
                };
                origin.host() == pattern
            }
            Self::ExactHostPort { host, port } => {
                if origin.port() != *port || *port == 0 {
                    return false;
                }
                let Some(pattern) = crate::origin::normalize_bypass_host(host) else {
                    return false;
                };
                origin.host() == pattern
            }
        }
    }
}

fn domain_host(origin: &EndpointOrigin) -> Option<String> {
    match origin.host_kind() {
        crate::EndpointHost::Domain(host) => Some(host),
        _ => None,
    }
}

fn normalize_suffix(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.is_empty() || lower.len() > 253 || lower.ends_with('.') {
        return None;
    }
    for label in lower.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        let bytes = label.as_bytes();
        if !bytes
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !bytes
                .last()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        {
            return None;
        }
    }
    Some(lower)
}

/// System proxy policy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyConfig {
    /// Always connect directly.
    #[default]
    Direct,
    /// Ordered proxy candidates with deterministic bypass rules.
    StaticRules {
        proxies: Vec<ProxyRule>,
        bypass: Vec<BypassPattern>,
    },
    /// A PAC script identified by a validated origin and opaque script id.
    PacUrl {
        script_origin: EndpointOrigin,
        script_id: String,
    },
    /// Delegate resolution to the native system proxy provider.
    SystemDynamic,
    /// The browser owns proxy/PAC/authentication; the application cannot
    /// resolve or inspect it. This is the mandatory representation on Web.
    BrowserOpaque,
}

impl ProxyConfig {
    pub const fn direct() -> Self {
        Self::Direct
    }

    pub fn static_rules(proxies: Vec<ProxyRule>, bypass: Vec<BypassPattern>) -> Self {
        Self::StaticRules { proxies, bypass }
    }

    pub fn pac_url(script_origin: EndpointOrigin, script_id: &str) -> Result<Self, SystemError> {
        validate_pac_script_id(script_id)?;
        Ok(Self::PacUrl {
            script_origin,
            script_id: script_id.to_owned(),
        })
    }

    pub const fn system_dynamic() -> Self {
        Self::SystemDynamic
    }

    pub const fn browser_opaque() -> Self {
        Self::BrowserOpaque
    }

    pub const fn is_browser_opaque(&self) -> bool {
        matches!(self, Self::BrowserOpaque)
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::Direct | Self::SystemDynamic | Self::BrowserOpaque => Ok(()),
            Self::StaticRules { bypass, .. } => {
                for pattern in bypass {
                    pattern.validate()?;
                }
                Ok(())
            }
            Self::PacUrl { script_id, .. } => validate_pac_script_id(script_id),
        }
    }

    /// Deterministic resolution for application-owned configurations.
    ///
    /// `SystemDynamic` and `PacUrl` are native/provider-owned; at this layer
    /// they fall back to `Direct`, and [`crate::NetworkService`] delegates them
    /// to an explicit [`crate::NetworkProvider`] instead. `BrowserOpaque`
    /// likewise reports `Direct` at this layer, while the service fails closed
    /// on Web.
    pub fn resolve_proxies(&self, target: &EndpointOrigin) -> Vec<ProxyCandidate> {
        match self {
            Self::Direct | Self::PacUrl { .. } | Self::SystemDynamic | Self::BrowserOpaque => {
                vec![ProxyCandidate::Direct]
            }
            Self::StaticRules { proxies, bypass } => {
                if bypass.iter().any(|pattern| pattern.matches(target)) {
                    return vec![ProxyCandidate::Direct];
                }
                let candidates: Vec<_> = proxies
                    .iter()
                    .filter(|rule| rule.matches_target(target.scheme()))
                    .map(ProxyRule::candidate)
                    .collect();
                if candidates.is_empty() {
                    vec![ProxyCandidate::Direct]
                } else {
                    candidates
                }
            }
        }
    }

    /// Alias for [`Self::resolve_proxies`].
    pub fn resolve(&self, target: &EndpointOrigin) -> Vec<ProxyCandidate> {
        self.resolve_proxies(target)
    }
}

/// Validate a platform's proxy representation.
///
/// Web exposes no application-visible proxy, PAC or 407 surface, so the only
/// accepted representation is [`ProxyConfig::BrowserOpaque`]. Native targets
/// cannot use the browser-owned representation.
pub fn validate_proxy_config(
    platform: PlatformTarget,
    config: &ProxyConfig,
) -> Result<(), SystemError> {
    config.validate()?;
    match platform {
        PlatformTarget::Web if !config.is_browser_opaque() => Err(unsupported(
            "The browser owns system proxy and PAC resolution on Web",
        )),
        PlatformTarget::Web => Ok(()),
        _ if config.is_browser_opaque() => Err(unsupported(
            "BrowserOpaque proxy policy is only valid on Web",
        )),
        _ => Ok(()),
    }
}

fn validate_pac_script_id(value: &str) -> Result<(), SystemError> {
    if value.is_empty() || value.len() > 128 {
        return Err(invalid("A PAC script id must contain 1 to 128 bytes"));
    }
    if !value.is_ascii()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(invalid(
            "A PAC script id may contain only ASCII alphanumerics, dots, underscores and hyphens",
        ));
    }
    Ok(())
}
