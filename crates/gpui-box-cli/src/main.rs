mod config;
mod generate;
mod output;
mod package;
mod platform;
mod reference;
mod registry;
mod sdk;
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
        [command] if command == "reference" => {
            if !output::is_json() {
                print!("{}", reference::markdown());
            }
            return serde_json::to_value(reference::contract())
                .map_err(|error| CliError::new("REFERENCE_FAILED", error.to_string()));
        }
        [command] if matches!(command.as_str(), "--version" | "-V") => {
            outputln!("gpui-box {}", gpui_box::VERSION);
            return Ok(json!({"version": gpui_box::VERSION}));
        }
        [command, operation] if command == "config" && operation == "schema" => {
            let schema = config::schema();
            if !output::is_json() {
                outputln!(
                    "{}",
                    serde_json::to_string_pretty(&schema).expect("serialize schema")
                );
            }
            return Ok(schema);
        }
        [command, operation] if command == "config" && operation == "validate" => {
            let report = config::validation_report(&root)
                .map_err(|error| CliError::new("CONFIG_VALIDATION_FAILED", error))?;
            outputln!(
                "Valid project schema {} / service schema {} · SHA256 {}",
                report["project_schema_version"],
                report["service_schema_version"],
                report["configuration_sha256"].as_str().unwrap_or_default()
            );
            return Ok(report);
        }
        [command, operation] if command == "platform" && operation == "list" => {
            let inventory = platform::inventory(&root)
                .map_err(|error| CliError::new("PLATFORM_QUERY_FAILED", error))?;
            if !output::is_json() {
                outputln!(
                    "Host {} · Project present {}",
                    inventory.host,
                    inventory.project_present
                );
                for platform in &inventory.platforms {
                    outputln!(
                        "{} · Target {} · Generate {} · Build adapter {} · Compatible host {} · Project enabled {:?} · Toolchain {} · Maturity {}",
                        platform.id,
                        platform.cli_target.unwrap_or("unavailable"),
                        platform.configuration_generation,
                        platform.build_adapter,
                        platform.compatible_host,
                        platform.project_enabled,
                        platform.toolchain_readiness,
                        platform.backend_maturity
                    );
                    outputln!(
                        "  {} · Evidence {}",
                        platform.remaining_acceptance,
                        platform.evidence
                    );
                }
                outputln!(
                    "Use doctor inside a project to inspect declared toolchain prerequisites."
                );
            }
            return serde_json::to_value(inventory)
                .map_err(|error| CliError::new("PLATFORM_QUERY_FAILED", error.to_string()));
        }
        [command, operation, target, option]
            if command == "platform" && operation == "add" && option == "--dry-run" =>
        {
            let plan = generate::platform_plan(&root, target)
                .map_err(|error| CliError::new("PLATFORM_ADDITION_FAILED", error))?;
            outputln!(
                "Planned platform {}. Enabled {}. Entry points and managed changes appear in the JSON result.",
                target,
                plan.already_enabled
            );
            return serde_json::to_value(plan)
                .map_err(|error| CliError::new("PLATFORM_ADDITION_FAILED", error.to_string()));
        }
        [command, operation, target] if command == "platform" && operation == "add" => {
            let receipt = generate::platform_add(&root, target)
                .map_err(|error| CliError::new("PLATFORM_ADDITION_FAILED", error))?;
            outputln!(
                "Platform {} enabled. Created {} entry points. Existing application sources preserved.",
                target,
                receipt.entry_points_created.len()
            );
            return serde_json::to_value(receipt)
                .map_err(|error| CliError::new("PLATFORM_ADDITION_FAILED", error.to_string()));
        }
        [command] | [command, _]
            if command == "list" || command == "info" && arguments.len() == 2 =>
        {
            if command == "list" && arguments.len() != 1 {
                return Err(CliError::new(
                    "INVALID_ARGUMENTS",
                    "Use list without arguments",
                ));
            }
            let snapshot = registry::bundled()
                .map_err(|error| CliError::new("REGISTRY_QUERY_FAILED", error))?;
            let view = snapshot
                .view(arguments.get(1).map(String::as_str))
                .map_err(|error| CliError::new("REGISTRY_QUERY_FAILED", error))?;
            if !output::is_json() {
                for entry in &view.entries {
                    outputln!("{} {} · {}", entry.id, entry.version, entry.title);
                    if command == "info" {
                        outputln!(
                            "Kind {:?} · Delivery {:?} · License {}\nSDK {} · GPUI {}\nSource https://github.com/jihuayu/gpui-box/tree/{}",
                            entry.kind,
                            entry.delivery,
                            entry.license,
                            entry.sdk_version,
                            entry.upstream_revision,
                            view.source_revision,
                        );
                        for source in &entry.sources {
                            outputln!("Source file {} · SHA256 {}", source.path, source.sha256);
                        }
                        for dependency in &entry.dependencies {
                            outputln!("Requires {} {}", dependency.id, dependency.version);
                        }
                        for (platform, evidence) in &entry.platforms {
                            outputln!(
                                "Platform {} · {:?} · {} · Evidence {}",
                                platform,
                                evidence.status,
                                evidence.scope,
                                evidence.evidence
                            );
                        }
                    }
                }
                outputln!(
                    "Registry {} · Installation tracking unavailable",
                    view.snapshot_sha256
                );
            }
            return serde_json::to_value(view)
                .map_err(|error| CliError::new("REGISTRY_QUERY_FAILED", error.to_string()));
        }
        [command, operation, items @ ..]
            if command == "registry" && operation == "resolve" && !items.is_empty() =>
        {
            let snapshot = registry::bundled()
                .map_err(|error| CliError::new("REGISTRY_RESOLUTION_FAILED", error))?;
            let resolution = snapshot
                .resolve(items)
                .map_err(|error| CliError::new("REGISTRY_RESOLUTION_FAILED", error))?;
            if !output::is_json() {
                for entry in &resolution.entries {
                    outputln!("{} {}", entry.id, entry.version);
                }
                outputln!(
                    "Resolved {} entries · Installation applied false",
                    resolution.entries.len()
                );
            }
            return serde_json::to_value(resolution)
                .map_err(|error| CliError::new("REGISTRY_RESOLUTION_FAILED", error.to_string()));
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
        [command, operation, destination, option, source]
            if command == "sdk" && operation == "export" && option == "--source" =>
        {
            let manifest = sdk::export(&root.join(source), &root.join(destination))
                .map_err(|error| CliError::new("SDK_EXPORT_FAILED", error))?;
            outputln!(
                "Exported committed SDK {} to {}",
                manifest.source_revision,
                destination
            );
            return serde_json::to_value(manifest)
                .map_err(|error| CliError::new("SDK_EXPORT_FAILED", error.to_string()));
        }
        [command, namespace, operation, source, option, cache]
            if command == "sdk"
                && namespace == "cache"
                && operation == "add"
                && option == "--cache" =>
        {
            let receipt = sdk::cache_add(&root.join(source), &root.join(cache))
                .map_err(|error| CliError::new("SDK_CACHE_FAILED", error))?;
            outputln!(
                "Verified SDK cache {} · Created {} · {}",
                receipt.content_sha256,
                receipt.created,
                receipt.path.display()
            );
            return serde_json::to_value(receipt)
                .map_err(|error| CliError::new("SDK_CACHE_FAILED", error.to_string()));
        }
        [command, namespace, operation, address, option, cache]
            if command == "sdk"
                && namespace == "cache"
                && operation == "verify"
                && option == "--cache" =>
        {
            let receipt = sdk::cache_verify(&root.join(cache), address)
                .map_err(|error| CliError::new("SDK_CACHE_FAILED", error))?;
            outputln!(
                "Verified SDK cache {} · {}",
                receipt.content_sha256,
                receipt.path.display()
            );
            return serde_json::to_value(receipt)
                .map_err(|error| CliError::new("SDK_CACHE_FAILED", error.to_string()));
        }
        [command, operation, path] if command == "sdk" && operation == "verify" => {
            let manifest = sdk::verify(&root.join(path))
                .map_err(|error| CliError::new("SDK_VERIFICATION_FAILED", error))?;
            outputln!(
                "Verified SDK {} with {} committed files",
                manifest.source_revision,
                manifest.files.len()
            );
            return serde_json::to_value(manifest)
                .map_err(|error| CliError::new("SDK_VERIFICATION_FAILED", error.to_string()));
        }
        [command, destination, options @ ..] if matches!(command.as_str(), "new" | "init") => {
            let mut distribution = None;
            let mut targets = None;
            let mut template = None;
            for pair in options.chunks(2) {
                let [option, value] = pair else {
                    return Err(CliError::new(
                        "INVALID_ARGUMENTS",
                        "Project options require values",
                    ));
                };
                match option.as_str() {
                    "--distribution" if distribution.is_none() => distribution = Some(value),
                    "--targets" if targets.is_none() => {
                        targets = Some(value.split(',').map(str::to_owned).collect())
                    }
                    "--template" if template.is_none() => {
                        template = Some(
                            generate::Template::parse(value)
                                .map_err(|error| CliError::new("INVALID_ARGUMENTS", error))?,
                        );
                    }
                    _ => {
                        return Err(CliError::new(
                            "INVALID_ARGUMENTS",
                            "Unknown or repeated project option",
                        ));
                    }
                }
            }
            let distribution = distribution
                .ok_or_else(|| CliError::new("INVALID_ARGUMENTS", "Use --distribution PATH"))?;
            let distribution = PathBuf::from(distribution).canonicalize().map_err(|e| {
                CliError::new("DISTRIBUTION_UNAVAILABLE", format!("Distribution: {e}"))
            })?;
            let destination = root.join(destination);
            let targets = targets.unwrap_or_else(|| vec!["desktop".into(), "android".into()]);
            let template = template.unwrap_or(generate::Template::Starter);
            if command == "init" {
                generate::initialize(&destination, distribution, targets, template)
                    .map_err(|error| CliError::new("PROJECT_INITIALIZATION_FAILED", error))?;
            } else if targets == ["desktop", "android"]
                && matches!(template, generate::Template::Starter)
            {
                generate::create(&destination, distribution)
                    .map_err(|error| CliError::new("PROJECT_CREATION_FAILED", error))?;
            } else {
                generate::create_with_targets(&destination, distribution, targets, template)
                    .map_err(|error| CliError::new("PROJECT_CREATION_FAILED", error))?;
            }
            outputln!(
                "Created {}. Run mise trust and mise install, then use a configured target's build or dev command.",
                destination.display()
            );
            return Ok(
                json!({"destination": destination, "template": {"id": template.id(), "version": 1}}),
            );
        }
        [command] if command == "generate" => {
            let receipt =
                generate::apply(&root, None).map_err(|e| CliError::new("GENERATION_FAILED", e))?;
            outputln!(
                "Updated managed files. Application source and native customizations preserved."
            );
            return serde_json::to_value(receipt)
                .map_err(|error| CliError::new("GENERATION_FAILED", error.to_string()));
        }
        [command, option, path] if command == "generate" && option == "--plan" => {
            let receipt = generate::apply(&root, Some(&root.join(path)))
                .map_err(|error| CliError::new("GENERATION_FAILED", error))?;
            outputln!(
                "Applied generation plan. {} files written.",
                receipt.files_written.len()
            );
            return serde_json::to_value(receipt)
                .map_err(|error| CliError::new("GENERATION_FAILED", error.to_string()));
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
            let report = workflow::doctor(&root)
                .map_err(|error| CliError::new("DIAGNOSTICS_FAILED", error))?;
            let ready = report.ready;
            let value = serde_json::to_value(report)
                .map_err(|error| CliError::new("DIAGNOSTICS_FAILED", error.to_string()))?;
            if !ready {
                return Err(CliError::new(
                    "DIAGNOSTICS_FAILED",
                    "Missing prerequisites listed above",
                )
                .with_details(value));
            }
            return Ok(value);
        }
        [command, platform, option] if command == "build" && option == "--dry-run" => {
            let plan = workflow::build_plan(&root, platform)
                .map_err(|error| CliError::new("BUILD_PLAN_FAILED", error))?;
            outputln!(
                "Planned {} build via {} {:?}. Managed changes are included in the JSON result.",
                plan.target,
                plan.command.program,
                plan.command.arguments
            );
            return serde_json::to_value(plan)
                .map_err(|error| CliError::new("BUILD_PLAN_FAILED", error.to_string()));
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
            let inventory =
                workflow::devices().map_err(|e| CliError::new("DEVICE_QUERY_FAILED", e))?;
            return serde_json::to_value(inventory)
                .map_err(|error| CliError::new("DEVICE_QUERY_FAILED", error.to_string()));
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
    let help = reference::help();
    outputln!("{help}");
    json!({"help": help, "response_schema_version": 1, "json_streaming_commands": false})
}
