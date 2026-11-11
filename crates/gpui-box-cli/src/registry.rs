use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const BUNDLED: &[u8] = include_bytes!("../../../registry/official-v1.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub sdk_version: String,
    pub upstream_revision: String,
    pub rust_version: String,
    pub source_revision: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub version: String,
    pub title: String,
    pub kind: Kind,
    pub delivery: Delivery,
    pub license: String,
    pub sdk_version: String,
    pub upstream_revision: String,
    pub sources: Vec<Source>,
    pub dependencies: Vec<Dependency>,
    pub platforms: BTreeMap<String, PlatformEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Control,
    Theme,
    Template,
    Capability,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Package,
    Source,
    Capability,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformEvidence {
    pub status: Maturity,
    pub scope: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Maturity {
    Experimental,
    Unverified,
    Unsupported,
}

#[derive(Serialize)]
pub struct RegistryView {
    pub schema_version: u32,
    pub snapshot_sha256: String,
    pub sdk_version: String,
    pub upstream_revision: String,
    pub source_revision: String,
    pub source_repository: &'static str,
    pub installation_tracking: &'static str,
    pub entries: Vec<Entry>,
}

#[derive(Serialize)]
pub struct Resolution {
    pub schema_version: u32,
    pub snapshot_sha256: String,
    pub sdk_version: String,
    pub upstream_revision: String,
    pub rust_version: String,
    pub source_revision: String,
    pub source_repository: &'static str,
    pub requested: Vec<String>,
    /// Dependencies precede their consumers, with lexical order as the tie-break.
    pub entries: Vec<Entry>,
    pub installation_applied: bool,
}

pub fn bundled() -> Result<Snapshot, String> {
    Snapshot::parse(BUNDLED)
}

fn digest() -> String {
    format!("{:x}", Sha256::digest(BUNDLED))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn identity(value: &str) -> bool {
    let parts = value.split('/').collect::<Vec<_>>();
    parts.len() == 3
        && value.len() <= 128
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.as_bytes()[0].is_ascii_lowercase()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn relative_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        })
}

impl Snapshot {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("Registry snapshot exceeds 1 MiB".into());
        }
        let snapshot: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("Invalid registry snapshot: {error}"))?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.sdk_version != gpui_box::VERSION
            || self.upstream_revision != gpui_box::UPSTREAM_REVISION
            || self.rust_version != "1.98.1"
            || !hex(&self.source_revision, 40)
            || self.entries.is_empty()
            || self.entries.len() > 256
        {
            return Err("Registry schema, SDK, Rust or GPUI identity is incompatible".into());
        }
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            if !identity(&entry.id) || !ids.insert(&entry.id) {
                return Err(format!(
                    "Invalid or duplicate registry identity {}",
                    entry.id
                ));
            }
            let kind = match entry.kind {
                Kind::Control => "control",
                Kind::Theme => "theme",
                Kind::Template => "template",
                Kind::Capability => "capability",
            };
            if entry.id.split('/').nth(1) != Some(kind) {
                return Err(format!("Registry identity kind mismatch for {}", entry.id));
            }
            if entry.version != self.sdk_version
                || entry.sdk_version != self.sdk_version
                || entry.upstream_revision != self.upstream_revision
                || entry.license != "Apache-2.0"
                || entry.title.is_empty()
                || entry.title.len() > 128
                || entry.title.chars().any(char::is_control)
            {
                return Err(format!("Incompatible entry metadata for {}", entry.id));
            }
            if !matches!(
                (&entry.kind, &entry.delivery),
                (Kind::Control | Kind::Theme, Delivery::Package)
                    | (Kind::Template, Delivery::Source)
                    | (Kind::Capability, Delivery::Capability)
            ) {
                return Err(format!("Incompatible delivery mode for {}", entry.id));
            }
            let mut paths = BTreeSet::new();
            if entry.sources.is_empty() || entry.sources.len() > 32 {
                return Err(format!(
                    "Missing or excessive source metadata for {}",
                    entry.id
                ));
            }
            for source in &entry.sources {
                if !relative_path(&source.path)
                    || !hex(&source.sha256, 64)
                    || !paths.insert(&source.path)
                {
                    return Err(format!("Invalid source metadata for {}", entry.id));
                }
            }
            let mut dependencies = BTreeSet::new();
            if entry.dependencies.len() > 32 {
                return Err(format!("Excessive dependency metadata for {}", entry.id));
            }
            for dependency in &entry.dependencies {
                if !identity(&dependency.id)
                    || dependency.version.is_empty()
                    || dependency.version.len() > 64
                    || !dependencies.insert(&dependency.id)
                {
                    return Err(format!("Invalid dependency metadata for {}", entry.id));
                }
            }
            let platforms = ["android", "ios", "linux", "macos", "web", "windows"];
            if entry
                .platforms
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>()
                != platforms
            {
                return Err(format!("Incomplete platform evidence for {}", entry.id));
            }
            for evidence in entry.platforms.values() {
                if evidence.scope.is_empty()
                    || evidence.scope.len() > 512
                    || evidence.scope.chars().any(char::is_control)
                    || !relative_path(&evidence.evidence)
                {
                    return Err(format!("Invalid platform evidence for {}", entry.id));
                }
            }
        }
        Ok(())
    }

    pub fn view(&self, item: Option<&str>) -> Result<RegistryView, String> {
        let mut entries = self.entries.clone();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        if let Some(item) = item {
            entries.retain(|entry| entry.id == item);
            if entries.is_empty() {
                return Err(format!("Unknown registry item {item}"));
            }
        }
        Ok(RegistryView {
            schema_version: 1,
            snapshot_sha256: digest(),
            sdk_version: self.sdk_version.clone(),
            upstream_revision: self.upstream_revision.clone(),
            source_revision: self.source_revision.clone(),
            source_repository: "https://github.com/jihuayu/gpui-box",
            installation_tracking: "unavailable",
            entries,
        })
    }

    pub fn resolve(&self, requested: &[String]) -> Result<Resolution, String> {
        self.validate()?;
        if requested.is_empty() || requested.len() > 256 {
            return Err("Resolve requires 1–256 registry item identities".into());
        }
        let entries = self
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        let requested = requested.iter().cloned().collect::<BTreeSet<_>>();
        let mut resolved = BTreeSet::new();
        let mut ordered = Vec::new();
        for id in &requested {
            visit(id, &entries, &mut Vec::new(), &mut resolved, &mut ordered)?;
        }
        Ok(Resolution {
            schema_version: 1,
            snapshot_sha256: digest(),
            sdk_version: self.sdk_version.clone(),
            upstream_revision: self.upstream_revision.clone(),
            rust_version: self.rust_version.clone(),
            source_revision: self.source_revision.clone(),
            source_repository: "https://github.com/jihuayu/gpui-box",
            requested: requested.into_iter().collect(),
            entries: ordered,
            installation_applied: false,
        })
    }
}

