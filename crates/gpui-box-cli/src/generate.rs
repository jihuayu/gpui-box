use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Copy)]
pub enum Template {
    Starter,
    Adaptive,
}
impl Template {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "starter" => Ok(Self::Starter),
            "adaptive" => Ok(Self::Adaptive),
            _ => Err("--template supports starter and adaptive".into()),
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Starter => "starter",
            Self::Adaptive => "adaptive",
        }
    }
}

pub fn create(root: &Path, distribution: PathBuf) -> Result<(), String> {
    create_with_targets(
        root,
        distribution,
        vec!["desktop".into(), "android".into()],
        Template::Starter,
    )
}

pub fn create_with_targets(
    root: &Path,
    distribution: PathBuf,
    targets: Vec<String>,
    template: Template,
) -> Result<(), String> {
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
        targets,
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
        generate_inner(&staging, Some(template), None)?;
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

pub fn initialize(
    root: &Path,
    distribution: PathBuf,
    targets: Vec<String>,
    template: Template,
) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Open init directory: {error}"))?;
    if !root.is_dir() {
        return Err("init requires an existing directory".into());
    }
    if root.join("gpui-box.json").exists() {
        return Err("Project is already initialized. Use generate to update managed files.".into());
    }
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Init directory must have a UTF-8 name")?;
    let staging = root.with_file_name(format!("{name}.gpui-box-init-{}", std::process::id()));
    create_with_targets(&staging, distribution, targets, template)?;
    // Use the caller's directory name rather than the temporary staging name.
    let mut config = AppConfig::load(&staging)?;
    config.name = name.into();
    write(
        &staging.join("gpui-box.json"),
        &serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )?;
    generate(&staging, false)?;

    let mut files = Vec::new();
    let mut directories = vec![PathBuf::new()];
    let mut staged_directories = Vec::new();
    while let Some(relative) = directories.pop() {
        for entry in fs::read_dir(staging.join(&relative)).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = relative.join(entry.file_name());
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                directories.push(path);
            } else {
                files.push(path);
            }
        }
        staged_directories.push(relative);
    }
    files.sort();
    // Configuration is the completion marker, published after the complete tree.
    files.sort_by_key(|path| path == Path::new("gpui-box.json"));
    let lock_path = managed_path(&root, ".gpui-box-init.lock")?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .map_err(|error| format!("Acquire init lock: {error}"))?;
    let _guard = GenerationLock(lock_path);
    drop(lock);
    let mut publish = Vec::new();
    for relative in &files {
        let text = relative.to_str().ok_or("Init output must be UTF-8")?;
        let destination = managed_path(&root, text)?;
        if destination.exists() {
            if matches!(text, "README.md" | "LICENSE" | ".gitignore") && destination.is_file() {
                continue;
            }
            return Err(format!(
                "Init conflicts with {}. Original preserved. Complete staged project retained at {}",
                destination.display(),
                staging.display()
            ));
        }
        publish.push((relative, destination));
    }
    for (relative, destination) in &publish {
        fs::create_dir_all(destination.parent().ok_or("Missing init parent")?)
            .map_err(|error| error.to_string())?;
        // A hard link publishes each file with exclusive-create semantics, including
        // on Unix where rename would overwrite a concurrently created file.
        fs::hard_link(staging.join(relative), destination).map_err(|error| format!("Init publication stopped at {}: {error}. Existing files preserved. Complete staged project retained at {}", destination.display(), staging.display()))?;
    }
    // Remove only the exact generated entries. Unexpected additions keep their
    // directory intact, and failed initialization retains the full staging tree.
    for relative in files {
        fs::remove_file(staging.join(relative))
            .map_err(|error| format!("Project initialized; staging cleanup failed: {error}"))?;
    }
    for relative in staged_directories.into_iter().rev() {
        fs::remove_dir(staging.join(relative))
            .map_err(|error| format!("Project initialized; staging cleanup failed: {error}"))?;
    }
    crate::outputln!("Initialized {}", root.display());
    Ok(())
}

