use crate::config::AppConfig;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn run(command: &mut Command) -> Result<(), String> {
    crate::outputln!("Running {command:?}");
    if crate::output::is_json() {
        command.stdin(Stdio::null());
        command.stdout(Stdio::from(std::io::stderr()));
    }
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

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AndroidDevice {
    pub serial: String,
    pub state: String,
    pub available: bool,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceInventory {
    pub schema_version: u32,
    pub completed: bool,
    pub platform: &'static str,
    pub devices: Vec<AndroidDevice>,
}

fn parse_devices(text: &str) -> Result<Vec<AndroidDevice>, String> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    if lines.next().map(str::trim) != Some("List of devices attached") {
        return Err("Unexpected adb device inventory header. Check Android platform-tools.".into());
    }
    let mut devices = BTreeMap::new();
    for line in lines {
        let mut fields = line.split_whitespace();
        let serial = fields.next().ok_or("Missing adb device serial")?;
        let mut state = fields.next().ok_or("Missing adb device state")?.to_string();
        if state == "no" {
            if fields.next() != Some("permissions") {
                return Err("Unrecognized adb permission state".into());
            }
            state = "no permissions".into();
        }
        let attributes = fields
            .filter_map(|field| field.split_once(':'))
            .filter(|(key, value)| {
                !key.is_empty()
                    && key
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                    && !value.is_empty()
            })
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let device = AndroidDevice {
            serial: serial.into(),
            available: state == "device",
            state,
            attributes,
        };
        if devices.insert(serial.to_string(), device).is_some() {
            return Err("Duplicate serial in adb device inventory".into());
        }
    }
    Ok(devices.into_values().collect())
}

fn query_devices(command: &mut Command) -> Result<DeviceInventory, String> {
    crate::outputln!("Running {command:?}");
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Query Android devices: {error}"))?;
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        return Err(format!(
            "Android device query failed with {}",
            output.status
        ));
    }
    if output.stdout.len() > 1024 * 1024 {
        return Err("Android device inventory exceeds its 1 MiB bound".into());
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| "Android device inventory must be UTF-8")?;
    let devices = parse_devices(text)?;
    crate::outputln!("{}", text.trim_end());
    Ok(DeviceInventory {
        schema_version: 1,
        completed: true,
        platform: "android",
        devices,
    })
}

