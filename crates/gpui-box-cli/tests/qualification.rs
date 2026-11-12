//! Explicit, real-tool qualification. Regular CLI tests keep their fast fixtures.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Instant,
};

struct Qualification {
    reports: PathBuf,
    build: PathBuf,
    records: Vec<Value>,
}

impl Qualification {
    fn run(&mut self, name: &str, command: &mut Command, success: bool) -> Result<Vec<u8>, String> {
        let started = Instant::now();
        let output = command
            .stdin(Stdio::null())
            .env("CARGO_TARGET_DIR", &self.build)
            .output()
            .map_err(|error| format!("Start {name}: {error}"))?;
        fs::write(
            self.reports.join(format!("{name}.stdout.log")),
            &output.stdout,
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            self.reports.join(format!("{name}.stderr.log")),
            &output.stderr,
        )
        .map_err(|error| error.to_string())?;
        self.records.push(json!({
            "name": name,
            "program": command.get_program().to_string_lossy(),
            "arguments": command.get_args().map(|argument| argument.to_string_lossy()).collect::<Vec<_>>(),
            "directory": command.get_current_dir(),
            "exit_code": output.status.code(),
            "expected_success": success,
            "elapsed_ms": started.elapsed().as_millis(),
        }));
        if output.status.success() != success {
            return Err(format!(
                "Unexpected {name} result. See {}",
                self.reports.display()
            ));
        }
        Ok(output.stdout)
    }

    fn cli(
        &mut self,
        name: &str,
        root: &Path,
        arguments: &[&str],
        success: bool,
    ) -> Result<Value, String> {
        let bytes = self.run(
            name,
            Command::new(env!("CARGO_BIN_EXE_gpui-box"))
                .current_dir(root)
                .arg("--json")
                .args(arguments),
            success,
        )?;
        let response: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if response["schema_version"] != 1 || response["success"] != success {
            return Err(format!("Unexpected {name} CLI envelope"));
        }
        if success {
            Ok(response["result"].clone())
        } else {
            Ok(response["error"].clone())
        }
    }

