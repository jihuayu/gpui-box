use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

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
}
fn default_assets() -> Vec<PathBuf> {
    vec![PathBuf::from("assets")]
}
impl AppConfig {
    pub fn load(root: &Path) -> Result<Self, String> {
        let path = root.join("gpui-box.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("Read {}: {e}", path.display()))?;
        let config: Self =
            serde_json::from_str(&text).map_err(|e| format!("Invalid gpui-box.json: {e}"))?;
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
            return Err("distribution must point to a gpui-box source checkout".into());
        }
        Ok(path)
    }
}
