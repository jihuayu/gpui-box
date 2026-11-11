use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn distribution() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
fn cli(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("start CLI")
}
fn create(root: &Path) -> PathBuf {
    let app = root.join("Application with spaces");
    let output = cli(
        root,
        &[
            "new",
            app.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    app
}
#[test]
fn generation_preserves_user_sources_and_rejects_managed_conflicts() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let source = app.join("app/src/lib.rs");
    fs::write(&source, "// custom application\n").unwrap();
    fs::write(
        app.join("android-custom.gradle.kts"),
        "// native customizations\n",
    )
    .unwrap();
    let output = cli(&app, &["generate"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        "// custom application\n"
    );
    assert_eq!(
        fs::read_to_string(app.join("android-custom.gradle.kts")).unwrap(),
        "// native customizations\n"
    );
    let manifest = app.join("Cargo.toml");
    fs::write(&manifest, "// conflict\n").unwrap();
    let output = cli(&app, &["generate"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("was edited"));
    assert_eq!(fs::read_to_string(manifest).unwrap(), "// conflict\n");
}
#[test]
fn configuration_rejects_unknown_keys_and_unimplemented_channels() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let file = app.join("gpui-box.json");
    let original = fs::read(&file).unwrap();
    let mut config: serde_json::Value = serde_json::from_slice(&original).unwrap();
    config["typo"] = true.into();
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = cli(&app, &["generate"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown field"));
    config.as_object_mut().unwrap().remove("typo");
    config["update_channel"] = "direct-apk".into();
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    let output = cli(&app, &["generate"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("M4/M5"));
}
#[test]
fn new_never_overwrites_an_existing_destination() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let output = cli(
        temporary.path(),
        &[
            "new",
            app.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert!(app.join("app/src/lib.rs").is_file());
}

#[test]
fn release_requires_explicit_signing_before_running_build_tools() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&app)
        .args(["package", "android", "--release"])
        .env_remove("GPUI_BOX_ANDROID_KEYSTORE")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("GPUI_BOX_ANDROID_KEYSTORE"));
    assert!(!app.join("dist").exists());
    assert!(!app.join("android-host/app/build").exists());
    assert!(
        app.join("android-host/gradle/wrapper/gradle-wrapper.jar")
            .is_file()
    );
    let host = fs::read_to_string(app.join("android-host/app/build.gradle.kts")).unwrap();
    assert!(host.contains("buildRustRelease"));
    assert!(host.contains("rustJniLibs/$profile"));
    assert!(host.contains("GPUI_BOX_ANDROID_STORE_PASSWORD"));
}

#[cfg(windows)]
#[test]
fn portable_assets_cannot_overwrite_bundle_metadata_or_recurse_into_output() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let path = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for asset in [
        "dist",
        "target",
        "licenses/custom",
        "bundle.json",
        "BOX-DESKTOP.EXE",
        "SHA256SUMS",
    ] {
        config["assets"] = serde_json::json!([asset]);
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = cli(&app, &["package", "desktop", "--portable"]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("overlaps"));
        assert!(!app.join("dist").exists());
    }
}
#[test]
fn native_declarations_follow_the_validated_permissions() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let manifest = app.join("android-host/app/src/main/AndroidManifest.xml");
    assert!(
        !fs::read_to_string(&manifest)
            .unwrap()
            .contains("android.permission.INTERNET")
    );
    let file = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["permissions"] = serde_json::json!(["internet"]);
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    assert!(
        fs::read_to_string(manifest)
            .unwrap()
            .contains("android.permission.INTERNET")
    );
}

#[test]
fn generation_recovers_an_interrupted_write_without_accepting_user_changes() {
    use sha2::{Digest, Sha256};
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let state_path = app.join(".gpui-box/generated.json");
    let mut state: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let manifest = app.join("Cargo.toml");
    let updated = format!(
        "{}\n# interrupted generation\n",
        fs::read_to_string(&manifest).unwrap()
    );
    state["Cargo.toml"] = format!("{:x}", Sha256::digest(updated.as_bytes())).into();
    let journal: std::collections::BTreeMap<_, _> = state
        .as_object()
        .unwrap()
        .iter()
        .map(|(path, hash)| (path, vec![hash]))
        .collect();
    fs::write(
        app.join(".gpui-box/pending.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    fs::write(&manifest, updated).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    assert!(!app.join(".gpui-box/pending.json").exists());
    fs::write(&manifest, "# user edit after interruption").unwrap();
    assert!(!cli(&app, &["generate"]).status.success());
}

#[test]
fn version_and_assets_are_validated_and_propagated() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let file = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["version"] = "1.2.3".into();
    config["version_code"] = 42.into();
    config["assets"] = serde_json::json!(["assets", "custom$assets"]);
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    assert!(
        fs::read_to_string(app.join("app/Cargo.toml"))
            .unwrap()
            .contains("version = \"1.2.3\"")
    );
    let host = fs::read_to_string(app.join("android-host/app/build.gradle.kts")).unwrap();
    assert!(host.contains("versionCode = 42"));
    assert!(host.contains("custom\\$assets"));
    // Windows canonical paths use a verbatim prefix that Java File cannot open.
    assert!(!host.contains("\\\\\\\\?\\\\"));
    config["assets"] = serde_json::json!(["../outside"]);
    fs::write(file, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(!cli(&app, &["generate"]).status.success());
}

#[test]
fn typed_services_generate_requested_declarations_and_remove_obsolete_entries() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let file = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["services"] = serde_json::json!({
        "schema_version": 1,
        "permissions": [{ "kind": "camera", "purpose": "Scan a document after selecting Scan" }],
        "url_schemes": ["gpui-box-test"]
    });
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::write(
        app.join("android-custom.gradle.kts"),
        "// app customization\n",
    )
    .unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    let manifest =
        fs::read_to_string(app.join("android-host/app/src/main/AndroidManifest.xml")).unwrap();
    assert!(manifest.contains("android.permission.CAMERA"));
    assert!(
        manifest.contains("android:name=\"android.hardware.camera\" android:required=\"false\"")
    );
    assert!(
        manifest.contains(
            "android:name=\"android.hardware.camera.autofocus\" android:required=\"false\""
        )
    );
    assert!(!manifest.contains("android.permission.RECORD_AUDIO"));
    assert!(manifest.contains("android:scheme=\"gpui-box-test\""));
    let declarations =
        fs::read_to_string(app.join("android-host/app/src/main/assets/gpui-box-services.json"))
            .unwrap();
    assert!(declarations.contains("Scan a document"));
    assert_eq!(
        fs::read_to_string(app.join("android-custom.gradle.kts")).unwrap(),
        "// app customization\n"
    );
    config["services"]["permissions"] = serde_json::json!([]);
    config["services"]["url_schemes"] = serde_json::json!([]);
    fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    let manifest =
        fs::read_to_string(app.join("android-host/app/src/main/AndroidManifest.xml")).unwrap();
    assert!(!manifest.contains("android.permission.CAMERA"));
    assert!(!manifest.contains("android.hardware.camera"));
    assert!(!manifest.contains("gpui-box-test"));
}

#[test]
fn typed_services_reject_unknown_ambiguous_or_incomplete_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let file = app.join("gpui-box.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    for services in [
        serde_json::json!({"schema_version": 0}),
        serde_json::json!({"schema_version": 2}),
        serde_json::json!({"notifications": true}),
        serde_json::json!({"permissions": [{"kind": "camera", "purpose": ""}]}),
        serde_json::json!({"permissions": [{"kind": "photos", "purpose": "Pick photos"}]}),
        serde_json::json!({"permissions": [{"kind": "camera", "purpose": "Scan"}, {"kind": "camera", "purpose": "Again"}]}),
        serde_json::json!({"url_schemes": ["https"]}),
        serde_json::json!({"url_schemes": ["content"]}),
        serde_json::json!({"url_schemes": ["Bad\"Scheme"]}),
        serde_json::json!({"url_schemes": ["test", "test"]}),
    ] {
        let mut config = original.clone();
        config["services"] = services;
        fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(
            !cli(&app, &["generate"]).status.success(),
            "should reject {config}"
        );
    }
}

type TreeSnapshot = std::collections::BTreeMap<PathBuf, (bool, String, std::time::SystemTime)>;

fn snapshot(root: &Path) -> TreeSnapshot {
    fn visit(root: &Path, path: &Path, files: &mut TreeSnapshot) {
        use sha2::{Digest, Sha256};
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            let directory = metadata.is_dir();
            files.insert(
                entry.path().strip_prefix(root).unwrap().to_path_buf(),
                (
                    directory,
                    if directory {
                        String::new()
                    } else {
                        format!("{:x}", Sha256::digest(fs::read(entry.path()).unwrap()))
                    },
                    // Windows can publish delayed directory timestamps from
                    // initial creation. File modification times remain exact.
                    if directory {
                        std::time::UNIX_EPOCH
                    } else {
                        metadata.modified().unwrap()
                    },
                ),
            );
            if directory {
                visit(root, &entry.path(), files);
            }
        }
    }
    let mut files = TreeSnapshot::new();
    visit(root, root, &mut files);
    files
}

