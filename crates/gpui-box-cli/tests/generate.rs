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

fn create_single_target(root: &Path, target: &str) -> PathBuf {
    let app = root.join("Platform App 有 空格");
    let output = cli(
        root,
        &[
            "new",
            app.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
            "--targets",
            target,
            "--template",
            "workbench",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    app
}

fn add_platform(app: &Path, target: &str, preview: bool, success: bool) -> serde_json::Value {
    let mut args = vec!["platform", "add", target, "--json"];
    if preview {
        args.push("--dry-run");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(app)
        .args(args)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["success"], success);
    if !success {
        assert_eq!(response["error"]["code"], "PLATFORM_ADDITION_FAILED");
    }
    response
}

#[test]
fn platform_add_previews_exact_outputs_and_repeats_without_rewriting_application_sources() {
    for (existing, added, entry_points) in [
        (
            "desktop",
            "android",
            vec![
                "android/src/lib.rs",
                "android-custom.gradle.kts",
                "mise.android.toml",
            ],
        ),
        ("android", "desktop", vec!["desktop/src/main.rs"]),
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let app = create_single_target(temporary.path(), existing);
        fs::write(
            app.join("app/src/providers.rs"),
            "// application service customization\n",
        )
        .unwrap();
        let before = snapshot(&app);
        let preview = add_platform(&app, added, true, true)["result"].clone();
        assert_eq!(preview, add_platform(&app, added, true, true)["result"]);
        assert_eq!(before, snapshot(&app));
        assert_eq!(preview["already_enabled"], false);
        assert_eq!(preview["configuration"]["action"], "update");
        let receipt = add_platform(&app, added, false, true)["result"].clone();
        assert_eq!(receipt["configuration_written"], true);
        let created = receipt["entry_points_created"].as_array().unwrap();
        assert_eq!(created.len(), entry_points.len());
        for path in entry_points {
            assert!(created.contains(&serde_json::json!(path)));
            assert!(app.join(path).is_file());
        }
        let after = snapshot(&app);
        for (path, state) in &before {
            if path.starts_with("app/src")
                || path.starts_with("assets")
                || path == Path::new("mise.toml")
                || path == Path::new(".gpui-box/template.json")
            {
                assert_eq!(after.get(path), Some(state), "{}", path.display());
            }
        }
        use sha2::{Digest, Sha256};
        for change in preview["generation"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .chain(std::iter::once(&preview["generation"]["metadata"]))
            .chain(preview["entry_points"].as_array().unwrap())
            .chain(std::iter::once(&preview["configuration"]))
        {
            let path = change["path"].as_str().unwrap();
            let digest = format!("{:x}", Sha256::digest(fs::read(app.join(path)).unwrap()));
            assert_eq!(change["after_sha256"], digest, "{path}");
        }
        let repeated = add_platform(&app, added, false, true);
        assert_eq!(repeated["result"]["already_enabled"], true);
        assert_eq!(repeated["result"]["configuration_written"], false);
        assert_eq!(
            repeated["result"]["entry_points_created"],
            serde_json::json!([])
        );
        assert_eq!(
            repeated["result"]["generation"]["files_written"],
            serde_json::json!([])
        );
        assert_eq!(after, snapshot(&app));
    }
}

#[test]
fn platform_add_rejects_unowned_entry_points_and_edited_managed_outputs_before_config_changes() {
    for conflict in [
        "android/src/lib.rs",
        "android-custom.gradle.kts",
        "mise.android.toml",
        "Cargo.toml",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let app = create_single_target(temporary.path(), "desktop");
        fs::create_dir_all(app.join(conflict).parent().unwrap()).unwrap();
        fs::write(app.join(conflict), "// existing unrelated content\n").unwrap();
        let before = snapshot(&app);
        for preview in [true, false] {
            add_platform(&app, "android", preview, false);
        }
        assert_eq!(before, snapshot(&app), "{conflict}");
    }
}

#[test]
fn platform_add_reenables_prior_target_and_preserves_custom_entry_points() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let custom = b"// customized Android entry point\n";
    fs::write(app.join("android/src/lib.rs"), custom).unwrap();
    let path = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["targets"] = serde_json::json!(["desktop"]);
    fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    let previous = fs::metadata(app.join("android/src/lib.rs"))
        .unwrap()
        .modified()
        .unwrap();
    add_platform(&app, "android", false, true);
    assert_eq!(fs::read(app.join("android/src/lib.rs")).unwrap(), custom);
    assert_eq!(
        fs::metadata(app.join("android/src/lib.rs"))
            .unwrap()
            .modified()
            .unwrap(),
        previous
    );
}

#[test]
fn platform_add_recovers_after_config_publication_and_partial_source_creation() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create_single_target(temporary.path(), "desktop");
    let path = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["targets"] = serde_json::json!(["desktop", "android"]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::write(
        app.join("android-custom.gradle.kts"),
        "// retained customization\n",
    )
    .unwrap();
    let response = add_platform(&app, "android", false, true);
    assert_eq!(response["result"]["already_enabled"], true);
    assert_eq!(
        response["result"]["entry_points_created"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fs::read_to_string(app.join("android-custom.gradle.kts")).unwrap(),
        "// retained customization\n"
    );
    assert!(app.join("android-host/app/build.gradle.kts").is_file());
    assert!(!app.join(".gpui-box/pending.json").exists());
}

#[test]
fn platform_add_failures_validate_targets_config_and_shared_generation_lock() {
    let temporary = tempfile::tempdir().unwrap();
    let before = snapshot(temporary.path());
    for preview in [true, false] {
        add_platform(temporary.path(), "ios", preview, false);
    }
    assert_eq!(before, snapshot(temporary.path()));
    let app = create_single_target(temporary.path(), "desktop");
    let lock = app.join(".gpui-box/generate.lock");
    fs::write(&lock, "another generator").unwrap();
    let before = snapshot(&app);
    for preview in [true, false] {
        add_platform(&app, "android", preview, false);
    }
    assert_eq!(before, snapshot(&app));
    fs::remove_file(lock).unwrap();
    fs::write(app.join("gpui-box.json"), b"invalid").unwrap();
    let before = snapshot(&app);
    for preview in [true, false] {
        add_platform(&app, "android", preview, false);
    }
    assert_eq!(before, snapshot(&app));
}

#[test]
fn platform_add_rejects_linked_entry_point_parents_and_preserves_external_content() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create_single_target(temporary.path(), "desktop");
    let outside = temporary.path().join("Outside source");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.txt"), "External content").unwrap();
    let before = snapshot(&app);
    let external_before = snapshot(&outside);
    let linked = app.join("android");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &linked).unwrap();
    #[cfg(windows)]
    {
        let script = "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:GPUI_BOX_TEST_LINK -Target $env:GPUI_BOX_TEST_TARGET | Out-Null";
        assert!(
            Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", script])
                .env("GPUI_BOX_TEST_LINK", &linked)
                .env("GPUI_BOX_TEST_TARGET", &outside)
                .status()
                .unwrap()
                .success()
        );
    }
    for preview in [true, false] {
        let response = add_platform(&app, "android", preview, false);
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("links")
        );
    }
    assert_eq!(external_before, snapshot(&outside));
    assert_eq!(
        fs::read_to_string(outside.join("sentinel.txt")).unwrap(),
        "External content"
    );
    #[cfg(windows)]
    fs::remove_dir(linked).unwrap();
    #[cfg(unix)]
    fs::remove_file(linked).unwrap();
    assert_eq!(before, snapshot(&app));
}

