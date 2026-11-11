//! Inspected application packages. Signing material stays in environment variables.
use crate::{config::AppConfig, workflow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

const WRAPPER_SHA256: &str = "81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f";
const GRADLE_SHA256: &str = "20f1b1176237254a6fc204d8434196fa11a4cfb387567519c61556e8710aed78";
const BUNDLETOOL_SHA256: &str = "a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29";

pub fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn verify_wrapper(host: &Path) -> Result<(), String> {
    if digest(&host.join("gradle/wrapper/gradle-wrapper.jar"))? != WRAPPER_SHA256 {
        return Err(
            "Gradle Wrapper JAR checksum mismatch. Regenerate from the pinned distribution.".into(),
        );
    }
    let properties = fs::read_to_string(host.join("gradle/wrapper/gradle-wrapper.properties"))
        .map_err(|error| error.to_string())?;
    // Java properties support ':'/whitespace separators, escapes and
    // continuations. Compare the entire managed file to reject shadow entries.
    let expected =
        include_str!("../../../platforms/android/gradle/wrapper/gradle-wrapper.properties");
    if properties.replace("\r\n", "\n") != expected.replace("\r\n", "\n")
        || !properties.contains(GRADLE_SHA256)
    {
        return Err("Gradle Wrapper distribution pin/checksum is invalid".into());
    }
    Ok(())
}

pub fn gradle(root: &Path, tasks: &[&str]) -> Result<Command, String> {
    let host = root.join("android-host");
    verify_wrapper(&host)?;
    // Invoke the verified wrapper directly through Java on every host. This
    // avoids Windows batch argument parsing and executable-bit migration issues.
    let mut command = Command::new("mise");
    command
        .current_dir(root)
        .args(["-E", "android", "exec", "--", "java", "-classpath"])
        .arg(host.join("gradle/wrapper/gradle-wrapper.jar"))
        .args(["org.gradle.wrapper.GradleWrapperMain", "-p", "android-host"])
        .args(tasks);
    Ok(command)
}

fn java(root: &Path) -> Command {
    let mut command = Command::new("mise");
    command
        .current_dir(root)
        .args(["-E", "android", "exec", "--", "java"]);
    command
}

fn output(command: &mut Command, operation: &str) -> Result<String, String> {
    let result = command
        .output()
        .map_err(|error| format!("{operation}: {error}"))?;
    if !result.status.success() {
        return Err(format!("{operation} failed ({})", result.status));
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

pub(crate) fn bundletool() -> Result<PathBuf, String> {
    let path = env::var_os("GPUI_BOX_BUNDLETOOL").map(PathBuf::from)
        .ok_or("Set GPUI_BOX_BUNDLETOOL to bundletool-all-1.18.3.jar. See the release workflow documentation for its verified source and SHA-256.")?;
    if digest(&path)? != BUNDLETOOL_SHA256 {
        return Err("Bundletool checksum mismatch".into());
    }
    Ok(path)
}

fn signing_inputs(root: &Path) -> Result<(), String> {
    for name in [
        "GPUI_BOX_ANDROID_KEYSTORE",
        "GPUI_BOX_ANDROID_STORE_PASSWORD",
        "GPUI_BOX_ANDROID_KEY_ALIAS",
        "GPUI_BOX_ANDROID_KEY_PASSWORD",
    ] {
        if env::var_os(name).is_none_or(|value| value.is_empty()) {
            return Err(format!("Release packaging requires {name}"));
        }
    }
    let configured =
        PathBuf::from(env::var_os("GPUI_BOX_ANDROID_KEYSTORE").ok_or("Missing keystore")?);
    if !configured.is_absolute() {
        return Err("GPUI_BOX_ANDROID_KEYSTORE must be an absolute path".into());
    }
    let store = root.join(configured);
    if !store.is_file() {
        return Err("Release keystore is unavailable".into());
    }
    if env::var("GPUI_BOX_ANDROID_KEY_ALIAS")
        .is_ok_and(|alias| alias.eq_ignore_ascii_case("androiddebugkey"))
    {
        return Err("Release packaging rejects Android debug signing".into());
    }
    Ok(())
}

fn signer(root: &Path, sdk: &Path, apk: &Path) -> Result<String, String> {
    let text = output(
        java(root)
            .arg("-cp")
            .arg(sdk.join("build-tools/36.0.0/lib/apksigner.jar"))
            .args([
                "com.android.apksigner.ApkSignerTool",
                "verify",
                "--verbose",
                "--print-certs",
            ])
            .arg(apk),
        "APK signature verification",
    )?;
    if text.lines().any(|line| {
        line.contains("certificate DN:") && line.to_ascii_lowercase().contains("android debug")
    }) {
        return Err("Release packaging rejects the Android debug certificate".into());
    }
    let signers: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix("Signer #1 certificate SHA-256 digest: "))
        .collect();
    let hash = signers
        .first()
        .ok_or("APK signer digest is missing")?
        .trim()
        .to_ascii_lowercase();
    if signers.len() != 1
        || hash.len() != 64
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || text.contains("Signer #2 certificate")
    {
        return Err("Expected one unambiguous release signer".into());
    }
    Ok(hash)
}

fn configured_signer(root: &Path) -> Result<String, String> {
    let text = output(
        Command::new("mise")
            .current_dir(root)
            .args([
                "-E",
                "android",
                "exec",
                "--",
                "keytool",
                "-J-Duser.language=en",
                "-list",
                "-v",
                "-keystore",
            ])
            .arg(env::var_os("GPUI_BOX_ANDROID_KEYSTORE").ok_or("Missing keystore")?)
            .arg("-alias")
            .arg(env::var_os("GPUI_BOX_ANDROID_KEY_ALIAS").ok_or("Missing key alias")?)
            .args(["-storepass:env", "GPUI_BOX_ANDROID_STORE_PASSWORD"]),
        "Release signing identity inspection",
    )?;
    if !text.contains("Entry type: PrivateKeyEntry")
        || text.to_ascii_lowercase().contains("cn=android debug")
    {
        return Err("Expected a release private-key entry with a non-debug certificate".into());
    }
    let fingerprint = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("SHA256: "))
        .ok_or("Keystore signer digest is missing")?;
    let fingerprint = fingerprint.replace(':', "").to_ascii_lowercase();
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid keystore signer digest".into());
    }
    Ok(fingerprint)
}

