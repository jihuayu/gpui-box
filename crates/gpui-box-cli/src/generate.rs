use crate::config::AppConfig;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn hash(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn write(path: &Path, value: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing parent directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    static NEXT_WRITE: AtomicU64 = AtomicU64::new(1);
    let name = path
        .file_name()
        .ok_or("Missing file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(
        ".{name}.gpui-box-{}-{}",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Create {}: {e}", temporary.display()))?;
    let result = (|| {
        file.write_all(value)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!("Write {}: {error}", path.display()));
    }
    Ok(())
}
fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("serialize string")
}
fn kotlin_quoted(value: &str) -> String {
    quoted(value).replace('$', "\\$")
}
fn portable_path(value: &str) -> String {
    if let Some(path) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("//{}", path.replace('\\', "/"))
    } else {
        value.trim_start_matches("\\\\?\\").replace('\\', "/")
    }
}

pub fn create(root: &Path, distribution: PathBuf) -> Result<(), String> {
    if root.exists() {
        return Err(format!(
            "Destination {} already exists. Choose a new directory.",
            root.display()
        ));
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Destination must have a UTF-8 directory name")?
        .to_string();
    let config = AppConfig {
        schema_version: 1,
        name,
        application_id: "dev.example.gpuibox".into(),
        version: "0.1.0".into(),
        version_code: 1,
        distribution,
        targets: vec!["desktop".into(), "android".into()],
        update_channel: "development".into(),
        permissions: vec![],
        assets: vec![PathBuf::from("assets")],
        services: crate::config::Services {
            schema_version: 1,
            ..Default::default()
        },
    };
    config.validate()?;
    // Build a sibling directory, then publish the complete project in one rename.
    let staging = root.with_file_name(format!("{}.gpui-box-{}", config.name, std::process::id()));
    if staging.exists() {
        return Err(format!(
            "Staging directory {} already exists",
            staging.display()
        ));
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        write(
            &staging.join("gpui-box.json"),
            &serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
        )?;
        generate(&staging, true)?;
        fs::create_dir_all(staging.join("assets")).map_err(|e| e.to_string())?;
        for asset in ["NotoSansSC-Regular.otf", "OFL.txt"] {
            fs::copy(
                config
                    .distribution
                    .join("examples/hello/assets")
                    .join(asset),
                staging.join("assets").join(asset),
            )
            .map_err(|e| format!("Copy font asset: {e}"))?;
        }
        fs::copy(config.distribution.join("LICENSE"), staging.join("LICENSE"))
            .map_err(|e| e.to_string())?;
        fs::copy(
            config.distribution.join("Cargo.lock"),
            staging.join("Cargo.lock"),
        )
        .map_err(|e| e.to_string())?;
        fs::rename(&staging, root).map_err(|e| format!("Publish project: {e}"))
    })();
    // Keep a failed staging directory for inspection. Never recursively delete caller paths.
    result
}