#[cfg(windows)]
#[test]
fn platform_add_recovers_journal_after_locked_managed_output_failure() {
    use std::os::windows::fs::OpenOptionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let app = create_single_target(temporary.path(), "desktop");
    let state = fs::read(app.join(".gpui-box/generated.json")).unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(app.join("Cargo.toml"))
        .unwrap();
    add_platform(&app, "android", false, false);
    assert!(app.join("android/src/lib.rs").is_file());
    assert!(app.join(".gpui-box/pending.json").is_file());
    assert_eq!(
        fs::read(app.join(".gpui-box/generated.json")).unwrap(),
        state
    );
    drop(held);
    let response = add_platform(&app, "android", false, true);
    assert_eq!(
        response["result"]["generation"]["recovered_pending_journal"],
        true
    );
    assert!(!app.join(".gpui-box/pending.json").exists());
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
fn selected_targets_generate_only_the_requested_starter_platform() {
    let temporary = tempfile::tempdir().unwrap();
    for target in ["desktop", "android"] {
        let app = temporary.path().join(format!("{target} 有 空格"));
        let output = cli(
            temporary.path(),
            &[
                "new",
                app.to_str().unwrap(),
                "--targets",
                target,
                "--distribution",
                distribution().to_str().unwrap(),
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let manifest = fs::read_to_string(app.join("Cargo.toml")).unwrap();
        assert!(manifest.contains(&format!("members = [\"app\",\"{target}\"]")));
        assert_eq!(app.join("desktop").exists(), target == "desktop");
        assert_eq!(app.join("android").exists(), target == "android");
        assert_eq!(app.join("android-host").exists(), target == "android");
        assert_eq!(app.join("mise.android.toml").exists(), target == "android");
        if target == "android" {
            assert!(
                fs::read_to_string(app.join("android-host/gradle.properties"))
                    .unwrap()
                    .contains("android.overridePathCheck=true")
            );
        }
        let provenance: serde_json::Value =
            serde_json::from_slice(&fs::read(app.join(".gpui-box/template.json")).unwrap())
                .unwrap();
        assert_eq!(provenance["template"], "starter");
        assert_eq!(provenance["template_version"], 1);
        assert_eq!(provenance["targets"], serde_json::json!([target]));
        let output = cli(&app, &["generate", "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["result"]["files_written"], serde_json::json!([]));
    }
}

#[test]
fn adaptive_template_records_provenance_and_preserves_application_sources() {
    let temporary = tempfile::tempdir().unwrap();
    for targets in ["desktop", "android", "desktop,android"] {
        let app = temporary.path().join(format!("Adaptive {targets}"));
        let output = cli(
            temporary.path(),
            &[
                "new",
                app.to_str().unwrap(),
                "--distribution",
                distribution().to_str().unwrap(),
                "--template",
                "adaptive",
                "--targets",
                targets,
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["result"]["template"]["id"], "adaptive");
        let provenance: serde_json::Value =
            serde_json::from_slice(&fs::read(app.join(".gpui-box/template.json")).unwrap())
                .unwrap();
        assert_eq!(provenance["template"], "adaptive");
        assert_eq!(provenance["template_version"], 1);
        assert_eq!(
            app.join("desktop/src/main.rs").exists(),
            targets.contains("desktop")
        );
        assert_eq!(
            app.join("android/src/lib.rs").exists(),
            targets.contains("android")
        );
        let source = app.join("app/src/lib.rs");
        let original = fs::read_to_string(&source).unwrap();
        assert!(original.contains("master_detail("));
        assert!(original.contains("Shared workspace draft"));
        assert!(original.contains("BackGesturePhase::Committed"));
        let customized = format!("{original}\n// Application-owned adaptive customization\n");
        fs::write(&source, customized.as_bytes()).unwrap();
        let metadata = fs::read(app.join(".gpui-box/template.json")).unwrap();
        let output = cli(&app, &["generate"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read_to_string(source).unwrap(), customized);
        assert_eq!(
            fs::read(app.join(".gpui-box/template.json")).unwrap(),
            metadata
        );
    }
}

#[test]
fn component_template_supports_all_targets_and_preserves_preview_customization() {
    let temporary = tempfile::tempdir().unwrap();
    for targets in ["desktop", "android", "desktop,android"] {
        let app = temporary
            .path()
            .join(format!("Component {targets} 有 空格"));
        let output = cli(
            temporary.path(),
            &[
                "new",
                app.to_str().unwrap(),
                "--distribution",
                distribution().to_str().unwrap(),
                "--template",
                "component",
                "--targets",
                targets,
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["result"]["template"]["id"], "component");
        let provenance_path = app.join(".gpui-box/template.json");
        let provenance_bytes = fs::read(&provenance_path).unwrap();
        let provenance: serde_json::Value = serde_json::from_slice(&provenance_bytes).unwrap();
        assert_eq!(provenance["template"], "component");
        assert_eq!(provenance["template_version"], 1);
        assert_eq!(
            provenance["targets"],
            serde_json::json!(targets.split(',').collect::<Vec<_>>())
        );
        let source_path = app.join("app/src/lib.rs");
        let original = fs::read_to_string(&source_path).unwrap();
        for label in [
            "Component author preview",
            "Component preview input",
            "High contrast",
            "Reduced motion",
            "Large text",
            "Example checked state",
        ] {
            assert!(original.contains(label));
        }
        assert_eq!(
            app.join("desktop/src/main.rs").is_file(),
            targets.contains("desktop")
        );
        assert_eq!(
            app.join("android/src/lib.rs").is_file(),
            targets.contains("android")
        );
        let customized = format!("{original}\n// My component specimen\n");
        fs::write(&source_path, &customized).unwrap();
        assert!(cli(&app, &["generate"]).status.success());
        assert_eq!(fs::read_to_string(&source_path).unwrap(), customized);
        assert_eq!(fs::read(provenance_path).unwrap(), provenance_bytes);
    }
    let app = temporary.path().join("Existing component preview");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("README.md"), "Existing author guide").unwrap();
    assert!(
        cli(
            temporary.path(),
            &[
                "init",
                app.to_str().unwrap(),
                "--distribution",
                distribution().to_str().unwrap(),
                "--template",
                "component"
            ]
        )
        .status
        .success()
    );
    assert_eq!(
        fs::read_to_string(app.join("README.md")).unwrap(),
        "Existing author guide"
    );
    assert!(
        fs::read_to_string(app.join("app/src/lib.rs"))
            .unwrap()
            .contains("ComponentPreview")
    );
}

#[test]
fn adaptive_init_preserves_existing_content_and_rejects_repeated_template_options() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("Existing adaptive 有 空格");
    fs::create_dir(&app).unwrap();
    fs::write(app.join("README.md"), b"Existing documentation").unwrap();
    fs::write(app.join("business.txt"), b"Existing business content").unwrap();
    let output = cli(
        temporary.path(),
        &[
            "init",
            app.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
            "--template",
            "adaptive",
            "--targets",
            "android",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(app.join("README.md")).unwrap(),
        b"Existing documentation"
    );
    assert_eq!(
        fs::read(app.join("business.txt")).unwrap(),
        b"Existing business content"
    );
    assert!(
        fs::read_to_string(app.join("app/src/lib.rs"))
            .unwrap()
            .contains("Adaptive workspace")
    );
    let before = snapshot(&app);
    let output = cli(
        temporary.path(),
        &[
            "init",
            app.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
            "--template",
            "starter",
            "--template",
            "adaptive",
            "--json",
        ],
    );
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENTS");
    assert_eq!(snapshot(&app), before);
}

#[test]
fn target_reenable_retains_managed_ownership_and_detects_disabled_target_edits() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let path = app.join("gpui-box.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["targets"] = serde_json::json!(["desktop"]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    config["targets"] = serde_json::json!(["desktop", "android"]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    config["targets"] = serde_json::json!(["desktop"]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    fs::write(app.join("android/Cargo.toml"), "caller-owned edit").unwrap();
    config["targets"] = serde_json::json!(["desktop", "android"]);
    fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    let before = snapshot(&app);
    let output = cli(&app, &["generate"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("was edited"));
    assert_eq!(snapshot(&app), before);
}

#[test]
fn init_preserves_existing_documents_and_rejects_repeated_initialization() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("Existing app 有 空格");
    fs::create_dir(&app).unwrap();
    for path in ["README.md", "LICENSE", ".gitignore", "business.txt"] {
        fs::write(app.join(path), format!("original {path}\n")).unwrap();
    }
    let distribution = distribution();
    let args = [
        "init",
        ".",
        "--distribution",
        distribution.to_str().unwrap(),
        "--targets",
        "desktop",
        "--json",
    ];
    let output = cli(&app, &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for path in ["README.md", "LICENSE", ".gitignore", "business.txt"] {
        assert_eq!(
            fs::read_to_string(app.join(path)).unwrap(),
            format!("original {path}\n")
        );
    }
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(app.join("gpui-box.json")).unwrap()).unwrap();
    assert_eq!(config["name"], "Existing app 有 空格");
    assert!(!app.join("android-host").exists());
    let before = snapshot(&app);
    let output = cli(&app, &args);
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already initialized")
    );
    assert_eq!(snapshot(&app), before);
    assert!(cli(&app, &["generate"]).status.success());
}

#[test]
fn init_conflicts_and_invalid_target_options_preserve_existing_tree() {
    let temporary = tempfile::tempdir().unwrap();
    for conflict in [
        "Cargo.toml",
        "app/src/lib.rs",
        "mise.toml",
        ".gpui-box/template.json",
    ] {
        let app = temporary.path().join(conflict.replace('/', "-"));
        fs::create_dir_all(app.join(conflict).parent().unwrap()).unwrap();
        fs::write(app.join(conflict), "caller-owned").unwrap();
        let before = snapshot(&app);
        let output = cli(
            &app,
            &[
                "init",
                ".",
                "--distribution",
                distribution().to_str().unwrap(),
                "--json",
            ],
        );
        assert!(!output.status.success());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Init conflicts"),
            "{response}"
        );
        assert_eq!(snapshot(&app), before);
    }
    for targets in ["", "android,android", "ios", "desktop,web"] {
        let app = temporary.path().join("Invalid target");
        let output = cli(
            temporary.path(),
            &[
                "new",
                app.to_str().unwrap(),
                "--distribution",
                distribution().to_str().unwrap(),
                "--targets",
                targets,
            ],
        );
        assert!(!output.status.success());
        assert!(!app.exists());
    }
}

#[test]
fn init_rejects_active_lock_and_project_options_fail_before_writes() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("Locked existing app");
    fs::create_dir(&app).unwrap();
    fs::write(app.join(".gpui-box-init.lock"), "another initializer").unwrap();
    let before = snapshot(&app);
    let output = cli(
        &app,
        &[
            "init",
            ".",
            "--distribution",
            distribution().to_str().unwrap(),
            "--json",
        ],
    );
    assert!(!output.status.success());
    assert_eq!(snapshot(&app), before);
    let destination = temporary.path().join("Must remain absent");
    for options in [
        vec!["--targets"],
        vec!["--targets", "desktop", "--targets", "android"],
        vec!["--template", "unknown"],
    ] {
        let distribution = distribution();
        let mut args = vec![
            "new",
            destination.to_str().unwrap(),
            "--distribution",
            distribution.to_str().unwrap(),
        ];
        args.extend(options);
        args.push("--json");
        let output = cli(temporary.path(), &args);
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "INVALID_ARGUMENTS");
        assert!(!destination.exists());
    }
}

#[cfg(unix)]
#[test]
fn init_rejects_linked_output_directories_without_writing_through_them() {
    let temporary = tempfile::tempdir().unwrap();
    let app = temporary.path().join("Linked existing app");
    let outside = temporary.path().join("Outside app");
    fs::create_dir(&app).unwrap();
    fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, app.join("app")).unwrap();
    let output = cli(
        &app,
        &[
            "init",
            ".",
            "--distribution",
            distribution().to_str().unwrap(),
            "--targets",
            "desktop",
        ],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("links or reparse points"));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&app).unwrap().count(), 1);
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

fn save_plan(root: &Path, plan: &serde_json::Value) -> PathBuf {
    let path = root.join("Saved generation plan.json");
    fs::write(&path, serde_json::to_vec_pretty(plan).unwrap()).unwrap();
    path
}

fn apply_plan(app: &Path, plan: &Path, success: bool) -> serde_json::Value {
    let output = cli(
        app,
        &["generate", "--plan", plan.to_str().unwrap(), "--json"],
    );
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["success"], success);
    if !success {
        assert_eq!(response["error"]["code"], "GENERATION_FAILED");
    }
    response
}