fn visit(
    id: &str,
    entries: &BTreeMap<&str, &Entry>,
    chain: &mut Vec<String>,
    resolved: &mut BTreeSet<String>,
    ordered: &mut Vec<Entry>,
) -> Result<(), String> {
    if chain.iter().any(|ancestor| ancestor == id) {
        chain.push(id.into());
        return Err(format!("Registry dependency cycle {}", chain.join(" -> ")));
    }
    if resolved.contains(id) {
        return Ok(());
    }
    chain.push(id.into());
    let entry = entries
        .get(id)
        .ok_or_else(|| format!("Missing registry dependency {}", chain.join(" -> ")))?;
    let mut dependencies = entry.dependencies.iter().collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.id.cmp(&right.id));
    for dependency in dependencies {
        if let Some(target) = entries.get(dependency.id.as_str())
            && dependency.version != target.version
        {
            return Err(format!(
                "Registry version conflict {} -> {} requires {}, bundled {}",
                chain.join(" -> "),
                dependency.id,
                dependency.version,
                target.version
            ));
        }
        visit(&dependency.id, entries, chain, resolved, ordered)?;
    }
    chain.pop();
    resolved.insert(id.into());
    ordered.push((*entry).clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    #[test]
    fn official_sources_and_evidence_match_the_distribution() {
        let snapshot = bundled().unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for entry in &snapshot.entries {
            for source in &entry.sources {
                let bytes = fs::read(root.join(&source.path)).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(bytes)),
                    source.sha256,
                    "{}",
                    source.path
                );
            }
            for evidence in entry.platforms.values() {
                assert!(root.join(&evidence.evidence).is_file());
            }
        }
        let ids = snapshot
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        assert_eq!(snapshot.resolve(&ids).unwrap().entries.len(), ids.len());
    }

    #[test]
    fn closure_is_deterministic_and_shared_dependencies_are_unique() {
        let snapshot = bundled().unwrap();
        let requests = [
            "official/template/workbench",
            "official/control/button",
            "official/template/component",
        ];
        let first = snapshot.resolve(&requests.map(str::to_owned)).unwrap();
        let mut reverse = requests.map(str::to_owned).to_vec();
        reverse.reverse();
        reverse.push(requests[0].into());
        let second = snapshot.resolve(&reverse).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(second).unwrap()
        );
        let ids = first
            .entries
            .iter()
            .map(|entry| &entry.id)
            .collect::<Vec<_>>();
        assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
        for (index, entry) in first.entries.iter().enumerate() {
            for dependency in &entry.dependencies {
                assert!(ids[..index].contains(&&dependency.id));
            }
        }
    }

    #[test]
    fn missing_dependencies_and_cycles_report_the_complete_chain() {
        let mut snapshot = bundled().unwrap();
        let id = snapshot.entries[0].id.clone();
        snapshot.entries[0].dependencies = vec![Dependency {
            id: "official/control/missing".into(),
            version: snapshot.sdk_version.clone(),
        }];
        let error = snapshot.resolve(std::slice::from_ref(&id)).err().unwrap();
        assert!(error.contains(&format!("{id} -> official/control/missing")));
        snapshot.entries[0].dependencies[0].id = id.clone();
        let error = snapshot.resolve(std::slice::from_ref(&id)).err().unwrap();
        assert!(error.contains(&format!("{id} -> {id}")));
        snapshot.entries[0].dependencies[0].version = "2.0.0".into();
        let error = snapshot.resolve(std::slice::from_ref(&id)).err().unwrap();
        assert!(error.contains("requires 2.0.0, bundled 0.1.0-dev"));
    }

    #[test]
    fn incompatible_or_incomplete_metadata_is_rejected() {
        let original: serde_json::Value = serde_json::from_slice(BUNDLED).unwrap();
        for (pointer, value) in [
            ("/schema_version", serde_json::json!(2)),
            ("/sdk_version", serde_json::json!("2.0.0")),
            ("/upstream_revision", serde_json::json!("a".repeat(40))),
            (
                "/entries/0/upstream_revision",
                serde_json::json!("a".repeat(40)),
            ),
            ("/entries/0/sources", serde_json::json!([])),
            ("/entries/0/sources/0/path", serde_json::json!("../outside")),
            ("/entries/0/sources/0/sha256", serde_json::json!("invalid")),
            ("/entries/0/platforms", serde_json::json!({})),
        ] {
            let mut modified = original.clone();
            *modified.pointer_mut(pointer).unwrap() = value;
            assert!(
                Snapshot::parse(&serde_json::to_vec(&modified).unwrap()).is_err(),
                "{pointer}"
            );
        }
        let mut duplicate = bundled().unwrap();
        duplicate.entries.push(duplicate.entries[0].clone());
        assert!(duplicate.validate().is_err());
        assert!(Snapshot::parse(&vec![b' '; 1024 * 1024 + 1]).is_err());
    }
}
