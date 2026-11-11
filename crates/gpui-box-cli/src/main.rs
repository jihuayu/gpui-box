mod config;
mod generate;
mod output;
mod package;
mod workflow;
use output::CliError;
use serde_json::{Value, json};
use std::{env, path::PathBuf, process::ExitCode};

macro_rules! outputln {
    ($($argument:tt)*) => { crate::output::message(format_args!($($argument)*)) };
}
pub(crate) use outputln;

fn execute(arguments: &[String]) -> Result<Value, CliError> {
    let root = env::current_dir()
        .map_err(|e| CliError::new("CURRENT_DIRECTORY_UNAVAILABLE", e.to_string()))?;
    match arguments {
        [] => return Ok(help()),
        [command] if matches!(command.as_str(), "--help" | "-h" | "help") => return Ok(help()),
        [command] if matches!(command.as_str(), "--version" | "-V") => {
            outputln!("gpui-box {}", gpui_box::VERSION);
            return Ok(json!({"version": gpui_box::VERSION}));
        }
        [command] if command == "info" => {
            outputln!(
                "gpui-box {}\nGPUI revision {}",
                gpui_box::VERSION,
                gpui_box::UPSTREAM_REVISION
            );
            outputln!(
                "Host {}-{}\nDesktop targets Windows, macOS, Linux\nMobile targets Android, iOS",
                env::consts::ARCH,
                env::consts::OS
            );
            return Ok(json!({
                "version": gpui_box::VERSION,
                "upstream_revision": gpui_box::UPSTREAM_REVISION,
                "host": {"architecture": env::consts::ARCH, "os": env::consts::OS},
                "desktop_targets": ["windows", "macos", "linux"],
                "mobile_targets": ["android", "ios"]
            }));
        }
        [command, destination, option, distribution]
            if command == "new" && option == "--distribution" =>
        {
            let distribution = PathBuf::from(distribution).canonicalize().map_err(|e| {
                CliError::new("DISTRIBUTION_UNAVAILABLE", format!("Distribution: {e}"))
            })?;
            let destination = root.join(destination);
            generate::create(&destination, distribution)
                .map_err(|e| CliError::new("PROJECT_CREATION_FAILED", e))?;
            outputln!(
                "Created {}. Run mise trust, mise install, then mise run dev:desktop.",
                destination.display()
            );
            return Ok(json!({"destination": destination}));
        }
        [command] if command == "generate" => {
            generate::generate(&root, false).map_err(|e| CliError::new("GENERATION_FAILED", e))?;
            outputln!(
                "Updated managed files. Application source and native customizations preserved."
            );
        }
        [command, option] if command == "generate" && option == "--dry-run" => {
            let plan = generate::plan(&root)
                .map_err(|error| CliError::new("GENERATION_PLAN_FAILED", error))?;
            if !output::is_json() {
                for file in plan.files.iter().chain(std::iter::once(&plan.metadata)) {
                    outputln!("{:?} {}", file.action, file.path);
                }
            }
            return serde_json::to_value(plan)
                .map_err(|error| CliError::new("GENERATION_PLAN_FAILED", error.to_string()));
        }
        [command] if command == "doctor" => {
            workflow::doctor(&root).map_err(|e| CliError::new("DIAGNOSTICS_FAILED", e))?
        }
        [command, platform, option]
            if command == "package" && platform == "android" && option == "--release" =>
        {
            package::android(&root).map_err(|e| CliError::new("PACKAGING_FAILED", e))?
        }
        [command, platform, option]
            if command == "package" && platform == "desktop" && option == "--portable" =>
        {
            package::windows_portable(&root).map_err(|e| CliError::new("PACKAGING_FAILED", e))?
        }
        [command] if command == "devices" => {
            workflow::devices().map_err(|e| CliError::new("DEVICE_QUERY_FAILED", e))?
        }
        [command] if command == "logs" => {
            workflow::logs(&root, None).map_err(|e| CliError::new("LOG_STREAM_FAILED", e))?
        }
        [command, option, serial] if command == "logs" && option == "--device" => {
            workflow::logs(&root, Some(serial))
                .map_err(|e| CliError::new("LOG_STREAM_FAILED", e))?
        }
        [command, platform]
            if matches!(command.as_str(), "dev" | "build" | "install" | "package") =>
        {
            workflow::workflow(&root, command, platform, None)
                .map_err(|e| CliError::new("WORKFLOW_FAILED", e))?
        }
        [command, platform, option, serial]
            if matches!(command.as_str(), "dev" | "build" | "install" | "package")
                && option == "--device" =>
        {
            workflow::workflow(&root, command, platform, Some(serial))
                .map_err(|e| CliError::new("WORKFLOW_FAILED", e))?
        }
        _ => {
            return Err(CliError::new(
                "INVALID_ARGUMENTS",
                "Unknown command or invalid arguments. Run gpui-box --help for available commands.",
            ));
        }
    }
    Ok(json!({"completed": true}))
}
fn main() -> ExitCode {
    let mut arguments = env::args().skip(1).collect::<Vec<_>>();
    let json_count = arguments
        .iter()
        .filter(|argument| *argument == "--json")
        .count();
    output::set_json(json_count != 0);
    arguments.retain(|argument| argument != "--json");
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    let result = if json_count > 1 {
        Err(CliError::new("INVALID_ARGUMENTS", "Use --json once"))
    } else if output::is_json() && matches!(command, "dev" | "logs") {
        Err(CliError::new(
            "STREAMING_JSON_UNSUPPORTED",
            "JSON responses require a finite command. Use human output for dev and logs.",
        ))
    } else {
        execute(&arguments)
    };
    let success = result.is_ok();
    if output::is_json() {
        if let Err(error) = output::respond(command, result) {
            eprintln!("JSON output failed: {error}");
            return ExitCode::FAILURE;
        }
        return if success {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::FAILURE
        }
    }
}
fn help() -> Value {
    const HELP: &str = "gpui-box - desktop and mobile developer tools\n\nUsage gpui-box COMMAND\n\n  new PATH --distribution PATH  Generate a desktop/Android app\n  generate                      Update unmodified managed files\n  doctor                        Inspect config and prerequisites\n  dev desktop|android           Run the application\n  build desktop|android         Build a development artifact\n  install android               Install on the selected device\n  package android               Assemble a debug APK\n  package android --release     Sign and inspect release APK/AAB\n  package desktop --portable    Bundle Windows development output\n  devices                       List Android devices\n  logs [--device SERIAL]        Follow application logs\n  info | --version | --help\n\nAndroid development workflows accept --device SERIAL. Release requires explicit signing environment variables and checksum-pinned GPUI_BOX_BUNDLETOOL. See docs/M4_ANDROID_PACKAGING.md in the distribution. Update channels follow M5.";
    outputln!(
        "{HELP}\n\nUse generate --dry-run to inspect managed changes without writing files. Add --json once for a schema-v1 response from finite commands. Diagnostics and child output go to stderr. dev/logs use human output."
    );
    json!({"help": HELP, "response_schema_version": 1, "json_streaming_commands": false})
}