#[test]
fn saved_plan_apply_reports_exact_writes_and_preserves_unchanged_files() {
    use sha2::{Digest, Sha256};
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let config_file = app.join("gpui-box.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_file).unwrap()).unwrap();
    config["version"] = "2.0.0".into();
    fs::write(config_file, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::remove_file(app.join("android-host/gradle.properties")).unwrap();
    let plan = dry_run(&app);
    let saved = save_plan(temporary.path(), &plan);
    // Application-owned edits do not invalidate managed-output preconditions.
    fs::write(app.join("app/src/lib.rs"), "// updated application\n").unwrap();
    let before = snapshot(&app);
    let receipt = apply_plan(&app, &saved, true)["result"].clone();
    assert_eq!(receipt["schema_version"], 1);
    assert_eq!(receipt["completed"], true);
    assert_eq!(receipt["recovered_pending_journal"], false);
    assert_eq!(receipt["plan_sha256"].as_str().unwrap().len(), 64);
    let after = snapshot(&app);
    let mut written = Vec::new();
    let mut unchanged = Vec::new();
    for change in plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .chain(std::iter::once(&plan["metadata"]))
    {
        let path = change["path"].as_str().unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(fs::read(app.join(path)).unwrap())),
            change["after_sha256"].as_str().unwrap()
        );
        if change["action"] == "unchanged" {
            unchanged.push(path);
            assert_eq!(before[Path::new(path)], after[Path::new(path)]);
        } else {
            written.push(path);
        }
    }
    assert_eq!(receipt["files_written"], serde_json::json!(written));
    assert_eq!(receipt["files_unchanged"], serde_json::json!(unchanged));
    assert_eq!(
        fs::read_to_string(app.join("app/src/lib.rs")).unwrap(),
        "// updated application\n"
    );
    let before = snapshot(&app);
    apply_plan(&app, &saved, false);
    assert_eq!(before, snapshot(&app));
}

