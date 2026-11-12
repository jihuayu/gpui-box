use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const SCHEMA: &str = include_str!("../../../schemas/gpui-box.project.v1.schema.json");

pub fn schema() -> serde_json::Value {
    serde_json::from_str(SCHEMA).expect("embedded project schema must be valid JSON")
}

pub fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| format!("Open {}: {error}", path.display()))?
        .take(MAX_CONFIG_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read {}: {error}", path.display()))?;
    if bytes.len() > MAX_CONFIG_BYTES {
        return Err("Project configuration exceeds 1 MiB".into());
    }
    Ok(bytes)
}

pub fn validation_report(root: &Path) -> Result<serde_json::Value, String> {
    let bytes = read_bytes(&root.join("gpui-box.json"))?;
    let config = AppConfig::from_bytes(&bytes)?;
    Ok(serde_json::json!({
        "schema_version": 1,
        "valid": true,
        "cli_version": gpui_box::VERSION,
        "upstream_revision": gpui_box::UPSTREAM_REVISION,
        "project_schema_version": config.schema_version,
        "service_schema_version": config.services.schema_version,
        "configuration_path": "gpui-box.json",
        "configuration_sha256": format!("{:x}", Sha256::digest(&bytes)),
        "configuration_bytes": bytes.len(),
        "schema_id": schema()["$id"],
        "schema_sha256": format!("{:x}", Sha256::digest(SCHEMA.as_bytes())),
        "normalized_configuration": config,
        "precedence": ["project gpui-box.json", "built-in optional-field defaults"],
        "local_overrides": "unsupported",
        "installation_lock": "unsupported",
        "validation_scope": "Project and service syntax/semantics. Distribution existence, SDK integrity, asset contents, toolchains, native providers and runtime are evaluated by their workflow gates",
        "runtime_executed": false,
    }))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u32,
    pub name: String,
    pub application_id: String,
    pub version: String,
    pub version_code: u32,
    pub distribution: PathBuf,
    pub targets: Vec<String>,
    pub update_channel: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "default_assets")]
    pub assets: Vec<PathBuf>,
    #[serde(default)]
    pub services: Services,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Services {
    #[serde(default = "service_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub permissions: Vec<PermissionDeclaration>,
    #[serde(default)]
    pub url_schemes: Vec<String>,
}
fn service_version() -> u32 {
    1
}
impl Default for Services {
    fn default() -> Self {
        Self {
            schema_version: 1,
            permissions: Vec::new(),
            url_schemes: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDeclaration {
    pub kind: PermissionKind,
    pub purpose: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    Notifications,
    Camera,
    Microphone,
    LocationWhenInUse,
    Contacts,
}
impl PermissionKind {
    pub fn android_permissions(self) -> &'static [&'static str] {
        match self {
            Self::Notifications => &["android.permission.POST_NOTIFICATIONS"],
            Self::Camera => &["android.permission.CAMERA"],
            Self::Microphone => &["android.permission.RECORD_AUDIO"],
            Self::LocationWhenInUse => &[
                "android.permission.ACCESS_COARSE_LOCATION",
                "android.permission.ACCESS_FINE_LOCATION",
            ],
            Self::Contacts => &["android.permission.READ_CONTACTS"],
        }
    }
}
impl Services {
    fn validate(&self, targets: &[String]) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("services.schema_version must be 1".into());
        }
        if (!self.permissions.is_empty() || !self.url_schemes.is_empty())
            && !targets.iter().any(|target| target == "android")
        {
            return Err("Native service declarations currently require the android target".into());
        }
        let mut permissions = BTreeSet::new();
        for declaration in &self.permissions {
            if !permissions.insert(declaration.kind) {
                return Err("services.permissions kinds must be unique".into());
            }
            if declaration.purpose.trim().is_empty()
                || declaration.purpose.len() > 512
                || declaration.purpose.chars().any(char::is_control)
            {
                return Err("services.permissions purpose must contain 1–512 printable bytes describing the contextual request".into());
            }
        }
        let mut schemes = BTreeSet::new();
        for scheme in &self.url_schemes {
            if scheme.len() > 64
                || !scheme.starts_with(|character: char| character.is_ascii_lowercase())
                || !scheme.bytes().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || b"+.-".contains(&character)
                })
                || matches!(
                    scheme.as_str(),
                    "http"
                        | "https"
                        | "file"
                        | "javascript"
                        | "data"
                        | "shell"
                        | "content"
                        | "intent"
                )
            {
                return Err("services.url_schemes requires lowercase custom schemes. Verified web links and document/resource schemes require their native provider configuration".into());
            }
            if !schemes.insert(scheme) {
                return Err("services.url_schemes must be unique".into());
            }
        }
        Ok(())
    }
}
fn default_assets() -> Vec<PathBuf> {
    vec![PathBuf::from("assets")]
}
impl AppConfig {
    pub fn load(root: &Path) -> Result<Self, String> {
        Self::from_bytes(&read_bytes(&root.join("gpui-box.json"))?)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err("Project configuration exceeds 1 MiB".into());
        }
        let config: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid gpui-box.json: {error}"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("schema_version must be 1".into());
        }
        if self.name.trim().is_empty() || self.name.chars().any(char::is_control) {
            return Err("name must contain printable text".into());
        }
        if self.application_id.split('.').count() < 2
            || !self.application_id.split('.').all(|part| {
                !part.is_empty()
                    && part.as_bytes()[0].is_ascii_alphabetic()
                    && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        {
            return Err(
                "application_id must be a dotted identifier such as dev.example.app".into(),
            );
        }
        if self.version_code == 0
            || self.version_code > 2_100_000_000
            || self.version.split('.').count() != 3
            || !self.version.split('.').all(|part| {
                !part.is_empty()
                    && part.bytes().all(|byte| byte.is_ascii_digit())
                    && (part == "0" || !part.starts_with('0'))
                    && part.parse::<u32>().is_ok()
            })
        {
            return Err(
                "version must be numeric major.minor.patch and version_code must be positive"
                    .into(),
            );
        }
        if self.targets.is_empty()
            || self
                .targets
                .iter()
                .any(|target| !matches!(target.as_str(), "desktop" | "android"))
        {
            return Err("targets supports desktop and android in this template version".into());
        }
        if self.targets.iter().collect::<BTreeSet<_>>().len() != self.targets.len() {
            return Err("targets must be unique".into());
        }
        if self.update_channel != "development" {
            return Err("Only the development channel is implemented. Signed release/update channels are tracked in M4/M5.".into());
        }
        self.services.validate(&self.targets)?;
        if self
            .permissions
            .iter()
            .any(|permission| permission != "internet")
        {
            return Err("permissions currently supports internet. Other native providers require their configuration milestone.".into());
        }
        for path in &self.assets {
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err(
                    "assets must contain project-relative paths without parent traversal".into(),
                );
            }
        }
        Ok(())
    }
    pub fn distribution(&self, root: &Path) -> Result<PathBuf, String> {
        let path = root
            .join(&self.distribution)
            .canonicalize()
            .map_err(|e| format!("Distribution path is unavailable: {e}"))?;
        if !path.join("vendor/gpui/UPSTREAM.json").is_file() {
            return Err(
                "distribution must point to a gpui-box source checkout or exported SDK".into(),
            );
        }
        if std::fs::symlink_metadata(path.join("gpui-box-sdk.json")).is_ok() {
            crate::sdk::verify(&path)?;
        }
        Ok(path)
    }
}
