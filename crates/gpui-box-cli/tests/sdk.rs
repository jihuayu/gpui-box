use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const REQUIRED: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "templates/app.rs",
    "components/gpui-box-components/Cargo.toml",
    "crates/gpui-box/Cargo.toml",
    "vendor/gpui/UPSTREAM.json",
    "platforms/android/gradle/wrapper/gradle-wrapper.jar",
];

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn source(root: &Path) -> String {
    fs::create_dir(root).unwrap();
    git(root, &["init"]);
    for name in REQUIRED {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"committed source\n").unwrap();
    }
    fs::write(
        root.join("vendor/gpui/UPSTREAM.json"),
        serde_json::to_vec(&serde_json::json!({
            "revision": gpui_box::UPSTREAM_REVISION
        }))
        .unwrap(),
    )
    .unwrap();
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=SDK Test",
            "-c",
            "user.email=sdk@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Fixture",
        ],
    );
    git(root, &["rev-parse", "HEAD"])
}

fn cli(arguments: &[&str], success: bool) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .arg("--json")
        .args(arguments)
        .output()
        .unwrap();
    response(output, success)
}

fn response(output: Output, success: bool) -> serde_json::Value {
    assert_eq!(
        output.status.success(),
        success,
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["success"], success);
    value
}

fn export(source: &Path, destination: &Path, success: bool) -> serde_json::Value {
    cli(
        &[
            "sdk",
            "export",
            destination.to_str().unwrap(),
            "--source",
            source.to_str().unwrap(),
        ],
        success,
    )
}

fn cache_add(sdk: &Path, cache: &Path, success: bool) -> serde_json::Value {
    cli(
        &[
            "sdk",
            "cache",
            "add",
            sdk.to_str().unwrap(),
            "--cache",
            cache.to_str().unwrap(),
        ],
        success,
    )
}

