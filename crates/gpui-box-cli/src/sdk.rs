use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const MANIFEST: &str = "gpui-box-sdk.json";
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkManifest {
    pub schema_version: u32,
    pub source_revision: String,
    pub upstream_revision: String,
    pub exporter_version: String,
    pub scope: String,
    pub files: BTreeMap<String, String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn relative_path(name: &str) -> Result<PathBuf, String> {
    if name.is_empty()
        || name.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || name.chars().any(char::is_control)
        || name.split('/').any(|part| {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with(['.', ' '])
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        })
    {
        return Err("SDK entries require portable relative paths".into());
    }
    Ok(PathBuf::from(name))
}

fn revision(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_manifest(manifest: &SdkManifest) -> Result<(), String> {
    if manifest.schema_version != 1
        || !revision(&manifest.source_revision)
        || manifest.upstream_revision != gpui_box::UPSTREAM_REVISION
        || manifest.exporter_version.is_empty()
        || manifest.files.len() > 10_000
    {
        return Err("Unsupported SDK manifest schema or provenance".into());
    }
    let mut names = BTreeSet::new();
    for (name, hash) in &manifest.files {
        relative_path(name)?;
        if name.eq_ignore_ascii_case(MANIFEST)
            || !names.insert(name.to_lowercase())
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("Invalid SDK manifest file {name}"));
        }
    }
    for required in REQUIRED {
        if !manifest.files.contains_key(*required) {
            return Err(format!("SDK source is missing {required}"));
        }
    }
    Ok(())
}

pub fn verify(root: &Path) -> Result<SdkManifest, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let manifest_path = root.join(MANIFEST);
    let metadata = fs::symlink_metadata(&manifest_path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("SDK manifest must be a regular file of at most 4 MiB".into());
    }
    let manifest: SdkManifest =
        serde_json::from_slice(&fs::read(manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Read SDK manifest: {error}"))?;
    validate_manifest(&manifest)?;
    let mut pending = vec![PathBuf::new()];
    let mut files = BTreeSet::new();
    let mut total = 0_u64;
    let mut entries = 0;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(root.join(&directory)).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            entries += 1;
            if entries > 10_000 {
                return Err("SDK exceeds 10000 entries".into());
            }
            let relative = directory.join(entry.file_name());
            let name = relative
                .iter()
                .map(|part| part.to_str().ok_or("SDK paths must be UTF-8"))
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            relative_path(&name)?;
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            if kind.is_dir() {
                pending.push(relative);
                continue;
            }
            if !kind.is_file() {
                return Err(format!("SDK contains a link or special file {name}"));
            }
            if name == MANIFEST {
                continue;
            }
            let expected = manifest
                .files
                .get(&name)
                .ok_or_else(|| format!("SDK contains an unlisted file {name}"))?;
            let size = entry.metadata().map_err(|error| error.to_string())?.len();
            total = total.checked_add(size).ok_or("SDK byte limit exceeded")?;
            if size > MAX_FILE || total > MAX_TOTAL {
                return Err("SDK content exceeds its byte limit".into());
            }
            let mut bytes = Vec::new();
            fs::File::open(entry.path())
                .map_err(|error| error.to_string())?
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() as u64 != size || &digest(&bytes) != expected {
                return Err(format!("SDK file digest mismatch {name}"));
            }
            files.insert(name);
        }
    }
    if files.len() != manifest.files.len() {
        return Err("SDK is missing committed files".into());
    }
    let upstream: serde_json::Value = serde_json::from_slice(
        &fs::read(root.join("vendor/gpui/UPSTREAM.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if upstream["revision"] != manifest.upstream_revision {
        return Err("SDK source uses an incompatible GPUI revision".into());
    }
    Ok(manifest)
}

fn git(source: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(source).stdin(Stdio::null());
    command
}

pub fn export(source: &Path, destination: &Path) -> Result<SdkManifest, String> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err("SDK destination already exists".into());
    }
    let parent = destination
        .parent()
        .ok_or("SDK destination requires a parent")?
        .canonicalize()
        .map_err(|error| format!("Open SDK destination parent: {error}"))?;
    let destination = parent.join(
        destination
            .file_name()
            .ok_or("SDK destination requires a name")?,
    );
    let prefix = git(source)
        .args(["rev-parse", "--show-prefix"])
        .output()
        .map_err(|error| format!("Resolve SDK checkout root: {error}"))?;
    if !prefix.status.success() || !prefix.stdout.iter().all(u8::is_ascii_whitespace) {
        return Err("SDK source must be the root of a Git checkout".into());
    }
    let revision = git(source)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()
        .map_err(|error| format!("Resolve committed SDK source: {error}"))?;
    if !revision.status.success() {
        return Err("SDK source requires a committed Git checkout".into());
    }
    let revision = String::from_utf8(revision.stdout)
        .map_err(|error| error.to_string())?
        .trim()
        .to_owned();
    if !self::revision(&revision) {
        return Err("SDK source requires a SHA-1 Git commit identity".into());
    }
    let attributes = git(source)
        .args(["rev-parse", "--git-path", "info/attributes"])
        .output()
        .map_err(|error| error.to_string())?;
    if !attributes.status.success() {
        return Err("Resolve checkout-local attributes failed".into());
    }
    let attributes = String::from_utf8(attributes.stdout).map_err(|error| error.to_string())?;
    let attributes = source.join(attributes.trim());
    if attributes.exists()
        && fs::metadata(attributes)
            .map_err(|error| error.to_string())?
            .len()
            != 0
    {
        return Err(
            "SDK export requires empty checkout-local info/attributes for reproducibility".into(),
        );
    }
    let staging = parent.join(format!(".gpui-box-sdk-{}", std::process::id()));
    fs::create_dir(&staging).map_err(|error| format!("Create exclusive SDK staging: {error}"))?;
    let result = export_staged(source, &destination, revision, &staging);
    result.map_err(|error| {
        format!(
            "{error}. Inspect retained SDK staging {}",
            staging.display()
        )
    })
}