#[test]
fn no_op_receipts_are_deterministic_and_do_not_rewrite_project_files() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let saved = save_plan(temporary.path(), &dry_run(&app));
    let before = snapshot(&app);
    let receipt = apply_plan(&app, &saved, true)["result"].clone();
    assert_eq!(receipt["files_written"], serde_json::json!([]));
    assert_eq!(receipt["files_unchanged"].as_array().unwrap().len(), 16);
    assert_eq!(receipt["recovered_pending_journal"], false);
    assert_eq!(before, snapshot(&app));
    assert_eq!(receipt, apply_plan(&app, &saved, true)["result"]);
    let output = cli(&app, &["generate", "--json"]);
    assert!(output.status.success());
    let ordinary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt, ordinary["result"]);
    assert_eq!(before, snapshot(&app));
}

#[test]
fn saved_plan_rejects_stale_inputs_preserving_current_project() {
    for changed in [
        "config", "managed", "missing", "metadata", "journal", "lock",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let app = create(temporary.path());
        let saved = save_plan(temporary.path(), &dry_run(&app));
        match changed {
            "config" => {
                let path = app.join("gpui-box.json");
                let mut bytes = fs::read(&path).unwrap();
                bytes.push(b'\n');
                fs::write(path, bytes).unwrap();
            }
            "managed" => fs::write(app.join("Cargo.toml"), "// concurrent edit\n").unwrap(),
            "missing" => fs::remove_file(app.join("Cargo.toml")).unwrap(),
            "metadata" => {
                let path = app.join(".gpui-box/generated.json");
                let mut bytes = fs::read(&path).unwrap();
                bytes.push(b'\n');
                fs::write(path, bytes).unwrap();
            }
            "journal" => fs::write(app.join(".gpui-box/pending.json"), b"{}").unwrap(),
            "lock" => fs::write(app.join(".gpui-box/generate.lock"), b"owner").unwrap(),
            _ => unreachable!(),
        }
        let before = snapshot(&app);
        apply_plan(&app, &saved, false);
        assert_eq!(before, snapshot(&app), "preserve {changed}");
    }
}

