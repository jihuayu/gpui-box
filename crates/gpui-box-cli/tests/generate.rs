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