fn dry_run(app: &Path) -> serde_json::Value {
    let output = cli(app, &["generate", "--dry-run", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["success"], true);
    assert_eq!(response["result"]["schema_version"], 1);
    response["result"].clone()
}

#[test]
fn dry_run_is_deterministic_and_preserves_file_bytes_names_and_modification_times() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    fs::write(app.join("app/src/lib.rs"), "// customized application\n").unwrap();
    let before = snapshot(&app);
    let plan = dry_run(&app);
    assert_eq!(plan, dry_run(&app));
    assert_eq!(before, snapshot(&app));
    assert!(
        plan["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["action"] == "unchanged")
    );
    assert_eq!(plan["metadata"]["action"], "unchanged");
    assert_eq!(plan["recovering_pending_journal"], false);
    assert!(
        !plan["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "app/src/lib.rs")
    );
    let human = cli(&app, &["generate", "--dry-run"]);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("Unchanged Cargo.toml"));
    assert_eq!(before, snapshot(&app));
}

#[test]
fn dry_run_reports_exact_hashes_and_creation_then_apply_matches_plan() {
    use sha2::{Digest, Sha256};
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let file = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    config["version"] = "1.2.3".into();
    config["version_code"] = 42.into();
    fs::write(file, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::remove_file(app.join("android-host/gradle.properties")).unwrap();
    let before = snapshot(&app);
    let plan = dry_run(&app);
    assert_eq!(before, snapshot(&app));
    let files = plan["files"].as_array().unwrap();
    assert!(
        files
            .windows(2)
            .all(|pair| pair[0]["path"].as_str() < pair[1]["path"].as_str())
    );
    let created = files
        .iter()
        .find(|file| file["path"] == "android-host/gradle.properties")
        .unwrap();
    assert_eq!(created["action"], "create");
    assert!(created["before_sha256"].is_null());
    assert!(files.iter().any(|file| file["action"] == "update"));
    for file in files {
        let path = app.join(file["path"].as_str().unwrap());
        if let Some(before) = file["before_sha256"].as_str() {
            assert_eq!(
                format!("{:x}", Sha256::digest(fs::read(path).unwrap())),
                before
            );
        }
    }
    assert!(cli(&app, &["generate"]).status.success());
    for file in files.iter().chain(std::iter::once(&plan["metadata"])) {
        let bytes = fs::read(app.join(file["path"].as_str().unwrap())).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            file["after_sha256"].as_str().unwrap()
        );
    }
}

#[test]
fn dry_run_without_metadata_creates_no_directories_or_locks() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("Uninitialized App");
    fs::create_dir(&app).unwrap();
    let seed = create(temporary.path());
    fs::copy(seed.join("gpui-box.json"), app.join("gpui-box.json")).unwrap();
    let before = snapshot(&app);
    let plan = dry_run(&app);
    assert!(
        plan["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["action"] == "create")
    );
    assert_eq!(plan["metadata"]["action"], "create");
    assert_eq!(before, snapshot(&app));
    assert!(!app.join(".gpui-box").exists());
}

#[test]
fn dry_run_conflicts_corruption_and_active_lock_preserve_the_project() {
    for failure in ["edited", "state", "journal", "lock", "config"] {
        let temporary = tempfile::tempdir().unwrap();
        let app = create(temporary.path());
        let path = match failure {
            "edited" => "Cargo.toml",
            "state" => ".gpui-box/generated.json",
            "journal" => ".gpui-box/pending.json",
            "lock" => ".gpui-box/generate.lock",
            _ => "gpui-box.json",
        };
        fs::write(app.join(path), b"preserve original failure").unwrap();
        let before = snapshot(&app);
        let output = cli(&app, &["generate", "--dry-run", "--json"]);
        assert!(!output.status.success(), "must fail for {failure}");
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "GENERATION_PLAN_FAILED");
        assert_eq!(before, snapshot(&app));
    }
}