    fn qualify(&mut self, source: &Path, workspace: &Path, target: &str) -> Result<Value, String> {
        let sdk = workspace.join("SDK 有 空格");
        let sdk_path = sdk.to_str().ok_or("SDK path requires UTF-8")?;
        let source_path = source.to_str().ok_or("Source path requires UTF-8")?;
        let manifest = self.cli(
            "sdk-export",
            source,
            &["sdk", "export", sdk_path, "--source", source_path],
            true,
        )?;
        let verified = self.cli(
            "sdk-verify-before",
            source,
            &["sdk", "verify", sdk_path],
            true,
        )?;
        if manifest != verified {
            return Err("Export and verification provenance differ".into());
        }
        let identity = self.cli("cli-info", source, &["info"], true)?;
        if identity["upstream_revision"] != manifest["upstream_revision"] {
            return Err("CLI and SDK GPUI revisions differ".into());
        }
        let cache_path = workspace.join("SDK Cache 有 空格");
        let cache_path_text = cache_path.to_str().ok_or("SDK cache path requires UTF-8")?;
        let cached = self.cli(
            "sdk-cache-add",
            source,
            &["sdk", "cache", "add", sdk_path, "--cache", cache_path_text],
            true,
        )?;
        let address = cached["content_sha256"]
            .as_str()
            .ok_or("Missing SDK cache identity")?;
        let reused = self.cli(
            "sdk-cache-repeat",
            source,
            &["sdk", "cache", "add", sdk_path, "--cache", cache_path_text],
            true,
        )?;
        let verified = self.cli(
            "sdk-cache-verify",
            source,
            &[
                "sdk",
                "cache",
                "verify",
                address,
                "--cache",
                cache_path_text,
            ],
            true,
        )?;
        if cached["created"] != true
            || reused["created"] != false
            || verified["verified"] != true
            || cached["manifest"] != manifest
        {
            return Err("SDK cache identity or idempotence differs".into());
        }
        let cached_path = PathBuf::from(cached["path"].as_str().ok_or("Missing cached SDK path")?);
        let sdk = cached_path.as_path();
        let sdk_path = sdk.to_str().ok_or("Cached SDK path requires UTF-8")?;
        let registry = self.cli("registry-list", workspace, &["list"], true)?;
        let registry_bytes =
            fs::read(sdk.join("registry/official-v1.json")).map_err(|error| error.to_string())?;
        let registry_digest = format!("{:x}", Sha256::digest(&registry_bytes));
        if registry["snapshot_sha256"] != registry_digest {
            return Err("Bundled CLI and exported SDK registry snapshots differ".into());
        }
        let resolution = self.cli(
            "registry-resolve",
            workspace,
            &[
                "registry",
                "resolve",
                "official/template/workbench",
                "official/template/component",
            ],
            true,
        )?;
        if resolution["snapshot_sha256"] != registry_digest
            || resolution["installation_applied"] != false
        {
            return Err("Registry resolution identity or effect differs".into());
        }
        self.run(
            "rust-identity",
            Command::new("mise").current_dir(source).args([
                "exec",
                "--",
                "rustc",
                "--version",
                "--verbose",
            ]),
            true,
        )?;
        self.run(
            "cargo-identity",
            Command::new("mise")
                .current_dir(source)
                .args(["exec", "--", "cargo", "--version"]),
            true,
        )?;
        let mut applications = Vec::new();
        for template in ["starter", "adaptive", "component", "workbench"] {
            for targets in ["desktop", "android", "desktop,android"] {
                let label = format!("{template}-{}", targets.replace(',', "-"));
                let app = workspace.join(format!("Application {label} 有 空格"));
                let app_path = app.to_str().ok_or("Application path requires UTF-8")?;
                self.cli(
                    &format!("{label}-new"),
                    workspace,
                    &[
                        "new",
                        app_path,
                        "--distribution",
                        sdk_path,
                        "--template",
                        template,
                        "--targets",
                        targets,
                    ],
                    true,
                )?;
                let configuration_path = app.join("gpui-box.json");
                let configuration_bytes =
                    fs::read(&configuration_path).map_err(|error| error.to_string())?;
                let configuration = self.cli(
                    &format!("{label}-config-validate"),
                    &app,
                    &["config", "validate"],
                    true,
                )?;
                if configuration["valid"] != true
                    || configuration["configuration_sha256"]
                        != format!("{:x}", Sha256::digest(&configuration_bytes))
                    || fs::read(&configuration_path).map_err(|error| error.to_string())?
                        != configuration_bytes
                {
                    return Err(format!(
                        "{label} configuration validation changed its input or identity"
                    ));
                }
                let provenance_path = app.join(".gpui-box/template.json");
                let provenance_bytes =
                    fs::read(&provenance_path).map_err(|error| error.to_string())?;
                let provenance: Value =
                    serde_json::from_slice(&provenance_bytes).map_err(|error| error.to_string())?;
                if provenance["template"] != template || provenance["template_version"] != 1 {
                    return Err(format!("Unexpected {label} template provenance"));
                }
                let source_path = app.join("app/src/lib.rs");
                let mut customized = fs::read(&source_path).map_err(|error| error.to_string())?;
                customized
                    .extend_from_slice(b"\n// Application-owned qualification customization.\n");
                fs::write(&source_path, &customized).map_err(|error| error.to_string())?;
                let native_path = app.join("android-custom.gradle.kts");
                let native = b"// Application-owned native qualification customization.\n";
                if targets.contains("android") {
                    fs::write(&native_path, native).map_err(|error| error.to_string())?;
                }
                let receipt = self.cli(&format!("{label}-generate"), &app, &["generate"], true)?;
                if receipt["files_written"] != json!([]) || receipt["completed"] != true {
                    return Err(format!("{label} regeneration was not a completed no-op"));
                }
                let mut enabled = targets.split(',').any(|configured| configured == target);
                let mut plan = self.cli(
                    &format!("{label}-plan"),
                    &app,
                    &["build", target, "--dry-run"],
                    enabled,
                )?;
                if !enabled && plan["code"] != "BUILD_PLAN_FAILED" {
                    return Err(format!("{label} accepted a disabled build target"));
                }
                let platform_added = template == "workbench" && !enabled;
                if platform_added {
                    let addition = self.cli(
                        &format!("{label}-platform-preview"),
                        &app,
                        &["platform", "add", target, "--dry-run"],
                        true,
                    )?;
                    if addition["already_enabled"] != false
                        || addition["configuration"]["action"] != "update"
                    {
                        return Err(format!(
                            "{label} platform preview did not enable the target"
                        ));
                    }
                    let saved_plan = self.reports.join(format!("{label}-platform-plan.json"));
                    fs::write(
                        &saved_plan,
                        serde_json::to_vec_pretty(&addition).map_err(|error| error.to_string())?,
                    )
                    .map_err(|error| error.to_string())?;
                    let saved_plan_path = saved_plan.to_str().ok_or("Plan path requires UTF-8")?;
                    let added = self.cli(
                        &format!("{label}-platform-add"),
                        &app,
                        &["platform", "add", target, "--plan", saved_plan_path],
                        true,
                    )?;
                    let repeated = self.cli(
                        &format!("{label}-platform-repeat"),
                        &app,
                        &["platform", "add", target],
                        true,
                    )?;
                    if added["plan_sha256"].as_str().map(str::len) != Some(64)
                        || added["completed"] != true
                        || repeated["already_enabled"] != true
                        || repeated["configuration_written"] != false
                        || repeated["entry_points_created"] != json!([])
                        || repeated["generation"]["files_written"] != json!([])
                    {
                        return Err(format!("{label} platform addition failed idempotence"));
                    }
                    plan = self.cli(
                        &format!("{label}-added-target-plan"),
                        &app,
                        &["build", target, "--dry-run"],
                        true,
                    )?;
                    enabled = true;
                }
                let mut artifact_receipt = None;
                if enabled {
                    if plan["target"] != target {
                        return Err(format!("{label} planned the wrong build target"));
                    }
                    for file in ["mise.toml", "mise.android.toml"] {
                        if app.join(file).is_file() {
                            self.run(
                                &format!("{label}-trust-{file}"),
                                Command::new("mise")
                                    .current_dir(&app)
                                    .arg("trust")
                                    .arg(app.join(file)),
                                true,
                            )?;
                        }
                    }
                    self.cli(&format!("{label}-build"), &app, &["build", target], true)?;
                    if template == "workbench" && target == "desktop" {
                        self.run(
                            &format!("{label}-application-tests"),
                            Command::new("mise")
                                .current_dir(&app)
                                .args(["exec", "--", "cargo", "test", "-p", "box-app", "--lib"]),
                            true,
                        )?;
                    }
                    let artifact = if target == "android" {
                        let apk =
                            app.join("android-host/app/build/outputs/apk/debug/app-debug.apk");
                        let archive = zip::ZipArchive::new(
                            fs::File::open(&apk).map_err(|error| error.to_string())?,
                        )
                        .map_err(|error| error.to_string())?;
                        for abi in ["arm64-v8a", "x86_64"] {
                            if !archive
                                .file_names()
                                .any(|name| name == format!("lib/{abi}/libgpui_box_android.so"))
                            {
                                return Err(format!("{label} APK is missing {abi}"));
                            }
                        }
                        apk
                    } else {
                        let bytes = self.run(
                            &format!("{label}-cargo-metadata"),
                            Command::new("mise").current_dir(&app).args([
                                "exec",
                                "--",
                                "cargo",
                                "metadata",
                                "--no-deps",
                                "--format-version",
                                "1",
                            ]),
                            true,
                        )?;
                        let metadata: Value =
                            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                        PathBuf::from(
                            metadata["target_directory"]
                                .as_str()
                                .ok_or("Cargo metadata omitted target_directory")?,
                        )
                        .join("debug")
                        .join(format!("box-desktop{}", env::consts::EXE_SUFFIX))
                    };
                    if !artifact.is_file()
                        || fs::metadata(&artifact)
                            .map_err(|error| error.to_string())?
                            .len()
                            == 0
                    {
                        return Err(format!("{label} build produced no artifact"));
                    }
                    let mut file = fs::File::open(&artifact).map_err(|error| error.to_string())?;
                    let mut digest = Sha256::new();
                    let mut buffer = [0; 64 * 1024];
                    loop {
                        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                        if count == 0 {
                            break;
                        }
                        digest.update(&buffer[..count]);
                    }
                    artifact_receipt = Some(
                        json!({"path": artifact, "sha256": format!("{:x}", digest.finalize())}),
                    );
                }
                if fs::read(&source_path).map_err(|error| error.to_string())? != customized
                    || fs::read(&provenance_path).map_err(|error| error.to_string())?
                        != provenance_bytes
                    || (targets.contains("android")
                        && fs::read(&native_path).map_err(|error| error.to_string())? != native)
                {
                    return Err(format!(
                        "{label} changed application-owned content or provenance"
                    ));
                }
                applications.push(json!({"template": provenance, "configuration": configuration, "targets": targets, "platform_added": platform_added, "build_target": target, "build_completed": enabled, "artifact": artifact_receipt, "path": app}));
            }
        }
        if self.cli(
            "sdk-verify-after",
            source,
            &["sdk", "verify", sdk_path],
            true,
        )? != manifest
        {
            return Err("Application builds changed the exported SDK".into());
        }
        Ok(
            json!({"cli": identity, "sdk": manifest, "cache": cached, "registry": registry, "resolution": resolution, "applications": applications}),
        )
    }
}