fn number(bytes: &[u8], offset: usize, length: usize) -> Result<u64, String> {
    let data = bytes
        .get(offset..offset.checked_add(length).ok_or("Invalid ELF offset")?)
        .ok_or("Truncated ELF header")?;
    Ok(data.iter().enumerate().fold(0, |value, (index, byte)| {
        value | ((*byte as u64) << (index * 8))
    }))
}

/// Verify ELF64 PT_LOAD alignment and expected ABI without executing native code.
fn inspect_elf(bytes: &[u8], machine: u64) -> Result<(), String> {
    if bytes.get(..6) != Some(&[0x7f, b'E', b'L', b'F', 2, 1]) || number(bytes, 18, 2)? != machine {
        return Err("Native library ELF class/byte order/ABI mismatch".into());
    }
    let offset =
        usize::try_from(number(bytes, 32, 8)?).map_err(|_| "Invalid ELF program offset")?;
    let size = number(bytes, 54, 2)? as usize;
    let count = number(bytes, 56, 2)? as usize;
    if size < 56 || count == 0 {
        return Err("Native library has no valid program headers".into());
    }
    let mut loads = 0;
    for index in 0..count {
        let start = offset
            .checked_add(index.checked_mul(size).ok_or("Invalid ELF program table")?)
            .ok_or("Invalid ELF program table")?;
        if number(bytes, start, 4)? != 1 {
            continue;
        }
        loads += 1;
        let alignment = number(bytes, start + 48, 8)?;
        if alignment < 16384
            || !alignment.is_power_of_two()
            || number(bytes, start + 8, 8)? % alignment != number(bytes, start + 16, 8)? % alignment
        {
            return Err("Native library fails 16 KB PT_LOAD alignment".into());
        }
    }
    if loads == 0 {
        return Err("Native library has no PT_LOAD segments".into());
    }
    Ok(())
}

fn native_libraries(apk: &Path, prefix: &str) -> Result<Value, String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(apk).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    for abi in ["arm64-v8a", "x86_64"] {
        archive
            .by_name(&format!("{prefix}{abi}/libgpui_box_android.so"))
            .map_err(|_| format!("Missing native library for {abi}"))?;
    }
    let names: Vec<_> = archive
        .file_names()
        .filter(|name| name.starts_with(prefix) && name.ends_with(".so"))
        .map(str::to_owned)
        .collect();
    let mut found = Vec::new();
    for name in names {
        let relative = name
            .strip_prefix(prefix)
            .ok_or("Invalid native library prefix")?;
        let abi = relative
            .split('/')
            .next()
            .ok_or("Invalid native library path")?;
        let machine = match abi {
            "arm64-v8a" => 183,
            "x86_64" => 62,
            _ => return Err("Unexpected native library ABI".into()),
        };
        let mut entry = archive
            .by_name(&name)
            .map_err(|_| format!("Missing native library for {abi}"))?;
        if entry.size() > 1024 * 1024 * 1024 {
            return Err("Native library exceeds inspection limit".into());
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        inspect_elf(&bytes, machine)?;
        found.push(json!({"abi": abi, "path": relative, "size": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes)), "elf_load_alignment": 16384}));
    }
    found.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    Ok(json!(found))
}