#[test]
fn saved_plan_rejects_unknown_malformed_oversized_and_incompatible_inputs() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let original = dry_run(&app);
    for field in [
        "schema_version",
        "cli_version",
        "upstream_revision",
        "path",
        "unknown",
        "nested",
    ] {
        let mut plan = original.clone();
        match field {
            "schema_version" => plan[field] = 999.into(),
            "cli_version" | "upstream_revision" => plan[field] = "different".into(),
            "path" => plan["files"][0]["path"] = "../outside".into(),
            "unknown" => plan[field] = true.into(),
            "nested" => plan["metadata"]["extra"] = true.into(),
            _ => unreachable!(),
        }
        let saved = save_plan(temporary.path(), &plan);
        let before = snapshot(&app);
        apply_plan(&app, &saved, false);
        assert_eq!(before, snapshot(&app));
    }
    let saved = save_plan(temporary.path(), &original);
    for bytes in [
        b"{broken".to_vec(),
        vec![b' '; 1024 * 1024 + 1],
        b"{\"result\":{}}".to_vec(),
    ] {
        fs::write(&saved, bytes).unwrap();
        let before = snapshot(&app);
        apply_plan(&app, &saved, false);
        assert_eq!(before, snapshot(&app));
    }
}

#[test]
fn recovery_plan_binds_journal_bytes_and_reports_recovery_without_output_rewrites() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let journal = app.join(".gpui-box/pending.json");
    fs::write(&journal, b"{}").unwrap();
    let plan = dry_run(&app);
    assert_eq!(plan["recovering_pending_journal"], true);
    assert_eq!(plan["pending_journal_sha256"].as_str().unwrap().len(), 64);
    let saved = save_plan(temporary.path(), &plan);
    fs::write(&journal, b"{}\n").unwrap();
    let before = snapshot(&app);
    apply_plan(&app, &saved, false);
    assert_eq!(before, snapshot(&app));
    fs::write(&journal, b"{}").unwrap();
    let mut expected = snapshot(&app);
    expected.remove(Path::new(".gpui-box/pending.json"));
    let receipt = apply_plan(&app, &saved, true)["result"].clone();
    assert_eq!(receipt["recovered_pending_journal"], true);
    assert_eq!(receipt["files_written"], serde_json::json!([]));
    assert_eq!(expected, snapshot(&app));
}