#[test]
fn sdk_cache_is_content_addressed_reused_offline_and_detects_changed_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().join("source");
    let revision = source(&checkout);
    let sdk = directory.path().join("SDK 有 空格");
    export(&checkout, &sdk, true);
    let cache = directory.path().join("Cache 有 空格");
    let first = cache_add(&sdk, &cache, true)["result"].clone();
    assert_eq!(first["created"], true);
    assert_eq!(first["verified"], true);
    assert_eq!(first["manifest"]["source_revision"], revision);
    let address = first["content_sha256"].as_str().unwrap();
    let cached = cache.join(address);
    assert_eq!(
        Path::new(first["path"].as_str().unwrap()),
        cached.canonicalize().unwrap()
    );
    assert_eq!(
        fs::read(cached.join("Cargo.toml")).unwrap(),
        b"committed source\n"
    );
    let repeated = cache_add(&sdk, &cache, true);
    assert_eq!(repeated["result"]["created"], false);
    assert_eq!(repeated["result"]["content_sha256"], address);
    let manifest_path = sdk.join("gpui-box-sdk.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let reformatted = cache_add(&sdk, &cache, true);
    assert_eq!(reformatted["result"]["created"], false);
    assert_eq!(reformatted["result"]["content_sha256"], address);
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .env("PATH", "")
        .args([
            "sdk",
            "cache",
            "verify",
            address,
            "--cache",
            cache.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(response(output, true)["result"]["verified"], true);
    fs::write(cached.join("Cargo.toml"), b"edited cache content").unwrap();
    assert_eq!(
        cache_add(&sdk, &cache, false)["error"]["code"],
        "SDK_CACHE_FAILED"
    );
    assert_eq!(
        fs::read(cached.join("Cargo.toml")).unwrap(),
        b"edited cache content"
    );
    assert_eq!(fs::read_dir(&cache).unwrap().count(), 2);
    assert_eq!(
        fs::read(sdk.join("Cargo.toml")).unwrap(),
        b"committed source\n"
    );
}

#[test]
fn sdk_cache_rejects_containment_invalid_addresses_and_active_writer_without_source_changes() {
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().join("source");
    source(&checkout);
    let sdk = directory.path().join("SDK");
    export(&checkout, &sdk, true);
    assert_eq!(
        cache_add(&sdk, &sdk.join("nested-cache"), false)["error"]["code"],
        "SDK_CACHE_FAILED"
    );
    assert!(!sdk.join("nested-cache").exists());
    let cache = directory.path().join("cache");
    for address in ["../outside", "short", &"A".repeat(64)] {
        let result = cli(
            &[
                "sdk",
                "cache",
                "verify",
                address,
                "--cache",
                cache.to_str().unwrap(),
            ],
            false,
        );
        assert_eq!(result["error"]["code"], "SDK_CACHE_FAILED");
    }
    assert!(!cache.exists());
    fs::create_dir(&cache).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(cache.join(".writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let result = cache_add(&sdk, &cache, false);
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("writer")
    );
    drop(lock);
    // Incomplete staging is retained, ignored as content and never adopted.
    let abandoned = cache.join(".staging-interrupted");
    fs::create_dir(&abandoned).unwrap();
    fs::write(abandoned.join("original"), b"retain me").unwrap();
    assert_eq!(cache_add(&sdk, &cache, true)["result"]["created"], true);
    assert_eq!(fs::read(abandoned.join("original")).unwrap(), b"retain me");
}

#[test]
fn sdk_cache_rejects_a_manifest_under_the_wrong_content_address() {
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().join("source");
    source(&checkout);
    let sdk = directory.path().join("SDK");
    export(&checkout, &sdk, true);
    let cache = directory.path().join("cache");
    let result = cache_add(&sdk, &cache, true);
    let address = result["result"]["content_sha256"].as_str().unwrap();
    let wrong = "0".repeat(64);
    fs::rename(cache.join(address), cache.join(&wrong)).unwrap();
    let response = cli(
        &[
            "sdk",
            "cache",
            "verify",
            &wrong,
            "--cache",
            cache.to_str().unwrap(),
        ],
        false,
    );
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("content address")
    );
    assert!(cache.join(wrong).is_dir());
}

#[cfg(unix)]
#[test]
fn sdk_cache_rejects_linked_roots_entries_and_lock_files() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().join("source");
    source(&checkout);
    let sdk = directory.path().join("SDK");
    export(&checkout, &sdk, true);
    let outside = directory.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let alias = directory.path().join("alias");
    symlink(&outside, &alias).unwrap();
    cache_add(&sdk, &alias, false);
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    let cache = directory.path().join("cache");
    fs::create_dir(&cache).unwrap();
    let victim = directory.path().join("victim");
    fs::write(&victim, b"original").unwrap();
    symlink(&victim, cache.join(".writer.lock")).unwrap();
    cache_add(&sdk, &cache, false);
    assert_eq!(fs::read(&victim).unwrap(), b"original");
    fs::remove_file(cache.join(".writer.lock")).unwrap();
    let address = "0".repeat(64);
    symlink(&sdk, cache.join(&address)).unwrap();
    cli(
        &[
            "sdk",
            "cache",
            "verify",
            &address,
            "--cache",
            cache.to_str().unwrap(),
        ],
        false,
    );
}

fn verify(root: &Path, success: bool) -> serde_json::Value {
    cli(&["sdk", "verify", root.to_str().unwrap()], success)
}

#[test]
fn export_is_deterministic_excludes_wip_and_preserves_existing_destinations() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let source_path = root.join("Source 有 空格");
    let revision = source(&source_path);
    fs::write(source_path.join("LICENSE"), b"uncommitted modification").unwrap();
    fs::write(source_path.join("untracked.txt"), b"local only").unwrap();
    git(&source_path, &["config", "core.autocrlf", "true"]);
    let first = root.join("SDK 有 空格");
    let second = root.join("SDK second");
    let manifest = export(&source_path, &first, true);
    assert_eq!(manifest["result"]["source_revision"], revision);
    assert_eq!(
        manifest["result"]["upstream_revision"],
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(
        manifest["result"]["files"].as_object().unwrap().len(),
        REQUIRED.len()
    );
    assert_eq!(
        fs::read(first.join("LICENSE")).unwrap(),
        b"committed source\n"
    );
    assert!(!first.join("untracked.txt").exists());
    assert!(!first.join(".git").exists());
    assert_eq!(verify(&first, true)["result"], manifest["result"]);
    git(&source_path, &["config", "core.autocrlf", "false"]);
    assert_eq!(
        export(&source_path, &second, true)["result"],
        manifest["result"]
    );
    let bytes = fs::read(first.join("gpui-box-sdk.json")).unwrap();
    assert_eq!(bytes, fs::read(second.join("gpui-box-sdk.json")).unwrap());
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .contains(&source_path.to_string_lossy().to_string())
    );
    assert_eq!(
        export(&source_path, &first, false)["error"]["code"],
        "SDK_EXPORT_FAILED"
    );
    assert_eq!(fs::read(first.join("gpui-box-sdk.json")).unwrap(), bytes);
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
}