fn select_device(devices: &[AndroidDevice], requested: Option<&str>) -> Result<String, String> {
    if let Some(serial) = requested {
        let device = devices
            .iter()
            .find(|device| device.serial == serial)
            .ok_or("Requested Android device was not found. Run gpui-box devices and check --device or ANDROID_SERIAL.")?;
        if !device.available {
            let fix = match device.state.as_str() {
                "unauthorized" => "Unlock the device and authorize USB debugging.",
                "offline" => "Restart or reconnect the emulator/device and retry.",
                "no permissions" => {
                    "Grant host USB access using the Android platform-tools setup instructions."
                }
                _ => "Start Android normally and enable debugging before retrying.",
            };
            return Err(format!(
                "Android device {serial} is {}. {fix}",
                device.state
            ));
        }
        return Ok(serial.into());
    }
    let available = devices
        .iter()
        .filter(|device| device.available)
        .collect::<Vec<_>>();
    match available.as_slice() {
        [device] => Ok(device.serial.clone()),
        [] => Err("No ready Android device. Start an emulator or connect/authorize a device, then run gpui-box devices.".into()),
        _ => Err(format!(
            "Multiple ready Android devices ({}). Select one with --device SERIAL or ANDROID_SERIAL.",
            available.iter().map(|device| device.serial.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn selected_device(serial: Option<&str>) -> Result<String, String> {
    let environment = if serial.is_none() {
        env::var_os("ANDROID_SERIAL")
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "ANDROID_SERIAL must be UTF-8")
            })
            .transpose()?
    } else {
        None
    };
    let requested = serial.or(environment.as_deref().filter(|value| !value.is_empty()));
    select_device(&devices()?.devices, requested)
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
    let selected = if platform == "android" && matches!(operation, "dev" | "install") {
        Some(selected_device(serial)?)
    } else {
        None
    };
    let serial = selected.as_deref().or(serial);
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
pub fn devices() -> Result<DeviceInventory, String> {
    query_devices(connected(None)?.args(["devices", "-l"]))
}
pub fn logs(root: &Path, serial: Option<&str>) -> Result<(), String> {
    let config = AppConfig::load(root)?;
    let serial = selected_device(serial)?;
    let pid = connected(Some(&serial))?
        .args(["shell", "pidof", &config.application_id])
        .output()
        .map_err(|e| e.to_string())?;
    if !pid.status.success() {
        return Err("Cannot query the application process. Check the selected device and use dev android first.".into());
    }
    let pid = String::from_utf8_lossy(&pid.stdout).trim().to_string();
    if !pid.chars().all(|value| value.is_ascii_digit()) || pid.is_empty() {
        return Err("Application is not running. Use dev android first.".into());
    }
    run(connected(Some(&serial))?.args(["logcat", &format!("--pid={pid}")]))
}
#[derive(Debug, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub passed: bool,
    pub required: bool,
    pub message: String,
    pub corrective_action: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub completed: bool,
    pub ready: bool,
    pub readiness_scope: &'static str,
    pub host: String,
    pub targets: Vec<String>,
    pub project_schema_version: u32,
    pub service_schema_version: u32,
    pub update_channel: String,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn check(
        &mut self,
        id: impl Into<String>,
        passed: bool,
        required: bool,
        message: impl Into<String>,
        fix: &str,
    ) {
        let message = message.into();
        if passed {
            crate::outputln!("OK {message}");
        } else if required {
            self.ready = false;
            eprintln!("MISSING {message}. {fix}");
        } else {
            crate::outputln!("RELEASE prerequisite {message}. {fix}");
        }
        self.checks.push(DoctorCheck {
            id: id.into(),
            passed,
            required,
            message,
            corrective_action: (!passed).then(|| fix.to_string()),
        });
    }
}

pub fn doctor(root: &Path) -> Result<DoctorReport, String> {
    let config = AppConfig::load(root)?;
    let distribution = config.distribution(root)?;
    crate::outputln!(
        "Configuration v{} · {} · {}",
        config.schema_version,
        config.application_id,
        config.update_channel
    );
    crate::outputln!("Distribution {}", distribution.display());
    let mut report = DoctorReport {
        schema_version: 1,
        completed: true,
        ready: true,
        readiness_scope: "declared_prerequisites",
        host: format!("{}-{}", env::consts::ARCH, env::consts::OS),
        targets: config.targets.clone(),
        project_schema_version: config.schema_version,
        service_schema_version: config.services.schema_version,
        update_channel: config.update_channel.clone(),
        checks: Vec::new(),
    };
    crate::outputln!(
        "Service declaration schema {}",
        config.services.schema_version
    );
    for declaration in &config.services.permissions {
        crate::outputln!(
            "CONFIGURED Android {:?} permission. Android host adapter is available. Runtime authorization is queried in the app. See #20/#2 for per-operation qualification.",
            declaration.kind
        );
    }
    for scheme in &config.services.url_schemes {
        crate::outputln!(
            "CONFIGURED Android custom URL scheme {scheme}. Validate host routing through the packaged application."
        );
    }
    for (index, asset) in config.assets.iter().enumerate() {
        report.check(
            format!("asset.{index}"),
            root.join(asset).exists(),
            true,
            format!("asset {}", asset.display()),
            "Create it or update assets in gpui-box.json.",
        );
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
            .stdin(Stdio::null())
            .args(["exec", "--", program])
            .args(args);
        let result = command.output();
        let available = result.as_ref().is_ok_and(|result| result.status.success());
        let message = match result {
            Ok(result) if available => format!(
                "{program} {}",
                String::from_utf8_lossy(&result.stdout).trim()
            ),
            _ => program.into(),
        };
        report.check(format!("tool.{program}"), available, true, message, fix);
    }
    if config.targets.iter().any(|t| t == "android") {
        let wrapper = crate::package::verify_wrapper(&root.join("android-host"));
        let wrapper_message = match &wrapper {
            Ok(()) => "Gradle Wrapper JAR and distribution SHA-256 pins".to_string(),
            Err(error) => format!("Gradle Wrapper. {error}"),
        };
        report.check(
            "android.wrapper",
            wrapper.is_ok(),
            true,
            wrapper_message,
            "Run gpui-box generate.",
        );
        report.check("release.bundletool", crate::package::bundletool().is_ok(), false,
            "optional release bundletool 1.18.3 checksum",
            "Set GPUI_BOX_BUNDLETOOL to the checksum-pinned bundletool-all-1.18.3.jar. See docs/M4_ANDROID_PACKAGING.md.");
        for name in [
            "GPUI_BOX_ANDROID_KEYSTORE",
            "GPUI_BOX_ANDROID_STORE_PASSWORD",
            "GPUI_BOX_ANDROID_KEY_ALIAS",
            "GPUI_BOX_ANDROID_KEY_PASSWORD",
        ] {
            report.check(
                format!("release.{name}"),
                env::var_os(name).is_some_and(|value| !value.is_empty()),
                false,
                format!("{name} configured"),
                "Configure release signing before release packaging.",
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
                .stdin(Stdio::null())
                .args(["-E", "android", "exec", "--"])
                .args(&args)
                .output();
            let available = result.is_ok_and(|output| {
                output.status.success()
                    && format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    )
                    .contains(expected)
            });
            report.check(
                format!("android.tool.{}", args[0]),
                available,
                true,
                format!("pinned {} {expected}", args[0]),
                &format!("Run {fix}."),
            );
        }
        let installed = Command::new("mise")
            .current_dir(root)
            .stdin(Stdio::null())
            .args(["exec", "--", "rustup", "target", "list", "--installed"])
            .output();
        for target in ["aarch64-linux-android", "x86_64-linux-android"] {
            let available = installed.as_ref().is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .any(|line| line == target)
            });
            report.check(
                format!("android.rust.{target}"),
                available,
                true,
                format!("Rust target {target}"),
                &format!("Run mise exec -- rustup target add {target}."),
            );
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
                    report.check(
                        format!("android.sdk.{path}"),
                        sdk.join(path).exists(),
                        true,
                        format!("Android {path}"),
                        "Install the pinned SDK packages and run sdkmanager --licenses.",
                    );
                }
            }
            Err(error) => {
                report.check("android.sdk", false, true, "Android SDK", &error);
            }
        }
        crate::outputln!(
            "Android tool pins JDK 17.0.21+9, Gradle 8.13, cargo-ndk 4.1.2. Run mise -E android install and rustup target add aarch64-linux-android x86_64-linux-android."
        );
    }
    if cfg!(windows) {
        crate::outputln!(
            "Desktop requires MSVC C++ Build Tools and a Windows SDK. The build command validates linking."
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_optional_checks_preserve_readiness_and_required_failures_clear_it() {
        let mut report = DoctorReport {
            schema_version: 1,
            completed: true,
            ready: true,
            readiness_scope: "declared_prerequisites",
            host: "test-host".into(),
            targets: vec!["android".into()],
            project_schema_version: 1,
            service_schema_version: 1,
            update_channel: "development".into(),
            checks: Vec::new(),
        };
        report.check(
            "optional",
            false,
            false,
            "optional release input",
            "Configure release input",
        );
        assert!(report.ready);
        report.check("required", false, true, "required tool", "Install tool");
        assert!(!report.ready);
        report.check("later", true, true, "later passing check", "unused");
        assert!(!report.ready);
        assert_eq!(
            report.checks[0].corrective_action.as_deref(),
            Some("Configure release input")
        );
        assert!(report.checks[2].corrective_action.is_none());
    }

    const INVENTORY: &str = "List of devices attached\r\n\
        emulator-5556 offline transport_id:2\r\n\
        emulator-5554 device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 device:emu64xa transport_id:1\r\n\
        usb-unauthorized unauthorized usb:1-1 transport_id:3\r\n\
        usb-denied no permissions (user in plugdev group; are your udev rules wrong?); see [https://developer.android.com/tools/device.html] usb:1-2 transport_id:4\r\n\
        recovery-device recovery transport_id:5\r\n";

    #[test]
    fn inventory_preserves_states_and_attributes_in_serial_order() {
        let devices = parse_devices(INVENTORY).unwrap();
        assert_eq!(devices.len(), 5);
        assert!(
            devices
                .windows(2)
                .all(|pair| pair[0].serial < pair[1].serial)
        );
        assert_eq!(devices[0].serial, "emulator-5554");
        assert!(devices[0].available);
        assert_eq!(devices[0].attributes["model"], "sdk_gphone64_x86_64");
        assert_eq!(devices[0].attributes["transport_id"], "1");
        assert!(devices[1..].iter().all(|device| !device.available));
        assert_eq!(devices[3].state, "no permissions");
        assert_eq!(devices[3].attributes["usb"], "1-2");
        assert!(
            parse_devices("List of devices attached\n\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn inventory_rejects_missing_header_malformed_rows_and_duplicate_serials() {
        for text in [
            "",
            "adb server diagnostic\nList of devices attached\n",
            "List of devices attached\nserial-only\n",
            "List of devices attached\nserial no unknown\n",
            "List of devices attached\na device\na offline\n",
        ] {
            assert!(parse_devices(text).is_err(), "reject {text:?}");
        }
    }

    #[test]
    fn selection_uses_only_available_devices_and_honors_explicit_selection() {
        let mut devices = parse_devices(INVENTORY).unwrap();
        assert_eq!(select_device(&devices, None).unwrap(), "emulator-5554");
        assert_eq!(
            select_device(&devices, Some("emulator-5554")).unwrap(),
            "emulator-5554"
        );
        devices.extend(parse_devices("List of devices attached\nother device\n").unwrap());
        assert!(
            select_device(&devices, None)
                .unwrap_err()
                .contains("Multiple ready")
        );
        assert_eq!(select_device(&devices, Some("other")).unwrap(), "other");
        assert!(select_device(&[], None).unwrap_err().contains("No ready"));
        assert!(
            select_device(&devices[1..4], None)
                .unwrap_err()
                .contains("No ready")
        );
    }

    #[test]
    fn selection_diagnoses_unknown_offline_unauthorized_and_permission_states() {
        let devices = parse_devices(INVENTORY).unwrap();
        for (serial, expected) in [
            ("missing", "not found"),
            ("emulator-5556", "Restart or reconnect"),
            ("usb-unauthorized", "authorize USB debugging"),
            ("usb-denied", "Grant host USB access"),
            ("recovery-device", "Start Android normally"),
        ] {
            assert!(
                select_device(&devices, Some(serial))
                    .unwrap_err()
                    .contains(expected)
            );
        }
    }
}
