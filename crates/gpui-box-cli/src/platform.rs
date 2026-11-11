use crate::config::AppConfig;
use serde::Serialize;
use std::{env, fs, path::Path};

#[derive(Debug, Serialize)]
pub struct PlatformInventory {
    pub schema_version: u32,
    pub cli_version: &'static str,
    pub upstream_revision: &'static str,
    pub host: String,
    pub project_present: bool,
    pub readiness_scope: &'static str,
    pub platforms: Vec<PlatformEntry>,
}

#[derive(Debug, Serialize)]
pub struct PlatformEntry {
    pub id: &'static str,
    pub cli_target: Option<&'static str>,
    pub configuration_generation: bool,
    pub build_adapter: bool,
    pub compatible_host: bool,
    pub project_enabled: Option<bool>,
    pub toolchain_readiness: &'static str,
    pub backend_maturity: &'static str,
    pub evidence: &'static str,
    pub remaining_acceptance: &'static str,
}

pub fn inventory(root: &Path) -> Result<PlatformInventory, String> {
    let config = match fs::symlink_metadata(root.join("gpui-box.json")) {
        Ok(_) => Some(AppConfig::load(root)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Inspect project configuration: {error}")),
    };
    Ok(report(
        env::consts::OS,
        config.as_ref().map(|config| config.targets.as_slice()),
    ))
}

struct PlatformPolicy {
    id: &'static str,
    cli_target: Option<&'static str>,
    hosts: &'static [&'static str],
    maturity: &'static str,
    evidence: &'static str,
    remaining_acceptance: &'static str,
}

const POLICIES: &[PlatformPolicy] = &[
    PlatformPolicy {
        id: "windows",
        cli_target: Some("desktop"),
        hosts: &["windows"],
        maturity: "experimental",
        evidence: "docs/M6_CI_VALIDATION.md",
        remaining_acceptance: "MSVC/SDK prerequisites, desktop interaction, native installer/update and assistive-reader qualification",
    },
    PlatformPolicy {
        id: "macos",
        cli_target: Some("desktop"),
        hosts: &["macos"],
        maturity: "unverified",
        evidence: "docs/M3_DEVELOPER_WORKFLOW.md",
        remaining_acceptance: "Apple host/toolchain build, runtime, signing, packaging and assistive-reader qualification",
    },
    PlatformPolicy {
        id: "linux",
        cli_target: Some("desktop"),
        hosts: &["linux"],
        maturity: "experimental",
        evidence: "docs/M6_CI_VALIDATION.md",
        remaining_acceptance: "Native libraries, interactive X11/Wayland, packaging/update and assistive-reader qualification",
    },
    PlatformPolicy {
        id: "android",
        cli_target: Some("android"),
        hosts: &["windows", "linux", "macos"],
        maturity: "experimental",
        evidence: "docs/M6_CI_VALIDATION.md",
        remaining_acceptance: "API35/compileSdk36 SDK/NDK/JDK prerequisites, physical devices, reader speech and release/install/update journey",
    },
    PlatformPolicy {
        id: "ios",
        cli_target: None,
        hosts: &["macos"],
        maturity: "unverified",
        evidence: "docs/REPOSITORY_LAYOUT.md",
        remaining_acceptance: "Official backend source is copied. CLI host generation/build adapter and Apple runtime/signing acceptance remain in M8",
    },
    PlatformPolicy {
        id: "web",
        cli_target: None,
        hosts: &["windows", "linux", "macos"],
        maturity: "unsupported",
        evidence: "docs/M3_DEVELOPER_WORKFLOW.md",
        remaining_acceptance: "Web backend and CLI generation/build/run adapter require their own qualification",
    },
];

fn report(host: &str, targets: Option<&[String]>) -> PlatformInventory {
    let platforms = POLICIES
        .iter()
        .map(|policy| PlatformEntry {
            id: policy.id,
            cli_target: policy.cli_target,
            configuration_generation: policy.cli_target.is_some(),
            build_adapter: policy.cli_target.is_some(),
            compatible_host: policy.hosts.contains(&host),
            project_enabled: targets.map(|targets| {
                policy
                    .cli_target
                    .is_some_and(|target| targets.iter().any(|configured| configured == target))
            }),
            toolchain_readiness: "unprobed",
            backend_maturity: policy.maturity,
            evidence: policy.evidence,
            remaining_acceptance: policy.remaining_acceptance,
        })
        .collect();
    PlatformInventory {
        schema_version: 1,
        cli_version: gpui_box::VERSION,
        upstream_revision: gpui_box::UPSTREAM_REVISION,
        host: format!("{}-{host}", env::consts::ARCH),
        project_present: targets.is_some(),
        readiness_scope: "configuration_and_host_policy. Use doctor for declared toolchain prerequisites",
        platforms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_target_policy_keeps_generation_build_and_prerequisites_separate() {
        for host in ["windows", "linux", "macos", "android", "unknown"] {
            let report = report(host, Some(&["desktop".into()]));
            assert_eq!(report.platforms.len(), 6);
            for platform in report.platforms {
                assert_eq!(platform.toolchain_readiness, "unprobed");
                if matches!(platform.id, "windows" | "linux" | "macos") {
                    assert!(platform.configuration_generation && platform.build_adapter);
                    assert_eq!(platform.compatible_host, host == platform.id);
                    assert_eq!(platform.project_enabled, Some(true));
                } else {
                    assert_eq!(platform.project_enabled, Some(false));
                }
                if platform.id == "android" {
                    assert_eq!(
                        platform.compatible_host,
                        matches!(host, "windows" | "linux" | "macos")
                    );
                }
                if matches!(platform.id, "ios" | "web") {
                    assert!(!platform.configuration_generation && !platform.build_adapter);
                }
            }
        }
        assert!(
            report("windows", None)
                .platforms
                .iter()
                .all(|platform| platform.project_enabled.is_none())
        );
    }
}
