//! Real-tool feature qualification, separate from the ordinary CLI fixture suite.
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

struct Matrix {
    source: PathBuf,
    reports: PathBuf,
    commands: Vec<Value>,
    cases: Vec<Value>,
}

impl Matrix {
    fn run(&mut self, label: &str, arguments: &[String]) -> Result<Vec<u8>, String> {
        let stdout = self.reports.join(format!("{label}.stdout.log"));
        let stderr = self.reports.join(format!("{label}.stderr.log"));
        let started = Instant::now();
        // Files avoid waiting for pipe EOF held by a tool's background helpers.
        let status = Command::new("cargo")
            .current_dir(&self.source)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(fs::File::create(&stdout).map_err(|error| error.to_string())?)
            .stderr(fs::File::create(&stderr).map_err(|error| error.to_string())?)
            .status()
            .map_err(|error| format!("Start {label}: {error}"))?;
        self.commands.push(json!({
            "label": label, "program": "cargo", "arguments": arguments,
            "exit_code": status.code(), "elapsed_ms": started.elapsed().as_millis(),
        }));
        if !status.success() {
            return Err(format!("{label} failed. See {}", stderr.display()));
        }
        fs::read(stdout).map_err(|error| error.to_string())
    }

    fn qualify(&mut self, target: &str, host: &str) -> Result<Value, String> {
        self.run("cargo-version", &words(&["--version", "--verbose"]))?;
        let metadata = self.run(
            "metadata",
            &words(&["metadata", "--locked", "--no-deps", "--format-version", "1"]),
        )?;
        let metadata: Value =
            serde_json::from_slice(&metadata).map_err(|error| error.to_string())?;
        let facade = metadata["packages"]
            .as_array()
            .ok_or("Missing packages")?
            .iter()
            .find(|package| package["name"] == "gpui-box")
            .ok_or("Missing facade package")?;
        let features = facade["features"]
            .as_object()
            .ok_or("Missing facade features")?;
        if features.get("default") != Some(&json!([])) {
            return Err("Facade default features must remain empty".into());
        }
        let capability_features = features
            .keys()
            .filter(|feature| !matches!(feature.as_str(), "default" | "desktop" | "android"))
            .cloned()
            .collect::<Vec<_>>();
        let contracts = capability_features
            .iter()
            .filter(|feature| !feature.ends_with("-gpui"))
            .cloned()
            .collect::<Vec<_>>();
        let mut selections = vec![("none".to_string(), Vec::new(), false)];
        for feature in &capability_features {
            selections.push((
                feature.clone(),
                vec![feature.clone()],
                feature.ends_with("-gpui"),
            ));
        }
        selections.push(("all-contracts".into(), contracts, false));
        let platform = if target == "android" {
            "android"
        } else {
            "desktop"
        };
        selections.push((platform.into(), vec![platform.into()], true));
        let mut all = capability_features.clone();
        all.push(platform.into());
        selections.push(("all-adapters".into(), all, true));
        let targets = if target == "android" {
            vec![
                ("arm64-v8a", "aarch64-linux-android"),
                ("x86_64", "x86_64-linux-android"),
            ]
        } else {
            vec![("host", host)]
        };
        let engine = self
            .source
            .join("vendor/gpui/crates/gpui")
            .canonicalize()
            .map_err(|error| error.to_string())?;
        for (abi, triple) in targets {
            for (name, enabled, expects_gpui) in &selections {
                let label = format!("{abi}-{name}");
                let mut tree = words(&[
                    "tree",
                    "--locked",
                    "-p",
                    "gpui-box",
                    "--no-default-features",
                    "--edges",
                    "normal",
                    "--prefix",
                    "none",
                    "--format",
                    "{p}",
                ]);
                tree.extend(words(&["--target", triple]));
                if !enabled.is_empty() {
                    tree.extend(["--features".into(), enabled.join(",")]);
                }
                let graph = self.run(&format!("{label}-tree"), &tree)?;
                let graph = std::str::from_utf8(&graph).map_err(|error| error.to_string())?;
                let packages = audit_graph(graph, *expects_gpui, &engine)?;
                if target == "android"
                    && packages.iter().any(|package| {
                        matches!(
                            package.split_whitespace().next(),
                            Some(
                                "gpui_platform"
                                    | "gpui_windows"
                                    | "gpui_linux"
                                    | "gpui_macos"
                                    | "gpui_apple"
                                    | "gpui_ios"
                                    | "gpui_web"
                            )
                        )
                    })
                {
                    return Err(format!("{label} includes an unrelated platform backend"));
                }
                let mut check = if target == "android" {
                    words(&["ndk", "--platform", "35", "-t", abi, "check"])
                } else {
                    words(&["check", "--target", triple])
                };
                check.extend(words(&[
                    "--locked",
                    "-p",
                    "gpui-box",
                    "--lib",
                    "--no-default-features",
                ]));
                if !enabled.is_empty() {
                    check.extend(["--features".into(), enabled.join(",")]);
                }
                self.run(&format!("{label}-check"), &check)?;
                self.cases.push(json!({
                    "name": name, "abi": abi, "target": triple,
                    "features": enabled, "normal_packages": packages, "gpui_expected": expects_gpui,
                    "compiled": true,
                }));
                println!("Feature qualification passed {label}");
            }
        }
        Ok(
            json!({"facade_version": facade["version"], "capability_features": capability_features,
            "selections_per_target": selections.len()}),
        )
    }
}

fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

fn audit_graph(text: &str, expects_gpui: bool, engine: &Path) -> Result<Vec<String>, String> {
    let packages = text
        .lines()
        .map(|line| line.trim_end_matches(" (*)").to_string())
        .collect::<BTreeSet<_>>();
    let mut engines = BTreeSet::new();
    for package in &packages {
        let name = package
            .split_whitespace()
            .next()
            .ok_or("Empty Cargo tree package")?;
        if name == "gpui-pre" || name.starts_with("gpui-pre-") {
            return Err(format!("Uncoordinated GPUI package identity {package}"));
        }
        if !expects_gpui
            && matches!(
                name,
                "gpui"
                    | "gpui_platform"
                    | "gpui-android"
                    | "gpui_wgpu"
                    | "gpui_windows"
                    | "gpui_macos"
                    | "gpui_linux"
                    | "gpui_apple"
                    | "gpui_ios"
                    | "gpui_web"
            )
        {
            return Err(format!("Contract feature unexpectedly includes {package}"));
        }
        if name == "gpui" {
            engines.insert(package);
        }
    }
    if expects_gpui {
        if engines.len() != 1 {
            return Err(format!(
                "Expected one GPUI identity, found {}",
                engines.len()
            ));
        }
        let identity = engines.first().ok_or("Missing GPUI identity")?;
        let path = identity
            .split_once(" (")
            .and_then(|(_, path)| path.strip_suffix(')'))
            .ok_or("GPUI must resolve to the copied source path")?;
        if Path::new(path)
            .canonicalize()
            .map_err(|error| error.to_string())?
            != engine
        {
            return Err(format!(
                "GPUI source differs from the copied distribution engine: {identity}"
            ));
        }
    }
    Ok(packages.into_iter().collect())
}

#[test]
fn graph_audit_rejects_backend_leaks_and_multiple_or_foreign_engine_identities() {
    let root = tempfile::tempdir().unwrap();
    let engine = root.path().canonicalize().unwrap();
    assert!(audit_graph("gpui-box v0.1.0\ngpui-system v0.1.0", false, &engine).is_ok());
    for leaked in [
        "gpui",
        "gpui_platform",
        "gpui_linux",
        "gpui-pre",
        "gpui_wgpu",
        "gpui-android",
    ] {
        assert!(audit_graph(&format!("{leaked} v0.1.0"), false, &engine).is_err());
    }
    let graph = format!("gpui v0.2.2 ({})", engine.display());
    assert!(audit_graph(&format!("{graph}\n{graph} (*)"), true, &engine).is_ok());
    assert!(audit_graph("gpui v0.2.2", true, &engine).is_err());
    assert!(audit_graph(&format!("{graph}\ngpui v0.3.0"), true, &engine).is_err());
    assert!(audit_graph("gpui-system v0.1.0", true, &engine).is_err());
    let foreign = tempfile::tempdir().unwrap();
    assert!(
        audit_graph(
            &format!("gpui v0.2.2 ({})", foreign.path().display()),
            true,
            &engine
        )
        .is_err()
    );
}

#[test]
#[ignore = "Real per-feature builds require installed host or Android native toolchains"]
fn facade_features_compile_independently_with_coordinated_dependencies() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let target = env::var("GPUI_BOX_FEATURE_TARGET").unwrap_or_else(|_| "host".into());
    assert!(matches!(target.as_str(), "host" | "android"));
    let reports = env::var_os("GPUI_BOX_FEATURE_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| source.join("target/ci/feature-qualification"));
    fs::create_dir_all(reports.parent().unwrap()).unwrap();
    fs::create_dir(&reports).expect("Feature report directory must be fresh");
    let revision = Command::new("git")
        .current_dir(&source)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(revision.status.success());
    let rust = Command::new("rustc")
        .args(["--version", "--verbose"])
        .output()
        .unwrap();
    assert!(rust.status.success());
    let rust_identity = String::from_utf8(rust.stdout).unwrap();
    let host = rust_identity
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .expect("rustc verbose identity must contain its host triple");
    let mut matrix = Matrix {
        source,
        reports: reports.clone(),
        commands: Vec::new(),
        cases: Vec::new(),
    };
    let result = matrix.qualify(&target, host);
    let receipt = json!({
        "schema_version": 1, "success": result.is_ok(), "target": target,
        "host": format!("{}-{}", env::consts::ARCH, env::consts::OS),
        "source_revision": String::from_utf8_lossy(&revision.stdout).trim(),
        "upstream_revision": gpui_box::UPSTREAM_REVISION,
        "rust": rust_identity, "qualification": result.as_ref().ok(),
        "error": result.as_ref().err(), "cases": matrix.cases, "commands": matrix.commands,
        "runtime_executed": false,
    });
    fs::write(
        reports.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    println!("Feature qualification report {}", reports.display());
    result.unwrap();
}