struct GenerationLock(PathBuf);
impl Drop for GenerationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAction {
    Create,
    Update,
    Unchanged,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    pub path: String,
    pub action: ChangeAction,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePlan {
    pub schema_version: u32,
    pub cli_version: String,
    pub upstream_revision: String,
    pub config_sha256: String,
    pub recovering_pending_journal: bool,
    pub pending_journal_sha256: Option<String>,
    pub files: Vec<FileChange>,
    pub metadata: FileChange,
}

#[derive(Debug, Serialize)]
pub struct ApplyReceipt {
    pub schema_version: u32,
    pub completed: bool,
    pub plan_sha256: String,
    pub files_written: Vec<String>,
    pub files_unchanged: Vec<String>,
    pub recovered_pending_journal: bool,
}

/// Accept only the bare result ChangePlan, bounded independently of file size.
/// Saved plans supply preconditions; generated bytes always come from this CLI.
pub fn apply(root: &Path, plan_path: Option<&Path>) -> Result<ApplyReceipt, String> {
    use std::io::Read;
    let expected = match plan_path {
        Some(path) => {
            let mut bytes = Vec::new();
            fs::File::open(path)
                .map_err(|error| format!("Open generation plan: {error}"))?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("Read generation plan: {error}"))?;
            if bytes.len() > 1024 * 1024 {
                return Err("Generation plan exceeds its 1 MiB bound".into());
            }
            Some(
                serde_json::from_slice::<ChangePlan>(&bytes)
                    .map_err(|error| format!("Invalid generation plan: {error}"))?,
            )
        }
        None => None,
    };
    generate_inner(root, None, expected.as_ref())
}

struct PreparedGeneration {
    files: BTreeMap<String, Vec<u8>>,
    state: Vec<u8>,
    journal: Vec<u8>,
    plan: ChangePlan,
}

/// A read-only snapshot. Apply always recomputes preconditions under its lock.
pub fn plan(root: &Path) -> Result<ChangePlan, String> {
    let lock = managed_path(root, ".gpui-box/generate.lock")?;
    if lock.exists() {
        return Err("Another generator owns this project or left a stale lock".into());
    }
    let prepared = prepare(root)?;
    if lock.exists() {
        return Err("Another generator started during the snapshot; retry the dry run".into());
    }
    Ok(prepared.plan)
}