fn inspect_manifest(text: &str, config: &AppConfig) -> Result<(), String> {
    const ANDROID: &str = "http://schemas.android.com/apk/res/android";
    let document = roxmltree::Document::parse(text).map_err(|error| error.to_string())?;
    let manifest = document.root_element();
    let code = config.version_code.to_string();
    if manifest.tag_name().name() != "manifest"
        || manifest.attribute("package") != Some(&config.application_id)
        || manifest.attribute((ANDROID, "versionCode")) != Some(code.as_str())
        || manifest.attribute((ANDROID, "versionName")) != Some(&config.version)
    {
        return Err("AAB identity/version does not match configuration".into());
    }
    let sdk = manifest
        .children()
        .find(|node| node.has_tag_name("uses-sdk"))
        .ok_or("Missing SDK declaration")?;
    if sdk.attribute((ANDROID, "minSdkVersion")) != Some("35")
        || sdk.attribute((ANDROID, "targetSdkVersion")) != Some("35")
    {
        return Err("AAB SDK declaration mismatch".into());
    }
    let application = manifest
        .children()
        .find(|node| node.has_tag_name("application"))
        .ok_or("Missing application")?;
    if application.attribute((ANDROID, "debuggable")) == Some("true")
        || application.attribute((ANDROID, "testOnly")) == Some("true")
        || application.attribute((ANDROID, "allowBackup")) != Some("false")
        || application.attribute((ANDROID, "extractNativeLibs")) != Some("false")
    {
        return Err("AAB release application flags are invalid".into());
    }
    let launcher = application
        .children()
        .find(|node| {
            node.has_tag_name("activity")
                && node.attribute((ANDROID, "name")) == Some("dev.gpui.box.MainActivity")
        })
        .ok_or("Missing main Activity")?;
    if launcher.attribute((ANDROID, "exported")) != Some("true")
        || !launcher.descendants().any(|node| {
            node.has_tag_name("category")
                && node.attribute((ANDROID, "name")) == Some("android.intent.category.LAUNCHER")
        })
    {
        return Err("AAB launcher is not exported/configured".into());
    }
    for component in application.children().filter(|node| node.is_element()) {
        if component != launcher && component.attribute((ANDROID, "exported")) == Some("true") {
            return Err("Additional exported components require release policy review".into());
        }
    }
    for permission in config
        .services
        .permissions
        .iter()
        .flat_map(|declaration| declaration.kind.android_permissions())
        .copied()
        .chain(
            config
                .permissions
                .iter()
                .map(|_| "android.permission.INTERNET"),
        )
    {
        if !manifest.children().any(|node| {
            node.has_tag_name("uses-permission")
                && node.attribute((ANDROID, "name")) == Some(permission)
        }) {
            return Err(format!(
                "Requested permission missing from release manifest: {permission}"
            ));
        }
    }
    for scheme in &config.services.url_schemes {
        if !launcher.descendants().any(|node| {
            node.has_tag_name("data")
                && node.attribute((ANDROID, "scheme")) == Some(scheme.as_str())
        }) {
            return Err("Requested custom scheme missing from release manifest".into());
        }
    }
    Ok(())
}