#[test]
fn android_install_and_dev_check_devices_before_regenerating_or_building() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let config_path = app.join("gpui-box.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["version"] = "4.0.0".into();
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let before = snapshot(&app);
    for operation in ["dev", "install"] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(&app)
            .args([operation, "android", "--device", "missing", "--json"])
            .env("ANDROID_HOME", temporary.path().join("missing-sdk"))
            .output()
            .unwrap();
        assert!(!output.status.success());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let code = if operation == "dev" {
            "STREAMING_JSON_UNSUPPORTED"
        } else {
            "WORKFLOW_FAILED"
        };
        assert_eq!(response["error"]["code"], code);
        if operation == "install" {
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("Query Android devices")
            );
        }
        assert_eq!(before, snapshot(&app));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&app)
        .args(["dev", "android", "--device", "missing"])
        .env("ANDROID_HOME", temporary.path().join("missing-sdk"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Query Android devices"));
    assert_eq!(before, snapshot(&app));
}

#[test]
fn doctor_reports_failed_checks_and_fixes_without_mutating_project() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let config_path = app.join("gpui-box.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["assets"] = serde_json::json!(["missing-assets"]);
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let before = snapshot(&app);
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&app)
        .args(["doctor", "--json"])
        .env("PATH", temporary.path().join("empty-tool-path"))
        .env("ANDROID_HOME", temporary.path().join("missing-sdk"))
        .env_remove("GPUI_BOX_BUNDLETOOL")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["result"].is_null());
    assert_eq!(response["error"]["code"], "DIAGNOSTICS_FAILED");
    let report = &response["error"]["details"];
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["completed"], true);
    assert_eq!(report["ready"], false);
    assert_eq!(report["project_schema_version"], 1);
    assert_eq!(report["service_schema_version"], 1);
    assert_eq!(report["update_channel"], "development");
    let checks = report["checks"].as_array().unwrap();
    let ids = checks
        .iter()
        .map(|check| check["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), checks.len());
    for id in [
        "asset.0",
        "tool.mise",
        "tool.rustc",
        "tool.cargo",
        "tool.git",
        "android.sdk.licenses/android-sdk-license",
    ] {
        let check = checks.iter().find(|check| check["id"] == id).unwrap();
        assert_eq!(check["passed"], false, "{id}");
        assert_eq!(check["required"], true);
        assert!(!check["corrective_action"].as_str().unwrap().is_empty());
    }
    let wrapper = checks
        .iter()
        .find(|check| check["id"] == "android.wrapper")
        .unwrap();
    assert_eq!(wrapper["passed"], true);
    assert!(wrapper["corrective_action"].is_null());
    let optional = checks
        .iter()
        .find(|check| check["id"] == "release.bundletool")
        .unwrap();
    assert_eq!(optional["required"], false);
    assert_eq!(before, snapshot(&app));
}

#[test]
fn doctor_signing_checks_expose_presence_without_environment_values() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&app)
        .args(["doctor", "--json"])
        .env("PATH", temporary.path().join("empty-tool-path"))
        .env("ANDROID_HOME", temporary.path().join("missing-sdk"))
        .env(
            "GPUI_BOX_ANDROID_KEYSTORE",
            "SENSITIVE_KEYSTORE_PATH_SENTINEL",
        )
        .env(
            "GPUI_BOX_ANDROID_STORE_PASSWORD",
            "SENSITIVE_PASSWORD_SENTINEL",
        )
        .env("GPUI_BOX_ANDROID_KEY_ALIAS", "SENSITIVE_ALIAS_SENTINEL")
        .env("GPUI_BOX_ANDROID_KEY_PASSWORD", "")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SENSITIVE_"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SENSITIVE_"));
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let checks = response["error"]["details"]["checks"].as_array().unwrap();
    for name in ["KEYSTORE", "STORE_PASSWORD", "KEY_ALIAS", "KEY_PASSWORD"] {
        let id = format!("release.GPUI_BOX_ANDROID_{name}");
        let check = checks.iter().find(|check| check["id"] == id).unwrap();
        assert_eq!(check["passed"], name != "KEY_PASSWORD");
        assert_eq!(check["required"], false);
    }
}

