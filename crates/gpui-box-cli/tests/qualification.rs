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
        for template in ["starter", "adaptive", "component"] {
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
                let enabled = targets.split(',').any(|configured| configured == target);
                let plan = self.cli(
                    &format!("{label}-plan"),
                    &app,
                    &["build", target, "--dry-run"],
                    enabled,
                )?;
                if !enabled && plan["code"] != "BUILD_PLAN_FAILED" {
                    return Err(format!("{label} accepted a disabled build target"));
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
                applications.push(json!({"template": provenance, "targets": targets, "build_target": target, "build_completed": enabled, "artifact": artifact_receipt, "path": app}));
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
        Ok(json!({"cli": identity, "sdk": manifest, "applications": applications}))
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