struct GenerationLock(PathBuf);
impl Drop for GenerationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
pub fn generate(root: &Path, initial: bool) -> Result<(), String> {
    fs::create_dir_all(root.join(".gpui-box")).map_err(|e| e.to_string())?;
    let lock_path = root.join(".gpui-box/generate.lock");
    let lock = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .map_err(|e| format!("Cannot acquire generation lock {}: {e}. Check for another generator before removing a stale lock.", lock_path.display()))?;
    let _guard = GenerationLock(lock_path);
    drop(lock);
    let config = AppConfig::load(root)?;
    let distribution = config.distribution(root)?;
    let distribution = portable_path(
        distribution
            .to_str()
            .ok_or("Distribution path must be UTF-8")?,
    );
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let mut add = |path: &str, value: String| {
        files.insert(path.into(), value.into_bytes());
    };
    add(
        "Cargo.toml",
        format!(
            r#"[workspace]
resolver = "3"
members = ["app", "desktop", "android"]
[workspace.dependencies]
gpui-box = {{ path = {facade}, default-features = false }}
gpui = {{ path = {gpui}, default-features = false }}
gpui-box-components = {{ path = {components} }}
[patch.crates-io]
async-task = {{ git = "https://github.com/zed-industries/async-task.git", rev = "015845a19fc31752c02367567d3b8900a406f9da" }}
[profile.dev]
debug = "limited"
"#,
            facade = quoted(&format!("{distribution}/crates/gpui-box")),
            gpui = quoted(&format!("{distribution}/vendor/gpui/crates/gpui")),
            components = quoted(&format!("{distribution}/components/gpui-box-components"))
        ),
    );
    add("app/Cargo.toml", "[package]\nname = \"box-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\n[dependencies]\ngpui.workspace = true\ngpui-box-components.workspace = true\n[target.'cfg(target_os = \"android\")'.dependencies]\ngpui-box = { workspace = true, features = [\"android\"] }\n".into());
    add("desktop/Cargo.toml", "[package]\nname = \"box-desktop\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\n[dependencies]\ngpui-box = { workspace = true, features = [\"desktop\"] }\nbox-app = { path = \"../app\" }\n".into());
    add("android/Cargo.toml", "[package]\nname = \"box-android\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\n[lib]\nname = \"gpui_box_android\"\ncrate-type = [\"cdylib\"]\n[target.'cfg(target_os = \"android\")'.dependencies]\ngpui-box = { workspace = true, features = [\"android\"] }\nbox-app = { path = \"../app\" }\n".into());
    add(
        "android-host/settings.gradle.kts",
        r#"pluginManagement { repositories { google(); mavenCentral(); gradlePluginPortal() } }
dependencyResolutionManagement { repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS); repositories { google(); mavenCentral() } }
rootProject.name = "gpui-box-app"
include(":app")
"#.to_string(),
    );
    add("android-host/build.gradle.kts", "plugins { id(\"com.android.application\") version \"8.10.1\" apply false; id(\"org.jetbrains.kotlin.android\") version \"2.1.20\" apply false }\n".into());
    let mut host =
        fs::read_to_string(Path::new(&distribution).join("platforms/android/app/build.gradle.kts"))
            .map_err(|e| e.to_string())?;
    host = host
        .replace(
            "rootProject.projectDir.resolve(\"../..\")",
            "rootProject.projectDir.resolve(\"..\")",
        )
        .replace("dev.gpui.box.example", &config.application_id)
        .replace(
            "versionCode = 1",
            &format!("versionCode = {}", config.version_code),
        )
        .replace(
            "versionName = \"0.1.0-dev\"",
            &format!("versionName = {}", quoted(&config.version)),
        )
        .replace("gpui-box-example-android", "box-android");
    // A fresh application adds its own root packages to the seeded lockfile.
    host = host.replace("\"build\", \"--locked\", \"-p\"", "\"build\", \"-p\"");
    host = host.replace("    inputs.files(fileTree(repository.resolve(\"crates\"))", "    inputs.files(fileTree(repository.resolve(\"app\")) { include(\"**/*.rs\", \"**/Cargo.toml\") })\n    inputs.files(fileTree(repository.resolve(\"crates\"))");
    host += "\nbuildRust.configure { inputs.files(fileTree(rootProject.file(\"../android\")) { include(\"**/*.rs\", \"**/Cargo.toml\") }); inputs.files(fileTree(rootProject.file(\"../assets\"))) }\n";
    host += &format!(
        "\nbuildRust.configure {{ inputs.files(fileTree({}) {{ include(\"**/*.rs\", \"**/Cargo.toml\", \"**/*.wgsl\", \"**/*.hlsl\"); exclude(\"target/**\", \".git/**\", \"**/build/**\") }}) }}\n",
        kotlin_quoted(&distribution)
    );
    host += &format!(
        "\nandroid.sourceSets[\"main\"].java.srcDir({})\n",
        kotlin_quoted(&format!(
            "{distribution}/platforms/android/app/src/main/java"
        ))
    );
    host += "\napply(from = rootProject.file(\"../android-custom.gradle.kts\"))\n";
    for asset in &config.assets {
        let path = asset
            .to_str()
            .ok_or("Asset paths must be UTF-8")?
            .replace('\\', "/");
        host += &format!(
            "\nandroid.sourceSets[\"main\"].assets.srcDir(rootProject.file({}))\nbuildRust.configure {{ inputs.files(fileTree(rootProject.file({}))) }}\n",
            kotlin_quoted(&format!("../{path}")),
            kotlin_quoted(&format!("../{path}"))
        );
    }
    add("android-host/app/build.gradle.kts", host);
    let mut permission = if config
        .permissions
        .iter()
        .any(|permission| permission == "internet")
    {
        "<uses-permission android:name=\"android.permission.INTERNET\" />".to_string()
    } else {
        String::new()
    };
    for declaration in &config.services.permissions {
        for name in declaration.kind.android_permissions() {
            permission += &format!("\n<uses-permission android:name=\"{name}\" />");
        }
    }
    // Runtime permission intent must not silently exclude devices from stores.
    // Applications can require hardware explicitly through native customization.
    for (kind, feature) in [
        (
            crate::config::PermissionKind::Camera,
            "android.hardware.camera",
        ),
        (
            crate::config::PermissionKind::Microphone,
            "android.hardware.microphone",
        ),
        (
            crate::config::PermissionKind::LocationWhenInUse,
            "android.hardware.location",
        ),
    ] {
        if config
            .services
            .permissions
            .iter()
            .any(|declaration| declaration.kind == kind)
        {
            permission += &format!(
                "\n<uses-feature android:name=\"{feature}\" android:required=\"false\" />"
            );
        }
    }
    let mut registrations = String::new();
    for scheme in &config.services.url_schemes {
        registrations += &format!(
            "<intent-filter><action android:name=\"android.intent.action.VIEW\"/><category android:name=\"android.intent.category.DEFAULT\"/><category android:name=\"android.intent.category.BROWSABLE\"/><data android:scheme=\"{scheme}\"/></intent-filter>\n"
        );
    }
    add(
        "android-host/app/src/main/assets/gpui-box-services.json",
        serde_json::to_string_pretty(&config.services).map_err(|error| error.to_string())?,
    );
    let name = config
        .name
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;");
    add(
        "android-host/app/src/main/res/values/strings.xml",
        format!("<resources><string name=\"app_name\">\"{name}\"</string></resources>\n"),
    );
    add(
        "android-host/app/src/main/AndroidManifest.xml",
        format!(
            r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android">
{permission}<uses-feature android:name="android.hardware.vulkan.level" android:version="1" android:required="true" />
<application android:label="@string/app_name" android:theme="@android:style/Theme.Material.NoActionBar" android:allowBackup="false" android:enableOnBackInvokedCallback="true">
<activity android:name="dev.gpui.box.MainActivity" android:exported="true" android:launchMode="singleTop" android:windowSoftInputMode="adjustResize"><intent-filter><action android:name="android.intent.action.MAIN"/><category android:name="android.intent.category.LAUNCHER"/></intent-filter>{registrations}</activity>
</application></manifest>
"#
        ),
    );
    add("android-host/gradle.properties", "org.gradle.jvmargs=-Xmx3g -Dfile.encoding=UTF-8\norg.gradle.parallel=false\nandroid.useAndroidX=true\n".into());
    for path in ["app/Cargo.toml", "desktop/Cargo.toml", "android/Cargo.toml"] {
        let manifest = files.get_mut(path).expect("generated package");
        *manifest = String::from_utf8(std::mem::take(manifest))
            .expect("UTF-8 manifest")
            .replace(
                "version = \"0.1.0\"",
                &format!("version = {}", quoted(&config.version)),
            )
            .into_bytes();
    }
    let state_path = root.join(".gpui-box/generated.json");
    let journal_path = root.join(".gpui-box/pending.json");
    // A journal permits safe retries after interruption between individual renames.
    let pending: BTreeMap<String, Vec<String>> = if journal_path.exists() {
        serde_json::from_slice(&fs::read(&journal_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid generation journal: {e}"))?
    } else {
        BTreeMap::new()
    };
    let previous: BTreeMap<String, String> = if state_path.exists() {
        serde_json::from_slice(&fs::read(&state_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid generator state: {e}"))?
    } else {
        BTreeMap::new()
    };
    // Preflight every managed path before writing any file.
    let mut journal: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, content) in &files {
        let hashes = journal.entry(path.clone()).or_default();
        hashes.push(hash(content));
        let target = root.join(path);
        if target.exists() {
            let current = hash(&fs::read(&target).map_err(|e| e.to_string())?);
            if previous.get(path) != Some(&current)
                && !pending
                    .get(path)
                    .is_some_and(|hashes| hashes.contains(&current))
            {
                return Err(format!(
                    "Managed file {path} was edited. Move customizations to app/src or android-custom.gradle.kts before generating."
                ));
            }
            hashes.push(current);
        }
    }
    let state: BTreeMap<_, _> = files
        .iter()
        .map(|(path, content)| (path.clone(), hash(content)))
        .collect();
    write(
        &journal_path,
        &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
    )?;
    for (path, content) in files {
        write(&root.join(&path), &content)?;
    }
    write(
        &state_path,
        &serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?,
    )?;
    fs::remove_file(journal_path).map_err(|e| format!("Complete generation journal: {e}"))?;
    if initial {
        write(
            &root.join("app/src/lib.rs"),
            include_bytes!("../../../templates/app.rs"),
        )?;
        write(
            &root.join("desktop/src/main.rs"),
            b"fn main() { gpui_box::application().run(box_app::open); }\n",
        )?;
        write(&root.join("android/src/lib.rs"), b"#![cfg(target_os = \"android\")]\ngpui_box::android::android_entrypoint!(box_app::open);\n")?;
        write(
            &root.join("android-custom.gradle.kts"),
            b"// Application-owned Android customization point. Preserved during generation.\n",
        )?;
        write(
            &root.join("mise.toml"),
            include_bytes!("../../../templates/mise.toml"),
        )?;
        write(
            &root.join("mise.android.toml"),
            include_bytes!("../../../templates/mise.android.toml"),
        )?;
        write(
            &root.join(".gitignore"),
            b"/target/\n/android-host/.gradle/\n/android-host/app/build/\nlocal.properties\n",
        )?;
        write(
            &root.join("README.md"),
            include_bytes!("../../../templates/README.md"),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::portable_path;

    #[test]
    fn canonical_windows_paths_remain_usable_by_gradle() {
        assert_eq!(
            portable_path(r"\\?\D:\Project files\gpui-box"),
            "D:/Project files/gpui-box"
        );
        assert_eq!(
            portable_path(r"\\?\UNC\server\share\gpui-box"),
            "//server/share/gpui-box"
        );
        assert_eq!(portable_path("/home/user/gpui-box"), "/home/user/gpui-box");
    }
}