#[test]
fn dry_run_reuses_pending_journal_preflight_without_finishing_recovery() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let state: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(app.join(".gpui-box/generated.json")).unwrap()).unwrap();
    let journal: std::collections::BTreeMap<_, _> = state
        .iter()
        .map(|(path, digest)| (path, vec![digest]))
        .collect();
    fs::write(
        app.join(".gpui-box/pending.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    fs::write(app.join(".gpui-box/generated.json"), b"{}").unwrap();
    let before = snapshot(&app);
    let plan = dry_run(&app);
    assert_eq!(plan["recovering_pending_journal"], true);
    assert_eq!(plan["metadata"]["action"], "update");
    assert_eq!(before, snapshot(&app));
    assert!(cli(&app, &["generate"]).status.success());
    assert!(!app.join(".gpui-box/pending.json").exists());
}

#[cfg(unix)]
#[test]
fn dry_run_and_apply_reject_managed_output_and_metadata_symlinks() {
    for relative in ["Cargo.toml", ".gpui-box/generated.json", "android-host/app"] {
        let temporary = tempfile::tempdir().unwrap();
        let app = create(temporary.path());
        let original = app.join(relative);
        let outside = temporary.path().join("outside");
        fs::rename(&original, &outside).unwrap();
        std::os::unix::fs::symlink(&outside, &original).unwrap();
        for arguments in [
            vec!["generate", "--dry-run", "--json"],
            vec!["generate", "--json"],
        ] {
            let output = cli(&app, &arguments);
            assert!(!output.status.success());
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("links")
            );
        }
    }
}