// All managed paths are known relative names. Reject existing links at every
// component before both planning and writing. Private project ownership remains
// required while running the generator; this is not a hostile-directory sandbox.
fn managed_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    let relative = Path::new(relative);
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Managed output requires a normal relative path".into());
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if redirected(&metadata) => {
                return Err(
                    "Managed output and metadata paths must not traverse links or reparse points"
                        .into(),
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Inspect managed path: {error}")),
        }
    }
    Ok(current)
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn file_change(root: &Path, path: &str, content: &[u8]) -> Result<FileChange, String> {
    let target = managed_path(root, path)?;
    let before = match fs::read(target) {
        Ok(content) => Some(hash(&content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Read planned file: {error}")),
    };
    let after = hash(content);
    let action = match &before {
        None => ChangeAction::Create,
        Some(before) if *before == after => ChangeAction::Unchanged,
        Some(_) => ChangeAction::Update,
    };
    Ok(FileChange {
        path: path.into(),
        action,
        before_sha256: before,
        after_sha256: after,
    })
}

fn prepare(root: &Path) -> Result<PreparedGeneration, String> {
    let config_bytes = fs::read(managed_path(root, "gpui-box.json")?)
        .map_err(|error| format!("Read gpui-box.json: {error}"))?;
    let config: AppConfig = serde_json::from_slice(&config_bytes)
        .map_err(|error| format!("Invalid gpui-box.json: {error}"))?;
    config.validate()?;
    let config_sha256 = hash(&config_bytes);
    let distribution = config.distribution(root)?;
    let distribution = portable_path(
        distribution
            .to_str()
            .ok_or("Distribution path must be UTF-8")?,
    );
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let desktop = config.targets.iter().any(|target| target == "desktop");
    let android = config.targets.iter().any(|target| target == "android");
    let mut members = vec!["app"];
    if desktop {
        members.push("desktop");
    }
    if android {
        members.push("android");
    }
    let mut add = |path: &str, value: String| {
        files.insert(path.into(), value.into_bytes());
    };
    add(
        "Cargo.toml",
        format!(
            r#"[workspace]
resolver = "3"
members = {members}
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
            components = quoted(&format!("{distribution}/components/gpui-box-components")),
            members = serde_json::to_string(&members).expect("serialize members")
        ),
    );
    let mut app = "[package]\nname = \"box-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\n[dependencies]\ngpui.workspace = true\ngpui-box-components.workspace = true\n".to_string();
    if android {
        app += "[target.'cfg(target_os = \"android\")'.dependencies]\ngpui-box = { workspace = true, features = [\"android\"] }\n";
    }
    add("app/Cargo.toml", app);
    if desktop {
        add("desktop/Cargo.toml", "[package]\nname = \"box-desktop\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\n[dependencies]\ngpui-box = { workspace = true, features = [\"desktop\"] }\nbox-app = { path = \"../app\" }\n".into());
    }
    if android {
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
        let mut host = fs::read_to_string(
            Path::new(&distribution).join("platforms/android/app/build.gradle.kts"),
        )
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
        host += "\nlistOf(buildRust, buildRustRelease).forEach { it.configure { inputs.files(fileTree(rootProject.file(\"../android\")) { include(\"**/*.rs\", \"**/Cargo.toml\") }); inputs.files(fileTree(rootProject.file(\"../assets\"))) } }\n";
        host += &format!(
            "\nlistOf(buildRust, buildRustRelease).forEach {{ it.configure {{ inputs.files(fileTree({}) {{ include(\"**/*.rs\", \"**/Cargo.toml\", \"**/*.wgsl\", \"**/*.hlsl\"); exclude(\"target/**\", \".git/**\", \"**/build/**\") }}) }} }}\n",
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
                "\nandroid.sourceSets[\"main\"].assets.srcDir(rootProject.file({}))\nlistOf(buildRust, buildRustRelease).forEach {{ it.configure {{ inputs.files(fileTree(rootProject.file({}))) }} }}\n",
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
                crate::config::PermissionKind::Camera,
                "android.hardware.camera.autofocus",
            ),
            (
                crate::config::PermissionKind::Microphone,
                "android.hardware.microphone",
            ),
            (
                crate::config::PermissionKind::LocationWhenInUse,
                "android.hardware.location",
            ),
            (
                crate::config::PermissionKind::LocationWhenInUse,
                "android.hardware.location.gps",
            ),
            (
                crate::config::PermissionKind::LocationWhenInUse,
                "android.hardware.location.network",
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
        add("android-host/gradle.properties", "org.gradle.jvmargs=-Xmx3g -Dfile.encoding=UTF-8\norg.gradle.parallel=false\nandroid.useAndroidX=true\n# Rust/Java builds are qualified with Unicode project paths on Windows.\nandroid.overridePathCheck=true\n".into());
        for path in [
            "gradlew",
            "gradlew.bat",
            "gradle/wrapper/gradle-wrapper.jar",
            "gradle/wrapper/gradle-wrapper.properties",
        ] {
            files.insert(
                format!("android-host/{path}"),
                fs::read(
                    Path::new(&distribution)
                        .join("platforms/android")
                        .join(path),
                )
                .map_err(|error| error.to_string())?,
            );
        }
    }
    for path in members.iter().map(|member| format!("{member}/Cargo.toml")) {
        let manifest = files.get_mut(&path).expect("generated package");
        *manifest = String::from_utf8(std::mem::take(manifest))
            .expect("UTF-8 manifest")
            .replace(
                "version = \"0.1.0\"",
                &format!("version = {}", quoted(&config.version)),
            )
            .into_bytes();
    }
    let state_path = managed_path(root, ".gpui-box/generated.json")?;
    let journal_path = managed_path(root, ".gpui-box/pending.json")?;
    // A journal permits safe retries after interruption between individual renames.
    let pending_bytes = match fs::read(&journal_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Read generation journal: {error}")),
    };
    let pending: BTreeMap<String, Vec<String>> = match &pending_bytes {
        Some(bytes) => {
            serde_json::from_slice(bytes).map_err(|e| format!("Invalid generation journal: {e}"))?
        }
        None => BTreeMap::new(),
    };
    let previous: BTreeMap<String, String> = if state_path.exists() {
        serde_json::from_slice(&fs::read(&state_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid generator state: {e}"))?
    } else {
        BTreeMap::new()
    };
    // Preflight every managed path before writing any file.
    let mut journal: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut changes = Vec::new();
    for (path, content) in &files {
        let hashes = journal.entry(path.clone()).or_default();
        hashes.push(hash(content));
        let change = file_change(root, path, content)?;
        if let Some(current) = &change.before_sha256 {
            if previous.get(path) != Some(current)
                && !pending
                    .get(path)
                    .is_some_and(|hashes| hashes.contains(current))
            {
                return Err(format!(
                    "Managed file {path} was edited. Move customizations to app/src or android-custom.gradle.kts before generating."
                ));
            }
            hashes.push(current.clone());
        }
        changes.push(change);
    }
    // Disabled targets retain their files and ownership hashes so re-enabling
    // them can distinguish the previous generated output from caller edits.
    let mut state = previous;
    state.extend(
        files
            .iter()
            .map(|(path, content)| (path.clone(), hash(content))),
    );
    let state = serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?;
    let metadata = file_change(root, ".gpui-box/generated.json", &state)?;
    Ok(PreparedGeneration {
        files,
        state,
        journal: serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
        plan: ChangePlan {
            schema_version: 1,
            cli_version: gpui_box::VERSION.into(),
            upstream_revision: gpui_box::UPSTREAM_REVISION.into(),
            config_sha256,
            recovering_pending_journal: pending_bytes.is_some(),
            pending_journal_sha256: pending_bytes.as_deref().map(hash),
            files: changes,
            metadata,
        },
    })
}

pub fn generate(root: &Path, initial: bool) -> Result<(), String> {
    generate_inner(root, initial.then_some(Template::Starter), None).map(|_| ())
}

fn generate_inner(
    root: &Path,
    initial: Option<Template>,
    expected: Option<&ChangePlan>,
) -> Result<ApplyReceipt, String> {
    let metadata = managed_path(root, ".gpui-box")?;
    fs::create_dir_all(metadata).map_err(|e| e.to_string())?;
    let lock_path = managed_path(root, ".gpui-box/generate.lock")?;
    let lock = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .map_err(|e| format!("Cannot acquire generation lock {}: {e}. Check for another generator before removing a stale lock.", lock_path.display()))?;
    let _guard = GenerationLock(lock_path);
    drop(lock);
    let prepared = prepare(root)?;
    if expected.is_some_and(|expected| *expected != prepared.plan) {
        return Err(
            "Saved generation plan is stale or incompatible; run generate --dry-run again".into(),
        );
    }
    // Recheck even no-op inputs before reporting completion or replacing a journal.
    check_config(root, &prepared.plan)?;
    check_journal(root, prepared.plan.pending_journal_sha256.as_deref())?;
    for change in prepared
        .plan
        .files
        .iter()
        .chain(std::iter::once(&prepared.plan.metadata))
    {
        let content = if change.path == prepared.plan.metadata.path {
            &prepared.state
        } else {
            &prepared.files[&change.path]
        };
        if file_change(root, &change.path, content)? != *change {
            return Err("Generation input changed concurrently; retry generation".into());
        }
    }
    let mut receipt = ApplyReceipt {
        schema_version: 1,
        completed: true,
        plan_sha256: hash(&serde_json::to_vec(&prepared.plan).map_err(|error| error.to_string())?),
        files_written: Vec::new(),
        files_unchanged: Vec::new(),
        recovered_pending_journal: prepared.plan.recovering_pending_journal,
    };
    let changed = prepared
        .plan
        .files
        .iter()
        .chain(std::iter::once(&prepared.plan.metadata))
        .any(|file| file.action != ChangeAction::Unchanged);
    if changed || prepared.plan.recovering_pending_journal {
        check_config(root, &prepared.plan)?;
        write(
            &managed_path(root, ".gpui-box/pending.json")?,
            &prepared.journal,
        )?;
        for change in &prepared.plan.files {
            let content = &prepared.files[&change.path];
            check_config(root, &prepared.plan)?;
            if file_change(root, &change.path, content)? != *change {
                return Err(
                    "Managed file changed concurrently; original preserved and journal retained"
                        .into(),
                );
            }
            apply_change(root, change, content, &mut receipt)?;
        }
        check_config(root, &prepared.plan)?;
        check_journal(root, Some(&hash(&prepared.journal)))?;
        for change in &prepared.plan.files {
            if hash(
                &fs::read(managed_path(root, &change.path)?).map_err(|error| error.to_string())?,
            ) != change.after_sha256
            {
                return Err(
                    "Managed file changed before state commit; recovery journal retained".into(),
                );
            }
        }
        if file_change(root, &prepared.plan.metadata.path, &prepared.state)?
            != prepared.plan.metadata
        {
            return Err("Generation state changed concurrently; recovery journal retained".into());
        }
        apply_change(root, &prepared.plan.metadata, &prepared.state, &mut receipt)?;
        check_journal(root, Some(&hash(&prepared.journal)))?;
        fs::remove_file(managed_path(root, ".gpui-box/pending.json")?)
            .map_err(|e| format!("Complete generation journal: {e}"))?;
    } else {
        receipt
            .files_unchanged
            .extend(prepared.plan.files.iter().map(|file| file.path.clone()));
        receipt
            .files_unchanged
            .push(prepared.plan.metadata.path.clone());
    }
    if let Some(template) = initial {
        let config = AppConfig::load(root)?;
        write(
            &root.join("app/src/lib.rs"),
            match template {
                Template::Starter => include_bytes!("../../../templates/app.rs").as_slice(),
                Template::Adaptive => include_bytes!("../../../templates/adaptive.rs").as_slice(),
            },
        )?;
        if config.targets.iter().any(|target| target == "desktop") {
            write(
                &root.join("desktop/src/main.rs"),
                b"fn main() { gpui_box::application().run(box_app::open); }\n",
            )?;
        }
        if config.targets.iter().any(|target| target == "android") {
            write(&root.join("android/src/lib.rs"), b"#![cfg(target_os = \"android\")]\ngpui_box::android::android_entrypoint!(box_app::open);\n")?;
            write(
                &root.join("android-custom.gradle.kts"),
                b"// Application-owned Android customization point. Preserved during generation.\n",
            )?;
            write(
                &root.join("mise.android.toml"),
                include_bytes!("../../../templates/mise.android.toml"),
            )?;
        }
        let mise = if config.targets.iter().any(|target| target == "desktop") {
            include_bytes!("../../../templates/mise.toml").as_slice()
        } else {
            b"[tools]\nrust = \"1.98.1\"\n"
        };
        write(&root.join("mise.toml"), mise)?;
        write(
            &root.join(".gpui-box/template.json"),
            &serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "template": template.id(),
                "template_version": 1,
                "cli_version": gpui_box::VERSION,
                "upstream_revision": gpui_box::UPSTREAM_REVISION,
                "targets": config.targets,
            }))
            .map_err(|error| error.to_string())?,
        )?;
        write(
            &root.join(".gitignore"),
            b"/target/\n/dist/\n/android-host/.gradle/\n/android-host/app/build/\nlocal.properties\n*.jks\n*.keystore\n.env\n.env.*\nkeystore.properties\n",
        )?;
        write(
            &root.join("README.md"),
            format!("Template {} v1. Configured targets are {}. Platform commands below apply when their target is enabled.\n\n{}{}", template.id(), config.targets.join(", "), include_str!("../../../templates/README.md"), match template {
                Template::Starter => "",
                Template::Adaptive => "\n## Adaptive example\n\nThe shared application retains its selected item and input entity across compact/wide layout changes. Compact navigation opens one detail page with Back, Escape and Android back handling. Wide layouts show both panes. Change the window width or rotate Android to exercise the 720 logical-pixel breakpoint. Keep business persistence in an application service. Process-death restoration requires the documented snapshot hooks.\n",
            }).as_bytes(),
        )?;
    }
    Ok(receipt)
}

fn check_config(root: &Path, plan: &ChangePlan) -> Result<(), String> {
    let bytes =
        fs::read(managed_path(root, "gpui-box.json")?).map_err(|error| error.to_string())?;
    if hash(&bytes) != plan.config_sha256 {
        return Err("Project configuration changed concurrently; retry generation".into());
    }
    Ok(())
}

fn check_journal(root: &Path, expected: Option<&str>) -> Result<(), String> {
    let current = file_change(root, ".gpui-box/pending.json", b"")?.before_sha256;
    if current.as_deref() != expected {
        return Err("Generation journal changed concurrently; original preserved".into());
    }
    Ok(())
}

fn apply_change(
    root: &Path,
    change: &FileChange,
    content: &[u8],
    receipt: &mut ApplyReceipt,
) -> Result<(), String> {
    if change.action == ChangeAction::Unchanged {
        receipt.files_unchanged.push(change.path.clone());
    } else {
        write(&managed_path(root, &change.path)?, content)?;
        receipt.files_written.push(change.path.clone());
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