fn inventory(
    root: &Path,
    distribution: &Path,
    destination: &Path,
    target_platform: &str,
) -> Result<Value, String> {
    let text = output(
        Command::new("mise").current_dir(root).args([
            "exec",
            "--",
            "cargo",
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            target_platform,
        ]),
        "Dependency inventory",
    )?;
    let metadata: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let licenses = destination.join("licenses");
    fs::create_dir(&licenses).map_err(|error| error.to_string())?;
    let supplement_root = distribution.join("tooling/licenses");
    let supplement: Value = serde_json::from_slice(
        &fs::read(supplement_root.join("sources.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut missing = Vec::new();
    let application_root = root.canonicalize().map_err(|error| error.to_string())?;
    let distribution_root = distribution
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut packages = Vec::new();
    for package in metadata["packages"]
        .as_array()
        .ok_or("Invalid cargo metadata packages")?
    {
        let name = package["name"].as_str().ok_or("Missing package name")?;
        let version = package["version"]
            .as_str()
            .ok_or("Missing package version")?;
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("Missing manifest path")?,
        );
        let directory = manifest.parent().ok_or("Missing package directory")?;
        let id = format!("{name}-{version}-{}", packages.len());
        let target = licenses.join(&id);
        fs::create_dir(&target).map_err(|error| error.to_string())?;
        let mut texts = Vec::new();
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let filename = entry.file_name();
            let upper = filename.to_string_lossy().to_ascii_uppercase();
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
                && ["LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT"]
                    .iter()
                    .any(|prefix| upper.starts_with(prefix))
            {
                fs::copy(entry.path(), target.join(&filename))
                    .map_err(|error| error.to_string())?;
                texts.push(filename.to_string_lossy().into_owned());
            }
        }
        if let Some(file) = package["license_file"].as_str() {
            let file = directory.join(file);
            if file.is_file() {
                fs::copy(file, target.join("DECLARED-LICENSE"))
                    .map_err(|error| error.to_string())?;
                texts.push("DECLARED-LICENSE".into());
            }
        }
        let mut provenance = Vec::new();
        // Local application and distribution packages inherit their owning
        // workspace's Apache license. Upstream packages retain their own files.
        if texts.is_empty() && package["source"].is_null() && package["license"] == "Apache-2.0" {
            let canonical = directory
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let owner = if canonical.starts_with(distribution_root.join("crates"))
                || canonical.starts_with(distribution_root.join("components"))
            {
                Some(distribution)
            } else if canonical.starts_with(&application_root) {
                Some(root)
            } else {
                None
            };
            if let Some(owner) = owner {
                fs::copy(owner.join("LICENSE"), target.join("WORKSPACE-LICENSE"))
                    .map_err(|error| error.to_string())?;
                texts.push("WORKSPACE-LICENSE".into());
                provenance.push(json!({"kind": "owning_workspace_license"}));
            }
        }
        for source in supplement["files"]
            .as_array()
            .ok_or("Invalid license supplement")?
        {
            if !package["source"]
                .as_str()
                .is_some_and(|source| source.starts_with("registry+"))
            {
                continue;
            }
            if !source["packages"].as_array().is_some_and(|packages| {
                packages
                    .iter()
                    .any(|package| package["name"] == name && package["version"] == version)
            }) {
                continue;
            }
            let relative = Path::new(
                source["path"]
                    .as_str()
                    .ok_or("Missing license supplement path")?,
            );
            if relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err("Invalid license supplement path".into());
            }
            let file = supplement_root.join(relative);
            if digest(&file)?
                != source["sha256"]
                    .as_str()
                    .ok_or("Missing license supplement hash")?
            {
                return Err("License supplement checksum mismatch".into());
            }
            let filename = relative
                .file_name()
                .ok_or("Missing license supplement name")?;
            fs::copy(file, target.join(filename)).map_err(|error| error.to_string())?;
            texts.push(filename.to_string_lossy().into_owned());
            provenance.push(source.clone());
        }
        if texts.is_empty() {
            missing.push(json!({"name": name, "version": version, "license": package["license"], "repository": package["repository"]}));
        }
        packages.push(json!({"name": name, "version": version, "source": package["source"], "license": package["license"], "repository": package["repository"], "license_directory": format!("licenses/{id}"), "license_texts": texts, "supplement_provenance": provenance}));
    }
    fs::write(destination.join("dependency-inventory.json"), serde_json::to_vec_pretty(&json!({"schema_version": 1, "filter_platform": target_platform, "scope": "Cargo resolved graph including build and platform dependencies", "packages": packages})).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    fs::copy(distribution.join("LICENSE"), destination.join("LICENSE"))
        .map_err(|error| error.to_string())?;
    fs::copy(
        distribution.join("vendor/gpui/UPSTREAM.json"),
        destination.join("GPUI-UPSTREAM.json"),
    )
    .map_err(|error| error.to_string())?;
    for (source, name) in [
        (root.join("assets/OFL.txt"), "FONT-OFL.txt"),
        (
            distribution.join("vendor/gpui/LICENSE-APACHE"),
            "GPUI-LICENSE-APACHE",
        ),
        (
            distribution.join("vendor/gpui/LICENSE-GPL"),
            "GPUI-LICENSE-GPL",
        ),
    ] {
        if source.is_file() {
            fs::copy(source, destination.join(name)).map_err(|error| error.to_string())?;
        }
    }
    fs::write(destination.join("THIRD-PARTY-NOTICES.txt"), "gpui-box release dependency inventory\n\nOriginal distribution contributions use Apache-2.0. Copied GPUI retains its upstream licensing and provenance in GPUI-UPSTREAM.json. Bundled Noto fonts use SIL OFL. See dependency-inventory.json and licenses/ for the resolved Cargo graph, declared licenses and available license/notice texts. Commit-pinned supplemental texts record their sources and hashes. Android packages additionally include runtime coordinates and Maven license declarations. This broad inventory includes build and non-target packages. Review redistribution obligations, any applicable Maven license texts and license-review.json before publication.\n").map_err(|error| error.to_string())?;
    let review = json!({"schema_version": 1, "cargo_packages": packages.len(), "cargo_missing_license_texts": missing, "publication_review_required": true, "scope": "Resolved Cargo graph and available texts. Maven POM license declarations and non-Cargo/font/native obligations require publication review."});
    fs::write(
        destination.join("license-review.json"),
        serde_json::to_vec_pretty(&review).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(review)
}

fn checksums(directory: &Path, prefix: &Path, result: &mut Vec<String>) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            checksums(&entry.path(), prefix, result)?;
        } else if kind.is_file() {
            let relative = entry
                .path()
                .strip_prefix(prefix)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            result.push(format!("{}  {relative}\n", digest(&entry.path())?));
        } else {
            return Err("Release inventory contains a non-regular entry".into());
        }
    }
    Ok(())
}