#[test]
fn build_plans_are_deterministic_read_only_and_use_real_command_arguments() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let config_path = app.join("gpui-box.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["version"] = "5.0.0".into();
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let before = snapshot(&app);
    for platform in ["desktop", "android"] {
        let mut plans = Vec::new();
        for _ in 0..2 {
            let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
                .current_dir(&app)
                .args(["build", platform, "--dry-run", "--json"])
                .env("PATH", temporary.path().join("missing-tools"))
                .env("ANDROID_HOME", temporary.path().join("missing-sdk"))
                .env("GPUI_BOX_ANDROID_STORE_PASSWORD", "SECRET_PLAN_SENTINEL")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET_PLAN_SENTINEL"));
            let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            let plan = response["result"].clone();
            assert_eq!(plan["schema_version"], 1);
            assert_eq!(plan["operation"], "build");
            assert_eq!(plan["target"], platform);
            assert_eq!(plan["command"]["program"], "mise");
            assert_eq!(plan["command"]["inherits_environment"], true);
            assert_eq!(
                plan["command"]["environment_overrides"],
                serde_json::json!({})
            );
            assert!(
                plan["generation"]["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|file| file["action"] == "update")
            );
            let arguments = plan["command"]["arguments"].as_array().unwrap();
            if platform == "desktop" {
                assert_eq!(
                    plan["command"]["arguments"],
                    serde_json::json!(["exec", "--", "cargo", "build", "-p", "box-desktop"])
                );
            } else {
                assert_eq!(arguments.last().unwrap(), ":app:assembleDebug");
                assert_eq!(arguments[0], "-E");
                assert_eq!(arguments[1], "android");
                let wrapper = PathBuf::from(arguments[6].as_str().unwrap());
                assert!(wrapper.ends_with("android-host/gradle/wrapper/gradle-wrapper.jar"));
                assert_eq!(
                    plan["toolchain_targets"],
                    serde_json::json!(["aarch64-linux-android", "x86_64-linux-android"])
                );
            }
            plans.push(plan);
            assert_eq!(before, snapshot(&app));
        }
        assert_eq!(plans[0], plans[1]);
    }
}

#[test]
fn build_plan_handles_uninitialized_projects_and_rejects_disabled_targets_and_conflicts() {
    let temporary = tempfile::tempdir().unwrap();
    let seed = create(temporary.path());
    let app = temporary.path().join("Existing source project");
    fs::create_dir(&app).unwrap();
    fs::copy(seed.join("gpui-box.json"), app.join("gpui-box.json")).unwrap();
    let before = snapshot(&app);
    let output = cli(&app, &["build", "android", "--dry-run", "--json"]);
    assert!(output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        response["result"]["generation"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["action"] == "create")
    );
    assert_eq!(before, snapshot(&app));
    for failure in ["disabled", "managed", "lock", "unknown"] {
        let failure_root = tempfile::tempdir().unwrap();
        let seed = create(failure_root.path());
        let config_path = seed.join("gpui-box.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        if failure == "disabled" {
            config["targets"] = serde_json::json!(["desktop"]);
            fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
        }
        if failure == "managed" {
            fs::write(seed.join("Cargo.toml"), "// conflict").unwrap();
        }
        if failure == "lock" {
            fs::write(seed.join(".gpui-box/generate.lock"), "owner").unwrap();
        }
        let before = snapshot(&seed);
        let platform = if failure == "unknown" {
            "web"
        } else {
            "android"
        };
        let output = cli(&seed, &["build", platform, "--dry-run", "--json"]);
        assert!(!output.status.success());
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], "BUILD_PLAN_FAILED");
        assert_eq!(before, snapshot(&seed));
    }
}