#[test]
fn verify_rejects_changed_missing_extra_and_unsupported_manifest_content() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("source");
    source(&source_path);
    let sdk = temporary.path().join("sdk");
    export(&source_path, &sdk, true);
    let license = sdk.join("LICENSE");
    fs::write(&license, b"modified").unwrap();
    assert!(
        verify(&sdk, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("digest mismatch")
    );
    fs::remove_file(&license).unwrap();
    assert!(
        verify(&sdk, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("missing")
    );
    fs::write(&license, b"committed source\n").unwrap();
    let extra = sdk.join("extra.rs");
    fs::write(&extra, b"unlisted").unwrap();
    assert!(
        verify(&sdk, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unlisted")
    );
    fs::remove_file(extra).unwrap();
    let manifest_path = sdk.join("gpui-box-sdk.json");
    let original = fs::read(&manifest_path).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    manifest["schema_version"] = 99.into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        verify(&sdk, false)["error"]["code"],
        "SDK_VERIFICATION_FAILED"
    );
    manifest["schema_version"] = 1.into();
    manifest["files"]["../escape"] = "a".repeat(64).into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        verify(&sdk, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("portable")
    );
    fs::write(manifest_path, original).unwrap();
    verify(&sdk, true);
}

#[test]
fn generator_checks_exported_sdk_before_using_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("source");
    source(&source_path);
    let sdk = temporary.path().join("sdk");
    export(&source_path, &sdk, true);
    fs::write(sdk.join("LICENSE"), b"modified").unwrap();
    let app = temporary.path().join("app");
    let result = cli(
        &[
            "new",
            app.to_str().unwrap(),
            "--distribution",
            sdk.to_str().unwrap(),
        ],
        false,
    );
    assert!(
        result["error"]["message"]
            .as_str()
            .unwrap()
            .contains("digest mismatch")
    );
    assert!(!app.exists());
}

#[test]
fn export_rejects_incomplete_sources_subdirectories_and_nonportable_names() {
    for invalid in ["missing", "subdirectory", "NUL.txt"] {
        let temporary = tempfile::tempdir().unwrap();
        let source_path = temporary.path().join("source");
        source(&source_path);
        let input = if invalid == "subdirectory" {
            source_path.join("crates")
        } else {
            source_path.clone()
        };
        if invalid == "missing" {
            git(&source_path, &["rm", "Cargo.lock"]);
        } else if invalid == "NUL.txt" {
            // Create a reserved-name tree through Git without writing a Windows device path.
            let blob = git(&source_path, &["rev-parse", "HEAD:LICENSE"]);
            git(
                &source_path,
                &[
                    "-c",
                    "core.protectNTFS=false",
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    "100644",
                    &blob,
                    "NUL.txt",
                ],
            );
        }
        if invalid != "subdirectory" {
            git(
                &source_path,
                &[
                    "-c",
                    "user.name=SDK Test",
                    "-c",
                    "user.email=sdk@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "-m",
                    "Invalid fixture",
                ],
            );
        }
        let destination = temporary.path().join("sdk");
        assert_eq!(
            export(&input, &destination, false)["error"]["code"],
            "SDK_EXPORT_FAILED"
        );
        assert!(!destination.exists());
    }
}

#[test]
fn checkout_local_archive_overrides_are_rejected_before_staging() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("source");
    source(&source_path);
    fs::write(
        source_path.join(".git/info/attributes"),
        b"LICENSE export-ignore\n",
    )
    .unwrap();
    let destination = temporary.path().join("sdk");
    assert!(
        export(&source_path, &destination, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("info/attributes")
    );
    assert_eq!(fs::read_dir(temporary.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn linked_files_are_rejected_during_export_and_verification() {
    let temporary = tempfile::tempdir().unwrap();
    let source_path = temporary.path().join("source");
    source(&source_path);
    let sdk = temporary.path().join("sdk");
    export(&source_path, &sdk, true);
    fs::remove_file(sdk.join("LICENSE")).unwrap();
    std::os::unix::fs::symlink(source_path.join("LICENSE"), sdk.join("LICENSE")).unwrap();
    assert!(
        verify(&sdk, false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("link")
    );
    std::os::unix::fs::symlink("LICENSE", source_path.join("linked")).unwrap();
    git(&source_path, &["add", "linked"]);
    git(
        &source_path,
        &[
            "-c",
            "user.name=SDK Test",
            "-c",
            "user.email=sdk@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Linked fixture",
        ],
    );
    export(&source_path, &temporary.path().join("rejected"), false);
}