/// Runnable Windows development output. Installer/release policy belongs to M7.
pub fn windows_portable(root: &Path) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Portable developer packaging currently requires a Windows host".into());
    }
    let config = AppConfig::load(root)?;
    if !config.targets.iter().any(|target| target == "desktop") {
        return Err("Desktop target is disabled".into());
    }
    let distribution = config.distribution(root)?;
    for asset in &config.assets {
        let first = asset
            .components()
            .next()
            .ok_or("Empty asset path")?
            .as_os_str()
            .to_string_lossy()
            .to_ascii_lowercase();
        if [
            "dist",
            "target",
            ".gpui-box",
            "licenses",
            "box-desktop.exe",
            "bundle.json",
            "sha256sums",
            "license",
            "gpui-upstream.json",
            "dependency-inventory.json",
            "license-review.json",
            "third-party-notices.txt",
            "font-ofl.txt",
        ]
        .contains(&first.as_str())
            || first.starts_with("gpui-license-")
        {
            return Err("Portable asset path overlaps build output or bundle metadata".into());
        }
    }
    fs::create_dir_all(root.join(".gpui-box")).map_err(|error| error.to_string())?;
    let lock_path = root.join(".gpui-box/release.lock");
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|_| "Another packaging workflow owns .gpui-box/release.lock")?;
    let _guard = ReleaseLock(lock_path);
    drop(lock);
    crate::generate::generate(root, false)?;
    let name = format!(
        "{}-{}-{}-windows-dev",
        config.version,
        config.version_code,
        env::consts::ARCH
    );
    let parent = root.join("dist/desktop");
    let destination = parent.join(format!("{name}.zip"));
    let directory = parent.join(&name);
    if destination.exists() || directory.exists() {
        return Err(
            "Portable output already exists. Preserve it and increment version_code.".into(),
        );
    }
    workflow::run(Command::new("mise").current_dir(root).args([
        "exec",
        "--",
        "cargo",
        "build",
        "-p",
        "box-desktop",
    ]))?;
    let metadata: Value = serde_json::from_str(&output(
        Command::new("mise").current_dir(root).args([
            "exec",
            "--",
            "cargo",
            "metadata",
            "--locked",
            "--no-deps",
            "--format-version",
            "1",
        ]),
        "Application build metadata",
    )?)
    .map_err(|error| error.to_string())?;
    let executable = Path::new(
        metadata["target_directory"]
            .as_str()
            .ok_or("Missing Cargo target directory")?,
    )
    .join("debug/box-desktop.exe");
    fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
    let staging = parent.join(format!("{name}.staging-{}", std::process::id()));
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    fs::copy(&executable, staging.join("box-desktop.exe")).map_err(|error| error.to_string())?;
    for asset in &config.assets {
        copy_assets(&root.join(asset), &staging.join(asset))?;
    }
    let rust = output(
        Command::new("mise")
            .current_dir(root)
            .args(["exec", "--", "rustc", "-vV"]),
        "Host Rust target",
    )?;
    let host = rust
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("Missing Rust host target")?;
    let review = inventory(root, &distribution, &staging, host)?;
    let metadata = json!({"schema_version": 1, "kind": "windows_portable_developer", "profile": "debug", "host_target": host,
        "application_id": config.application_id, "version": config.version, "version_code": config.version_code,
        "executable": "box-desktop.exe", "executable_sha256": digest(&executable)?, "gpui_revision": gpui_box::UPSTREAM_REVISION,
        "license_review": review, "requirements": "Windows supported by the vendored GPUI backend, compatible GPU/driver and Microsoft Visual C++ runtime. Installer, signing and update qualification follow M7."});
    fs::write(
        staging.join("bundle.json"),
        serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut sums = Vec::new();
    checksums(&staging, &staging, &mut sums)?;
    fs::write(staging.join("SHA256SUMS"), sums.concat()).map_err(|error| error.to_string())?;
    let zip_staging = parent.join(format!("{name}.zip.staging-{}", std::process::id()));
    let archive = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&zip_staging)
        .map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(archive);
    zip_directory(&staging, &staging, &mut archive)?;
    archive
        .finish()
        .map_err(|error| error.to_string())?
        .sync_all()
        .map_err(|error| error.to_string())?;
    fs::rename(&staging, &directory).map_err(|error| error.to_string())?;
    fs::rename(&zip_staging, &destination).map_err(|error| error.to_string())?;
    fs::write(
        parent.join(format!("{name}.zip.sha256")),
        format!("{}  {name}.zip\n", digest(&destination)?),
    )
    .map_err(|error| error.to_string())?;
    crate::outputln!("Windows developer bundle {}", destination.display());
    Ok(())
}

