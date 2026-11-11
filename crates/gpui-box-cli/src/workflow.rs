use crate::config::AppConfig;
use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

pub fn run(command: &mut Command) -> Result<(), String> {
    println!("Running {command:?}");
    let status = command
        .status()
        .map_err(|e| format!("Start command: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Command failed with {status}"))
    }
}
pub(crate) fn sdk() -> Result<PathBuf, String> {
    env::var_os("ANDROID_HOME").or_else(|| env::var_os("ANDROID_SDK_ROOT")).map(PathBuf::from)
        .ok_or_else(|| "Set ANDROID_HOME to the Android SDK directory. Install platform-tools, platforms;android-36, build-tools;36.0.0 and ndk;30.0.16248370.".into())
}
fn adb() -> Result<PathBuf, String> {
    Ok(sdk()?
        .join("platform-tools")
        .join(if cfg!(windows) { "adb.exe" } else { "adb" }))
}
fn connected(serial: Option<&str>) -> Result<Command, String> {
    let mut command = Command::new(adb()?);
    if let Some(serial) = serial {
        command.args(["-s", serial]);
    }
    Ok(command)
}
fn target(config: &AppConfig, platform: &str) -> Result<(), String> {
    if config.targets.iter().any(|value| value == platform) {
        Ok(())
    } else {
        Err(format!("Target {platform} is disabled in gpui-box.json"))
    }
}
pub fn workflow(
    root: &Path,
    operation: &str,
    platform: &str,
    serial: Option<&str>,
) -> Result<(), String> {
    let config = AppConfig::load(root)?;
    config.distribution(root)?;
    target(&config, platform)?;
    crate::generate::generate(root, false)?;
    match (operation, platform) {
        ("build" | "dev", "desktop") => {
            let task = if operation == "dev" { "run" } else { "build" };
            run(Command::new("mise").current_dir(root)
                .args(["exec", "--", "cargo", task, "-p", "box-desktop"]))
        }
        ("build" | "dev" | "install" | "package", "android") => {
            let task = if matches!(operation, "dev" | "install") {
                ":app:installDebug"
            } else {
                ":app:assembleDebug"
            };
            let mut build = crate::package::gradle(root, &[task])?;
            if let Some(serial) = serial {
                build.env("ANDROID_SERIAL", serial);
            }
            run(&mut build)?;
            if operation == "dev" {
                let activity = format!("{}/dev.gpui.box.MainActivity", config.application_id);
                run(connected(serial)?.args(["shell", "am", "start", "-W", "-n", &activity]))?;
            }
            Ok(())
        }
        ("package", "desktop") => Err("Desktop installer packaging is tracked in M7. Use build desktop for the development executable.".into()),
        _ => Err(format!("Unsupported workflow {operation} {platform}. Use desktop or android.")),
    }
}
pub fn devices() -> Result<(), String> {
    run(connected(None)?.arg("devices").arg("-l"))
}
pub fn logs(root: &Path, serial: Option<&str>) -> Result<(), String> {
    let config = AppConfig::load(root)?;
    let pid = connected(serial)?
        .args(["shell", "pidof", &config.application_id])
        .output()
        .map_err(|e| e.to_string())?;
    let pid = String::from_utf8_lossy(&pid.stdout).trim().to_string();
    if !pid.chars().all(|value| value.is_ascii_digit()) || pid.is_empty() {
        return Err("Application is not running. Use dev android first.".into());
    }
    run(connected(serial)?.args(["logcat", &format!("--pid={pid}")]))
}
pub fn doctor(root: &Path) -> Result<(), String> {
    let config = AppConfig::load(root)?;
    let distribution = config.distribution(root)?;
    println!(
        "Configuration v{} · {} · {}",
        config.schema_version, config.application_id, config.update_channel
    );
    println!("Distribution {}", distribution.display());
    let mut missing = false;
    println!(
        "Service declaration schema {}",
        config.services.schema_version
    );
    for declaration in &config.services.permissions {
        println!(
            "CONFIGURED Android {:?} permission. Android host adapter is available. Runtime authorization is queried in the app. See #20/#2 for per-operation qualification.",
            declaration.kind
        );
    }
    for scheme in &config.services.url_schemes {
        println!(
            "CONFIGURED Android custom URL scheme {scheme}. Validate host routing through the packaged application."
        );
    }
    for asset in &config.assets {
        if !root.join(asset).exists() {
            missing = true;
            eprintln!(
                "MISSING asset {}. Create it or update assets in gpui-box.json.",
                asset.display()
            );
        }
    }
    for (program, args, fix) in [
        (
            "mise",
            vec!["--version"],
            "Install mise and run mise install",
        ),
        ("rustc", vec!["--version"], "Run mise install"),
        ("cargo", vec!["--version"], "Run mise install"),
        ("git", vec!["--version"], "Install Git"),
    ] {
        let mut command = Command::new("mise");
        command
            .current_dir(root)
            .args(["exec", "--", program])
            .args(args);
        match command.output() {
            Ok(result) if result.status.success() => {
                println!("OK {}", String::from_utf8_lossy(&result.stdout).trim())
            }
            _ => {
                eprintln!("MISSING {program}. {fix}");
                missing = true;
            }
        }
    }
    if config.targets.iter().any(|t| t == "android") {
        match crate::package::verify_wrapper(&root.join("android-host")) {
            Ok(()) => println!("OK Gradle Wrapper JAR and distribution SHA-256 pins"),
            Err(error) => {
                missing = true;
                eprintln!("INVALID Gradle Wrapper. {error}. Run gpui-box generate.");
            }
        }
        match crate::package::bundletool() {
            Ok(_) => println!("OK optional release bundletool 1.18.3 checksum"),
            Err(error) => println!("RELEASE prerequisite {error}"),
        }
        for name in [
            "GPUI_BOX_ANDROID_KEYSTORE",
            "GPUI_BOX_ANDROID_STORE_PASSWORD",
            "GPUI_BOX_ANDROID_KEY_ALIAS",
            "GPUI_BOX_ANDROID_KEY_PASSWORD",
        ] {
            println!(
                "RELEASE {name} {}",
                if env::var_os(name).is_some_and(|value| !value.is_empty()) {
                    "configured"
                } else {
                    "unset"
                }
            );
        }
        for (args, expected, fix) in [
            (
                vec!["java", "-version"],
                "17.0.21",
                "mise -E android install",
            ),
            (
                vec!["gradle", "--version"],
                "Gradle 8.13",
                "mise -E android install",
            ),
            (
                vec!["cargo", "ndk", "--version"],
                "4.1.2",
                "mise -E android install",
            ),
        ] {
            let result = Command::new("mise")
                .current_dir(root)
                .args(["-E", "android", "exec", "--"])
                .args(&args)
                .output();
            match result {
                Ok(output)
                    if output.status.success()
                        && format!(
                            "{}{}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        )
                        .contains(expected) =>
                {
                    println!("OK {} {expected}", args[0])
                }
                _ => {
                    missing = true;
                    eprintln!("MISSING pinned {} {expected}. Run {fix}.", args[0]);
                }
            }
        }
        let installed = Command::new("mise")
            .current_dir(root)
            .args(["exec", "--", "rustup", "target", "list", "--installed"])
            .output();
        for target in ["aarch64-linux-android", "x86_64-linux-android"] {
            if installed.as_ref().is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .any(|line| line == target)
            }) {
                println!("OK Rust target {target}");
            } else {
                missing = true;
                eprintln!(
                    "MISSING Rust target {target}. Run mise exec -- rustup target add {target}."
                );
            }
        }
        match sdk() {
            Ok(sdk) => {
                for path in [
                    if cfg!(windows) {
                        "platform-tools/adb.exe"
                    } else {
                        "platform-tools/adb"
                    },
                    "platforms/android-36",
                    "build-tools/36.0.0",
                    "ndk/30.0.16248370",
                    "licenses/android-sdk-license",
                ] {
                    if sdk.join(path).exists() {
                        println!("OK Android {path}");
                    } else {
                        missing = true;
                        eprintln!(
                            "MISSING Android {path}. Install the pinned SDK packages and run sdkmanager --licenses."
                        );
                    }
                }
            }
            Err(error) => {
                eprintln!("{error}");
                missing = true;
            }
        }
        println!(
            "Android tool pins JDK 17.0.21+9, Gradle 8.13, cargo-ndk 4.1.2. Run mise -E android install and rustup target add aarch64-linux-android x86_64-linux-android."
        );
    }
    if cfg!(windows) {
        println!(
            "Desktop requires MSVC C++ Build Tools and a Windows SDK. The build command validates linking."
        );
    }
    if missing {
        Err("Missing prerequisites listed above".into())
    } else {
        Ok(())
    }
}