#[test]
#[ignore = "Real external SDK/template builds require installed native toolchains"]
fn external_templates_build_from_a_verified_committed_sdk() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let target = env::var("GPUI_BOX_QUALIFICATION_TARGET")
        .expect("Set GPUI_BOX_QUALIFICATION_TARGET to desktop or android");
    assert!(matches!(target.as_str(), "desktop" | "android"));
    let reports = env::var_os("GPUI_BOX_QUALIFICATION_REPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| source.join("target/ci/template-qualification"));
    fs::create_dir_all(reports.parent().unwrap()).unwrap();
    fs::create_dir(&reports).expect("Qualification report directory must be fresh");
    let workspace = tempfile::Builder::new()
        .prefix("gpui-box qualification 有 空格 ")
        .tempdir()
        .unwrap()
        .keep();
    let build = workspace.join("build");
    let mut qualification = Qualification {
        reports: reports.clone(),
        build: build.clone(),
        records: Vec::new(),
    };
    let result = qualification.qualify(source, &workspace, &target);
    let report = json!({
        "schema_version": 1,
        "host": format!("{}-{}", env::consts::ARCH, env::consts::OS),
        "target": target,
        "workspace": workspace,
        "build_directory": build,
        "success": result.is_ok(),
        "qualification": result.as_ref().ok(),
        "error": result.as_ref().err(),
        "commands": qualification.records,
        "runtime_executed": false,
    });
    fs::write(
        reports.join("receipt.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("Qualification report {}", reports.display());
    result.unwrap();
}