fn copy_assets(source: &Path, destination: &Path) -> Result<(), String> {
    let kind = fs::symlink_metadata(source)
        .map_err(|error| error.to_string())?
        .file_type();
    if kind.is_dir() {
        fs::create_dir_all(destination).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            copy_assets(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if kind.is_file() {
        fs::create_dir_all(destination.parent().ok_or("Missing asset parent")?)
            .map_err(|error| error.to_string())?;
        fs::copy(source, destination).map_err(|error| error.to_string())?;
    } else {
        return Err("Portable assets must be regular files/directories, without symlinks".into());
    }
    Ok(())
}

fn zip_directory(
    directory: &Path,
    root: &Path,
    archive: &mut zip::ZipWriter<fs::File>,
) -> Result<(), String> {
    use std::io::Write;
    let mut entries = fs::read_dir(directory)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            zip_directory(&entry.path(), root, archive)?;
        } else {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            archive
                .start_file(
                    relative,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(|error| error.to_string())?;
            std::io::copy(
                &mut fs::File::open(entry.path()).map_err(|error| error.to_string())?,
                archive,
            )
            .map_err(|error| error.to_string())?;
        }
    }
    archive.flush().map_err(|error| error.to_string())
}

pub fn android(root: &Path) -> Result<(), String> {
    let config = AppConfig::load(root)?;
    if !config.targets.iter().any(|target| target == "android") {
        return Err("Android target is disabled".into());
    }
    signing_inputs(root)?;
    let expected_signer = configured_signer(root)?;
    let bundletool = bundletool()?;
    let sdk = workflow::sdk()?;
    let distribution = config.distribution(root)?;
    fs::create_dir_all(root.join(".gpui-box")).map_err(|error| error.to_string())?;
    let lock_path = root.join(".gpui-box/release.lock");
    let lock_file = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .map_err(|_| "Release workflow is already active. Check ownership before removing a stale .gpui-box/release.lock.")?;
    let _guard = ReleaseLock(lock_path);
    drop(lock_file);
    crate::generate::generate(root, false)?;
    let destination = root
        .join("dist/android")
        .join(format!("{}-{}", config.version, config.version_code));
    if destination.exists() {
        return Err("Release output already exists. Preserve it and increment version_code or explicitly move the old output.".into());
    }
    workflow::run(&mut gradle(
        root,
        &[
            ":app:assembleRelease",
            ":app:bundleRelease",
            ":app:releaseDependencyInventory",
            "-PgpuiBoxRelease=true",
        ],
    )?)?;
    let apk = root.join("android-host/app/build/outputs/apk/release/app-release.apk");
    let aab = root.join("android-host/app/build/outputs/bundle/release/app-release.aab");
    let signer = signer(root, &sdk, &apk)?;
    if signer != expected_signer {
        return Err("APK signer does not match the configured release identity".into());
    }
    let badging = output(
        Command::new(sdk.join("build-tools/36.0.0").join(if cfg!(windows) {
            "aapt.exe"
        } else {
            "aapt"
        }))
        .args(["dump", "badging"])
        .arg(&apk),
        "APK identity inspection",
    )?;
    for expected in [
        format!("name='{}'", config.application_id),
        format!("versionCode='{}'", config.version_code),
        format!("versionName='{}'", config.version),
        "sdkVersion:'35'".into(),
        "targetSdkVersion:'35'".into(),
    ] {
        if !badging.contains(&expected) {
            return Err(format!("APK identity/configuration mismatch: {expected}"));
        }
    }
    let native = native_libraries(&apk, "lib/")?;
    if native != native_libraries(&aab, "base/lib/")? {
        return Err("APK and AAB native library contents differ".into());
    }
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&apk).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let mut services = String::new();
    archive
        .by_name("assets/gpui-box-services.json")
        .map_err(|error| error.to_string())?
        .read_to_string(&mut services)
        .map_err(|error| error.to_string())?;
    if serde_json::from_str::<Value>(&services).map_err(|error| error.to_string())?
        != serde_json::to_value(&config.services).map_err(|error| error.to_string())?
    {
        return Err("Packaged service declarations differ from configuration".into());
    }
    output(
        Command::new(sdk.join("build-tools/36.0.0").join(if cfg!(windows) {
            "zipalign.exe"
        } else {
            "zipalign"
        }))
        .args(["-c", "-P", "16", "4"])
        .arg(&apk),
        "APK 16 KB ZIP alignment",
    )?;
    output(
        java(root)
            .arg("-jar")
            .arg(&bundletool)
            .arg("validate")
            .arg(format!("--bundle={}", aab.display())),
        "AAB validation",
    )?;
    let bundle_config: Value = serde_json::from_str(&output(
        java(root)
            .arg("-jar")
            .arg(&bundletool)
            .args(["dump", "config"])
            .arg(format!("--bundle={}", aab.display())),
        "AAB native alignment configuration",
    )?)
    .map_err(|error| error.to_string())?;
    if bundle_config["optimizations"]["uncompressNativeLibraries"]["alignment"]
        != "PAGE_ALIGNMENT_16K"
    {
        return Err("AAB does not request 16 KB native ZIP alignment".into());
    }
    let signature = output(
        Command::new("mise")
            .current_dir(root)
            .args([
                "-E",
                "android",
                "exec",
                "--",
                "jarsigner",
                "-J-Duser.language=en",
                "-verify",
            ])
            .arg(&aab),
        "AAB JAR signature verification",
    )?;
    if !signature.contains("jar verified") {
        return Err("AAB signature verification did not confirm a signed archive".into());
    }
    let bundle_certificate = output(
        Command::new("mise")
            .current_dir(root)
            .args([
                "-E",
                "android",
                "exec",
                "--",
                "keytool",
                "-J-Duser.language=en",
                "-printcert",
                "-jarfile",
            ])
            .arg(&aab),
        "AAB signer inspection",
    )?;
    let bundle_hash = bundle_certificate
        .lines()
        .find_map(|line| line.trim().strip_prefix("SHA256: "))
        .ok_or("AAB signer digest is missing")?
        .replace(':', "")
        .to_ascii_lowercase();
    if bundle_hash != signer {
        return Err("AAB signer does not match the inspected APK".into());
    }
    let bundle_manifest = output(
        java(root)
            .arg("-jar")
            .arg(&bundletool)
            .args(["dump", "manifest", "--module=base"])
            .arg(format!("--bundle={}", aab.display())),
        "AAB manifest inspection",
    )?;
    inspect_manifest(&bundle_manifest, &config)?;
    let staging = destination.with_file_name(format!(
        "{}.staging-{}",
        destination
            .file_name()
            .ok_or("Missing release name")?
            .to_string_lossy(),
        std::process::id()
    ));
    fs::create_dir_all(staging.parent().ok_or("Missing release parent")?)
        .map_err(|error| error.to_string())?;
    fs::create_dir(&staging).map_err(|error| error.to_string())?;
    let mut artifacts = Vec::new();
    fs::write(staging.join("AndroidManifest.xml"), &bundle_manifest)
        .map_err(|error| error.to_string())?;
    fs::write(staging.join("gpui-box-services.json"), &services)
        .map_err(|error| error.to_string())?;
    for (source, name, kind) in [(&apk, "app.apk", "apk"), (&aab, "app.aab", "aab")] {
        let target = staging.join(name);
        fs::copy(source, &target).map_err(|error| error.to_string())?;
        artifacts.push(json!({"path": name, "kind": kind, "sha256": digest(&target)?, "size": fs::metadata(&target).map_err(|error| error.to_string())?.len()}));
    }
    let license_review = inventory(root, &distribution, &staging, "aarch64-linux-android")?;
    let android_inventory = root.join("android-host/app/build/reports/gpui-box");
    fs::copy(
        android_inventory.join("android-dependencies.json"),
        staging.join("android-dependencies.json"),
    )
    .map_err(|error| error.to_string())?;
    let poms = staging.join("maven-poms");
    fs::create_dir(&poms).map_err(|error| error.to_string())?;
    let dependencies: Value = serde_json::from_slice(
        &fs::read(staging.join("android-dependencies.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    for dependency in dependencies
        .as_array()
        .ok_or("Invalid Maven dependency inventory")?
    {
        let filename = format!(
            "{}-{}-{}.pom",
            dependency["group"].as_str().ok_or("Missing Maven group")?,
            dependency["name"].as_str().ok_or("Missing Maven name")?,
            dependency["version"]
                .as_str()
                .ok_or("Missing Maven version")?
        );
        if Path::new(&filename).components().count() != 1 {
            return Err("Invalid Maven dependency coordinates".into());
        }
        let source = android_inventory.join("maven-poms").join(&filename);
        if source.is_file() {
            fs::copy(source, poms.join(filename)).map_err(|error| error.to_string())?;
        } else if dependency["version"] != "unspecified" {
            return Err("Resolved Maven dependency is missing its POM".into());
        }
    }
    let metadata = json!({"schema_version": 1, "application_id": config.application_id, "version": config.version, "version_code": config.version_code, "update_channel": config.update_channel, "profile": "release", "minimum_api": 35, "target_api": 35, "compile_api": 36, "signer_sha256": signer, "native_libraries": native, "artifacts": artifacts, "gpui_revision": gpui_box::UPSTREAM_REVISION, "bundletool_version": "1.18.3", "license_review": license_review, "qualification": "Build/signature/alignment validation. Publication/license review, distribution-channel and physical-device acceptance remain separate."});
    fs::write(
        staging.join("release.json"),
        serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut sums = Vec::new();
    checksums(&staging, &staging, &mut sums)?;
    fs::write(staging.join("SHA256SUMS"), sums.concat()).map_err(|error| error.to_string())?;
    fs::rename(&staging, &destination)
        .map_err(|error| format!("Publish release artifacts: {error}"))?;
    crate::outputln!("Inspected release artifacts {}", destination.display());
    Ok(())
}

struct ReleaseLock(PathBuf);
impl Drop for ReleaseLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_manifest_checks_namespaced_flags_identity_and_exported_components() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("app");
        let distribution = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        crate::generate::create(&root, distribution).unwrap();
        let config = AppConfig::load(&root).unwrap();
        let manifest = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="dev.example.gpuibox" android:versionCode="1" android:versionName="0.1.0">
        <uses-sdk android:minSdkVersion="35" android:targetSdkVersion="35"/>
        <application android:allowBackup="false" android:extractNativeLibs="false">
        <activity android:name="dev.gpui.box.MainActivity" android:exported="true"><intent-filter><category android:name="android.intent.category.LAUNCHER"/></intent-filter></activity>
        </application></manifest>"#;
        assert!(inspect_manifest(manifest, &config).is_ok());
        for invalid in [
            manifest.replace("versionCode=\"1\"", "versionCode=\"2\""),
            manifest.replace("minSdkVersion=\"35\"", "minSdkVersion=\"26\""),
            manifest.replace("<application ", "<application android:debuggable=\"true\" "),
            manifest.replace(
                "</application>",
                "<provider android:exported=\"true\"/></application>",
            ),
            manifest.replace("extractNativeLibs=\"false\"", "extractNativeLibs=\"true\""),
        ] {
            assert!(inspect_manifest(&invalid, &config).is_err());
        }
    }

    #[test]
    fn wrapper_rejects_modified_jar_and_shadowed_distribution_properties() {
        let temporary = tempfile::tempdir().unwrap();
        let host = temporary.path();
        fs::create_dir_all(host.join("gradle/wrapper")).unwrap();
        fs::write(
            host.join("gradle/wrapper/gradle-wrapper.jar"),
            include_bytes!("../../../platforms/android/gradle/wrapper/gradle-wrapper.jar"),
        )
        .unwrap();
        let properties =
            include_str!("../../../platforms/android/gradle/wrapper/gradle-wrapper.properties");
        fs::write(
            host.join("gradle/wrapper/gradle-wrapper.properties"),
            properties,
        )
        .unwrap();
        assert!(verify_wrapper(host).is_ok());
        fs::write(
            host.join("gradle/wrapper/gradle-wrapper.properties"),
            format!("{properties}\ndistributionUrl=https://example.invalid/untrusted.zip\n"),
        )
        .unwrap();
        assert!(verify_wrapper(host).is_err());
        for shadow in [
            "distributionUrl : https://example.invalid",
            "distribution\\u0055rl=https://example.invalid",
            "distributionUrl https://example.invalid",
        ] {
            fs::write(
                host.join("gradle/wrapper/gradle-wrapper.properties"),
                format!("{properties}\n{shadow}\n"),
            )
            .unwrap();
            assert!(verify_wrapper(host).is_err());
        }
        fs::write(host.join("gradle/wrapper/gradle-wrapper.jar"), b"modified").unwrap();
        assert!(verify_wrapper(host).unwrap_err().contains("checksum"));
    }

    #[test]
    fn native_alignment_rejects_wrong_abi_and_small_pages() {
        let mut elf = vec![0; 128];
        elf[..6].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1]);
        elf[18] = 183;
        elf[32] = 64;
        elf[54] = 56;
        elf[56] = 1;
        elf[64] = 1;
        elf[112..120].copy_from_slice(&16384u64.to_le_bytes());
        assert!(inspect_elf(&elf, 183).is_ok());
        assert!(inspect_elf(&elf, 62).is_err());
        elf[112..120].copy_from_slice(&4096u64.to_le_bytes());
        assert!(inspect_elf(&elf, 183).unwrap_err().contains("16 KB"));
        assert!(inspect_elf(&elf[..20], 183).is_err());
    }

    #[test]
    fn archive_and_checksums_cover_nested_assets_and_license_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("bundle");
        fs::create_dir_all(root.join("licenses/example")).unwrap();
        fs::create_dir(root.join("assets")).unwrap();
        fs::write(root.join("licenses/example/LICENSE"), b"license").unwrap();
        fs::write(root.join("assets/text.txt"), b"asset").unwrap();
        let mut sums = Vec::new();
        checksums(&root, &root, &mut sums).unwrap();
        assert_eq!(sums.len(), 2);
        assert!(sums[0].ends_with("  assets/text.txt\n"));
        assert!(sums[1].ends_with("  licenses/example/LICENSE\n"));
        let archive_path = temporary.path().join("bundle.zip");
        let mut archive = zip::ZipWriter::new(fs::File::create(&archive_path).unwrap());
        zip_directory(&root, &root, &mut archive).unwrap();
        archive.finish().unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(archive_path).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        let mut text = String::new();
        archive
            .by_name("licenses/example/LICENSE")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "license");
    }
}