#[cfg(windows)]
#[test]
fn interrupted_plan_apply_requires_new_preview_and_ordinary_generate_recovers() {
    use std::os::windows::fs::OpenOptionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let config_path = app.join("gpui-box.json");
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    config["version"] = "3.0.0".into();
    fs::write(config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    let saved = save_plan(temporary.path(), &dry_run(&app));
    let state_path = app.join(".gpui-box/generated.json");
    let state = fs::read(&state_path).unwrap();
    // Allow preflight reads but deny replacement of a later managed output.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(app.join("desktop/Cargo.toml"))
        .unwrap();
    let old_manifest = fs::read(app.join("desktop/Cargo.toml")).unwrap();
    let response = apply_plan(&app, &saved, false);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Write")
    );
    assert!(app.join(".gpui-box/pending.json").is_file());
    assert!(!app.join(".gpui-box/generate.lock").exists());
    assert_eq!(fs::read(&state_path).unwrap(), state);
    assert_eq!(
        fs::read(app.join("desktop/Cargo.toml")).unwrap(),
        old_manifest
    );
    assert!(
        fs::read_to_string(app.join("app/Cargo.toml"))
            .unwrap()
            .contains("3.0.0")
    );
    drop(held);
    let before = snapshot(&app);
    apply_plan(&app, &saved, false);
    assert_eq!(before, snapshot(&app));
    let output = cli(&app, &["generate", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["result"]["recovered_pending_journal"], true);
    assert_eq!(
        response["result"]["files_written"],
        serde_json::json!(["desktop/Cargo.toml", ".gpui-box/generated.json"])
    );
    assert!(!app.join(".gpui-box/pending.json").exists());
    assert!(
        fs::read_to_string(app.join("desktop/Cargo.toml"))
            .unwrap()
            .contains("3.0.0")
    );
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

#[test]
fn workbench_template_preserves_feature_sources_and_selected_dependencies() {
    let temporary = tempfile::tempdir().unwrap();
    for targets in ["desktop", "android", "desktop,android"] {
        let app = temporary
            .path()
            .join(format!("Workbench {targets} 有 空格"));
        let output = cli(
            temporary.path(),
            &[
                "new",
                app.to_str().unwrap(),
                "--distribution",
                distribution().to_str().unwrap(),
                "--template",
                "workbench",
                "--targets",
                targets,
                "--json",
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let provenance = fs::read(app.join(".gpui-box/template.json")).unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&provenance).unwrap();
        assert_eq!(metadata["template"], "workbench");
        assert_eq!(metadata["template_version"], 1);
        let manifest = fs::read_to_string(app.join("app/Cargo.toml")).unwrap();
        assert!(manifest.contains("features = [\"menus\"]"));
        let mut customized = Vec::new();
        for name in [
            "lib.rs",
            "contracts.rs",
            "documents.rs",
            "search.rs",
            "providers.rs",
        ] {
            let path = app.join("app/src").join(name);
            let source = fs::read_to_string(&path).unwrap();
            if name == "contracts.rs" {
                assert!(!source.contains("gpui"));
            }
            if name == "documents.rs" {
                assert!(!source.contains("crate::search"));
            }
            if name == "search.rs" {
                assert!(!source.contains("crate::documents"));
            }
            let source = format!("{source}\n// Application-owned customization\n");
            fs::write(&path, &source).unwrap();
            customized.push((path, source));
        }
        let output = cli(&app, &["generate", "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["result"]["files_written"], serde_json::json!([]));
        for (path, source) in customized {
            assert_eq!(fs::read_to_string(path).unwrap(), source);
        }
        assert_eq!(
            fs::read(app.join(".gpui-box/template.json")).unwrap(),
            provenance
        );
        assert_eq!(app.join("desktop").exists(), targets.contains("desktop"));
        assert_eq!(app.join("android").exists(), targets.contains("android"));
    }
    let existing = temporary.path().join("Existing workbench");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("README.md"), "Existing docs").unwrap();
    let output = cli(
        temporary.path(),
        &[
            "init",
            existing.to_str().unwrap(),
            "--distribution",
            distribution().to_str().unwrap(),
            "--template",
            "workbench",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(existing.join("README.md")).unwrap(),
        "Existing docs"
    );
    assert!(existing.join("app/src/search.rs").is_file());
}

#[test]
fn template_provenance_failures_are_read_only_and_legacy_apps_remain_compatible() {
    let temporary = tempfile::tempdir().unwrap();
    let app = create(temporary.path());
    let provenance = app.join(".gpui-box/template.json");
    let original = fs::read(&provenance).unwrap();
    let manifest = fs::read(app.join("app/Cargo.toml")).unwrap();
    for invalid in [
        b"invalid".to_vec(),
        vec![b' '; 16385],
        br#"{"schema_version":2,"template_version":1,"template":"starter"}"#.to_vec(),
        br#"{"schema_version":1,"template_version":1,"template":"unknown"}"#.to_vec(),
    ] {
        fs::write(&provenance, &invalid).unwrap();
        for args in [
            vec!["generate", "--dry-run", "--json"],
            vec!["generate", "--json"],
        ] {
            assert!(!cli(&app, &args).status.success());
            assert_eq!(fs::read(&provenance).unwrap(), invalid);
            assert_eq!(fs::read(app.join("app/Cargo.toml")).unwrap(), manifest);
        }
    }
    fs::write(&provenance, original).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    fs::remove_file(provenance).unwrap();
    assert!(cli(&app, &["generate"]).status.success());
    assert_eq!(fs::read(app.join("app/Cargo.toml")).unwrap(), manifest);
}