fn export_staged(
    source: &Path,
    destination: &Path,
    revision: String,
    staging: &Path,
) -> Result<SdkManifest, String> {
    let archive_path = staging.join("source.zip");
    let archive_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive_path)
        .map_err(|error| error.to_string())?;
    let output = git(source)
        .args([
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.eol=lf",
            "-c",
            "core.attributesFile=",
        ])
        .env("GIT_ATTR_NOSYSTEM", "1")
        .args(["archive", "--format=zip"])
        .stdout(archive_file)
        .arg(&revision)
        .output()
        .map_err(|error| format!("Archive committed SDK source: {error}"))?;
    if !output.status.success() {
        return Err("Git could not archive the committed SDK source; staging retained".into());
    }
    if fs::metadata(&archive_path)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_TOTAL
    {
        return Err("SDK archive exceeds 512 MiB; staging retained".into());
    }
    let mut archive =
        zip::ZipArchive::new(fs::File::open(&archive_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if archive.len() > 10_000 {
        return Err("SDK archive exceeds 10000 entries".into());
    }
    let root = staging.join("sdk");
    fs::create_dir(&root).map_err(|error| error.to_string())?;
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry
            .name()
            .strip_suffix('/')
            .unwrap_or(entry.name())
            .to_owned();
        let relative = relative_path(&name)?;
        if !names.insert(name.to_lowercase()) || entry.is_symlink() {
            return Err(format!("Duplicate or linked SDK entry {name}"));
        }
        if entry.is_dir() {
            continue;
        }
        if name.eq_ignore_ascii_case(MANIFEST) || entry.size() > MAX_FILE {
            return Err(format!(
                "Unsupported, duplicate or oversized SDK entry {name}"
            ));
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        total += bytes.len() as u64;
        if bytes.len() as u64 > MAX_FILE || total > MAX_TOTAL {
            return Err("SDK content exceeds its byte limit".into());
        }
        let path = root.join(relative);
        fs::create_dir_all(path.parent().ok_or("Missing SDK parent")?)
            .map_err(|error| error.to_string())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| error.to_string())?;
        drop(file);
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|error| error.to_string())?;
        }
        files.insert(name, digest(&bytes));
    }
    let manifest = SdkManifest {
        schema_version: 1, source_revision: revision, upstream_revision: gpui_box::UPSTREAM_REVISION.into(),
        exporter_version: gpui_box::VERSION.into(), scope: "complete committed source tree; registry/git dependencies and toolchains use separate caches".into(), files,
    };
    fs::write(
        root.join(MANIFEST),
        serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    verify(&root)?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err("SDK destination appeared during export; staging retained".into());
    }
    fs::rename(root, destination)
        .map_err(|error| format!("Publish SDK: {error}; staging retained"))?;
    drop(archive);
    fs::remove_file(archive_path)
        .and_then(|()| fs::remove_dir(staging))
        .map_err(|error| {
            format!(
                "SDK published at {}, staging cleanup failed: {error}",
                destination.display()
            )
        })?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::relative_path;

    #[test]
    fn archive_names_reject_platform_aliases_and_traversal() {
        for name in [
            "",
            "/absolute",
            "../parent",
            "a/./b",
            "a//b",
            "a/",
            "a\\b",
            "C:/drive",
            "a:stream",
            "a\n",
            "AUX.txt",
            "a/COM1.rs",
            "a/end.",
            "a/end ",
            "a/*",
        ] {
            assert!(relative_path(name).is_err(), "{name:?}");
        }
        for name in ["Cargo.toml", "crates/example/src/lib.rs", "assets/中文.txt"] {
            assert!(relative_path(name).is_ok(), "{name:?}");
        }
    }
}
