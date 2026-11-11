use crate::{ErrorCode, SystemError, error};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_URL: usize = 8192;
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || matches!(v, b'-' | b'_' | b'.'))
}
/// Normalized URL. Debug deliberately never exposes its potentially sensitive payload.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AllowedUrl(String);
impl AllowedUrl {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for AllowedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AllowedUrl([redacted])")
    }
}
#[derive(Clone)]
pub struct UrlPolicy {
    schemes: BTreeSet<String>,
}
impl UrlPolicy {
    pub fn new(schemes: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, SystemError> {
        let schemes: BTreeSet<_> = schemes
            .into_iter()
            .map(|v| v.as_ref().to_ascii_lowercase())
            .collect();
        if schemes.is_empty()
            || schemes.len() > 16
            || schemes.iter().any(|s| {
                !valid_scheme(s) || matches!(s.as_str(), "file" | "javascript" | "data" | "shell")
            })
        {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "URL schemes must be explicitly allowed; file and executable schemes are excluded",
            ));
        }
        Ok(Self { schemes })
    }
    pub fn parse(&self, input: &str) -> Result<AllowedUrl, SystemError> {
        if input.is_empty()
            || input.len() > MAX_URL
            || input.trim() != input
            || input.chars().any(char::is_control)
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "URL is empty, oversized or contains invalid whitespace",
            ));
        }
        let parsed = url::Url::parse(input)
            .map_err(|_| error(ErrorCode::InvalidInput, "URL is malformed"))?;
        if !self.schemes.contains(parsed.scheme()) {
            return Err(error(ErrorCode::Unsupported, "URL scheme is not allowed"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(error(
                ErrorCode::InvalidInput,
                "URL credentials are not accepted",
            ));
        }
        if matches!(parsed.scheme(), "http" | "https")
            && (!input
                .split_once(':')
                .is_some_and(|(_, tail)| tail.starts_with("//"))
                || parsed.host_str().is_none())
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "A web URL requires an explicit authority",
            ));
        }
        if parsed.as_str().len() > MAX_URL {
            return Err(error(
                ErrorCode::InvalidInput,
                "Normalized URL is oversized",
            ));
        }
        Ok(AllowedUrl(parsed.into()))
    }
    pub(crate) fn validate(&self, url: &AllowedUrl) -> Result<(), SystemError> {
        self.parse(url.as_str()).map(|_| ())
    }
}
fn valid_scheme(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActivationId(String);
impl ActivationId {
    /// Hosts must reuse this ID for redelivery of one event, not derive it from a URL.
    pub fn new(id: impl Into<String>) -> Result<Self, SystemError> {
        let id = id.into();
        if !identifier(&id) {
            return Err(error(
                ErrorCode::InvalidInput,
                "Activation identity is invalid",
            ));
        }
        Ok(Self(id))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for ActivationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationId([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationTarget {
    route: String,
    version: u32,
    parameters: BTreeMap<String, String>,
}
impl ActivationTarget {
    pub fn new(
        route: impl Into<String>,
        version: u32,
        parameters: BTreeMap<String, String>,
    ) -> Result<Self, SystemError> {
        let value = Self {
            route: route.into(),
            version,
            parameters,
        };
        value.validate_shape()?;
        Ok(value)
    }
    pub fn route(&self) -> &str {
        &self.route
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn parameters(&self) -> &BTreeMap<String, String> {
        &self.parameters
    }
    fn validate_shape(&self) -> Result<(), SystemError> {
        if !identifier(&self.route)
            || self.version == 0
            || self.parameters.len() > 16
            || self
                .parameters
                .iter()
                .any(|(k, v)| !identifier(k) || v.len() > 1024 || v.chars().any(char::is_control))
        {
            return Err(error(
                ErrorCode::InvalidInput,
                "Activation route or parameters are invalid",
            ));
        }
        Ok(())
    }
}
impl fmt::Debug for ActivationTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationTarget([redacted])")
    }
}
#[derive(Clone, Default)]
pub struct RouteRegistry(BTreeSet<(String, u32)>);
impl RouteRegistry {
    pub fn register(&mut self, route: &str, version: u32) -> Result<(), SystemError> {
        if !identifier(route) || version == 0 {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Registered route is invalid",
            ));
        }
        self.0.insert((route.to_owned(), version));
        Ok(())
    }
    pub fn validate(&self, target: &ActivationTarget) -> Result<(), SystemError> {
        target.validate_shape()?;
        if !self.0.contains(&(target.route.clone(), target.version)) {
            return Err(error(
                ErrorCode::InvalidConfiguration,
                "Activation route version is not registered",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationSource {
    ColdLaunch,
    WarmLaunch,
    Notification,
    Forwarded,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationPayload {
    OpenUrl(AllowedUrl),
    Route(ActivationTarget),
    Notification {
        notification_id: ActivationId,
        target: ActivationTarget,
    },
    Activate,
}
impl fmt::Debug for ActivationPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ActivationPayload([redacted])")
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationEvent {
    id: ActivationId,
    source: ActivationSource,
    payload: ActivationPayload,
    received_ms: u64,
}
impl ActivationEvent {
    pub fn new(
        id: ActivationId,
        source: ActivationSource,
        payload: ActivationPayload,
        received_at: SystemTime,
    ) -> Result<Self, SystemError> {
        Ok(Self {
            id,
            source,
            payload,
            received_ms: timestamp(received_at)?,
        })
    }
    pub fn id(&self) -> &ActivationId {
        &self.id
    }
    pub fn source(&self) -> ActivationSource {
        self.source
    }
    pub fn payload(&self) -> &ActivationPayload {
        &self.payload
    }
    pub fn received_millis(&self) -> u64 {
        self.received_ms
    }
    pub(crate) fn validate(
        &self,
        urls: &UrlPolicy,
        routes: &RouteRegistry,
    ) -> Result<(), SystemError> {
        ActivationId::new(self.id.0.clone())?;
        match &self.payload {
            ActivationPayload::OpenUrl(url) => urls.validate(url),
            ActivationPayload::Route(target) => routes.validate(target),
            ActivationPayload::Notification {
                notification_id,
                target,
            } => {
                ActivationId::new(notification_id.0.clone())?;
                routes.validate(target)
            }
            ActivationPayload::Activate => Ok(()),
        }
    }
}
impl fmt::Debug for ActivationEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActivationEvent")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
pub(crate) fn timestamp(time: SystemTime) -> Result<u64, SystemError> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or_else(|| {
            error(
                ErrorCode::InvalidInput,
                "Activation timestamp is outside the supported range",
            )
        })
}
