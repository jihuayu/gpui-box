use serde::Serialize;

#[derive(Serialize)]
pub struct CommandReference {
    pub syntax: &'static str,
    pub summary: &'static str,
    pub result: &'static str,
    pub effects: &'static str,
    pub offline: &'static str,
    pub json: bool,
}

#[derive(Serialize)]
pub struct Reference {
    pub schema_version: u32,
    pub response_schema_version: u32,
    pub commands: &'static [CommandReference],
    pub notes: &'static [&'static str],
    pub errors: &'static [ErrorReference],
}

#[derive(Serialize)]
pub struct ErrorReference {
    pub code: &'static str,
    pub meaning: &'static str,
}

const COMMANDS: &[CommandReference] = &[
    CommandReference {
        syntax: "new PATH --distribution PATH [--targets desktop,android] [--template starter|adaptive|component]",
        summary: "Create a version-1 application in a new directory",
        result: "Destination and template identity",
        effects: "Publish a staged project. A failed staging tree is retained",
        offline: "Local source checkout or verified exported SDK required",
        json: true,
    },
    CommandReference {
        syntax: "init PATH --distribution PATH [--targets desktop,android] [--template starter|adaptive|component]",
        summary: "Initialize an existing directory with ownership conflict checks",
        result: "Destination and template identity",
        effects: "Lock, stage and publish files. Configuration is published last",
        offline: "Local SDK and filesystem hard-link support required",
        json: true,
    },
    CommandReference {
        syntax: "sdk export PATH --source CHECKOUT",
        summary: "Export the committed source closure with a SHA-256 manifest",
        result: "SdkManifest v1",
        effects: "Run local Git, stage a sibling tree and publish a fresh destination",
        offline: "Committed local Git checkout and Git executable required",
        json: true,
    },
    CommandReference {
        syntax: "sdk verify PATH",
        summary: "Verify SDK paths, digests and upstream provenance",
        result: "SdkManifest v1",
        effects: "Read SDK files",
        offline: "Local SDK content required",
        json: true,
    },
    CommandReference {
        syntax: "generate [--plan PATH]",
        summary: "Apply managed generation with optional saved-plan preconditions",
        result: "ApplyReceipt v1",
        effects: "Use the generation lock and journal, preserve application-owned files",
        offline: "Read local config/SDK and optional bare ChangePlan JSON",
        json: true,
    },
    CommandReference {
        syntax: "generate --dry-run",
        summary: "Preview exact managed changes and their hashes",
        result: "ChangePlan v1",
        effects: "Read files without locks, mutation or child processes",
        offline: "Read local config/SDK and ownership metadata",
        json: true,
    },
    CommandReference {
        syntax: "doctor",
        summary: "Report declared prerequisites and corrective actions",
        result: "DoctorReport v1 or error.details report",
        effects: "Read config/assets and query installed tool versions",
        offline: "Tool subprocess behavior follows installed wrappers/configuration",
        json: true,
    },
    CommandReference {
        syntax: "build desktop|android [--device SERIAL]",
        summary: "Build the configured development target",
        result: "Completion acknowledgement",
        effects: "Regenerate managed files, run mise/Cargo or verified Gradle Wrapper",
        offline: "Child tools may download dependencies or tools. Existing caches may suffice",
        json: true,
    },
    CommandReference {
        syntax: "build desktop|android --dry-run",
        summary: "Preview generation and the build argument vector",
        result: "BuildPlan v1",
        effects: "Read files without mutation, tool processes or network requests",
        offline: "Local config/SDK required. Build tools may be absent",
        json: true,
    },
    CommandReference {
        syntax: "dev desktop|android [--device SERIAL]",
        summary: "Run desktop or install and cold-start Android",
        result: "Human workflow output",
        effects: "Regenerate, build and run/install. Android starts ActivityManager",
        offline: "Child builds may download. Android requires available adb device",
        json: false,
    },
    CommandReference {
        syntax: "install android [--device SERIAL]",
        summary: "Build and install a debug APK",
        result: "Completion acknowledgement",
        effects: "Regenerate, build and install on a selected Android device",
        offline: "Child builds may download. adb and available device required",
        json: true,
    },
    CommandReference {
        syntax: "package android [--device SERIAL]",
        summary: "Build a development APK",
        result: "Completion acknowledgement",
        effects: "Regenerate managed files and run assembleDebug",
        offline: "Child tools follow inherited dependency/tool resolution",
        json: true,
    },
    CommandReference {
        syntax: "package android --release",
        summary: "Build, sign and inspect APK/AAB artifacts",
        result: "Completion acknowledgement",
        effects: "Regenerate, run release tools and publish dist/android metadata",
        offline: "Explicit signing and checksum-pinned bundletool required. Builds may download",
        json: true,
    },
    CommandReference {
        syntax: "package desktop --portable",
        summary: "Build a Windows development-profile portable bundle",
        result: "Completion acknowledgement",
        effects: "Regenerate/build and publish ZIP, digests and license inventory",
        offline: "Windows tools and assets required. Builds may download",
        json: true,
    },
    CommandReference {
        syntax: "devices",
        summary: "List Android devices with native availability states",
        result: "DeviceInventory v1",
        effects: "Run adb devices -l. adb may start its shared server",
        offline: "Installed adb required. Existing network-connected adb transports are included",
        json: true,
    },
    CommandReference {
        syntax: "logs [--device SERIAL]",
        summary: "Follow Android application logs",
        result: "Human streaming output",
        effects: "Run adb logcat against a selected device",
        offline: "Installed adb and available device required",
        json: false,
    },
    CommandReference {
        syntax: "info",
        summary: "Report CLI/upstream identity and host labels",
        result: "Version, upstream revision, host and target-family labels",
        effects: "Write command output",
        offline: "Built-in metadata",
        json: true,
    },
    CommandReference {
        syntax: "reference",
        summary: "Render this command inventory and machine-readable contract",
        result: "CommandReference v1",
        effects: "Write command output",
        offline: "Built-in metadata",
        json: true,
    },
    CommandReference {
        syntax: "help | --help | -h",
        summary: "Show current syntax and compatibility notes",
        result: "Help text and response schema identity",
        effects: "Write command output",
        offline: "Built-in metadata",
        json: true,
    },
    CommandReference {
        syntax: "--version | -V",
        summary: "Show the distribution version",
        result: "Version string",
        effects: "Write command output",
        offline: "Built-in metadata",
        json: true,
    },
];

const NOTES: &[&str] = &[
    "Append --json once anywhere in a finite command. Stdout contains one schema-v1 envelope with cli_version, upstream_revision, command, success, result and error. Human diagnostics and child output use stderr. Exit status is zero on success and nonzero on failure. error has code/message and optional details. dev/logs reject JSON before starting work.",
    "new/init default to starter-v1 and both desktop/android targets. --targets also accepts desktop or android alone. Options take values and repeated options fail. Relative paths resolve against the current working directory. Project workflows read gpui-box.json there.",
    "generate --plan accepts the bare result object from generate --dry-run --json, bounded to1MiB. Apply recomputes current inputs under its lock. Conflicts retain current files and recovery state. Saved plans contain snapshot preconditions.",
    "Android min/native/target API is35 and compileSdk is36. --device SERIAL applies to Android device selection for dev/install/logs. Build/package carry the Android serial environment without installing. Desktop workflows build/run on the current host. info target-family labels describe project scope. Operation qualification is documented separately.",
    "Commands use noninteractive defaults. An ambiguous Android device selection fails with guidance. The CLI has no global offline option. Read-only plans and source SDK operations work from local content. Builds follow inherited Cargo/mise/Gradle resolution and cache policies. Signing inputs stay in the documented GPUI_BOX_ANDROID_* variables and GPUI_BOX_BUNDLETOOL.",
    "SDK SHA-256 verification checks integrity against its supplied manifest. Use a trusted source and delivery channel for authenticity. Available original license texts and unresolved source reviews are tracked in tooling/licenses. Native CLI publication and authenticated SDK download remain roadmap gates.",
    "Build completion, package inspection, device installation and interactive runtime are separate observations. Current generated channels use development. Release-channel provisioning, updates, native installers and broader platform qualification follow their own milestone gates. Physical-device testing remains deferred.",
    "Recovery and onboarding are documented in docs/M3_DEVELOPER_WORKFLOW.md. Packaging/signing use docs/M4_ANDROID_PACKAGING.md. External template CI records CLI/SDK/source/upstream versions, host/targets, commands and artifact digests independently from runtime evidence.",
];

const ERRORS: &[ErrorReference] = &[
    ErrorReference {
        code: "REFERENCE_FAILED",
        meaning: "Built-in command reference serialization failed",
    },
    ErrorReference {
        code: "INVALID_ARGUMENTS",
        meaning: "Unknown command, missing/invalid/repeated option or repeated --json",
    },
    ErrorReference {
        code: "CURRENT_DIRECTORY_UNAVAILABLE",
        meaning: "The current working directory cannot be resolved",
    },
    ErrorReference {
        code: "STREAMING_JSON_UNSUPPORTED",
        meaning: "dev/logs requested a finite JSON response",
    },
    ErrorReference {
        code: "DISTRIBUTION_UNAVAILABLE",
        meaning: "The new/init distribution path cannot be opened",
    },
    ErrorReference {
        code: "PROJECT_CREATION_FAILED",
        meaning: "new failed validation, staging or publication",
    },
    ErrorReference {
        code: "PROJECT_INITIALIZATION_FAILED",
        meaning: "init failed validation, ownership, staging or publication",
    },
    ErrorReference {
        code: "SDK_EXPORT_FAILED",
        meaning: "Committed source closure could not be exported",
    },
    ErrorReference {
        code: "SDK_VERIFICATION_FAILED",
        meaning: "SDK manifest, paths, source identity or digests failed verification",
    },
    ErrorReference {
        code: "GENERATION_FAILED",
        meaning: "Generation/apply failed validation, snapshot checks, ownership or file operations",
    },
    ErrorReference {
        code: "GENERATION_PLAN_FAILED",
        meaning: "Read-only generation preview failed",
    },
    ErrorReference {
        code: "BUILD_PLAN_FAILED",
        meaning: "Read-only build preview failed or target is disabled",
    },
    ErrorReference {
        code: "DIAGNOSTICS_FAILED",
        meaning: "Doctor failed to inspect or required checks failed. Completed reports use error.details",
    },
    ErrorReference {
        code: "DEVICE_QUERY_FAILED",
        meaning: "adb device inventory failed or returned invalid output",
    },
    ErrorReference {
        code: "WORKFLOW_FAILED",
        meaning: "Build/dev/install/debug-package workflow failed",
    },
    ErrorReference {
        code: "PACKAGING_FAILED",
        meaning: "Signed Android or Windows portable packaging failed",
    },
    ErrorReference {
        code: "LOG_STREAM_FAILED",
        meaning: "Human adb log stream failed",
    },
];

pub fn contract() -> Reference {
    Reference {
        schema_version: 1,
        response_schema_version: 1,
        commands: COMMANDS,
        notes: NOTES,
        errors: ERRORS,
    }
}

pub fn help() -> String {
    let mut output =
        "gpui-box - desktop and mobile developer tools\n\nUsage gpui-box COMMAND\n\n".to_string();
    for command in COMMANDS {
        output.push_str(&format!("  {}\n    {}\n", command.syntax, command.summary));
    }
    output.push_str("\nUse --json once for finite commands. See gpui-box reference for response, side-effect, offline and recovery contracts.\n");
    output
}

pub fn markdown() -> String {
    let mut output = "# gpui-box CLI reference\n\nGenerated by `gpui-box reference` from the versioned command inventory.\n\n## Commands\n".to_string();
    for command in COMMANDS {
        output.push_str(&format!("\n### `{}`\n\n{}.\n\n| Contract | Behavior |\n| --- | --- |\n| Result | {} |\n| Effects | {} |\n| Offline | {} |\n| JSON | {} |\n", command.syntax, command.summary, command.result, command.effects, command.offline, if command.json { "Supported" } else { "Unavailable for streaming workflows" }));
    }
    output.push_str("\n## Compatibility and execution contract\n");
    for note in NOTES {
        output.push_str(&format!("\n{note}\n"));
    }
    output.push_str("\n## Stable error codes\n\n| Code | Meaning |\n| --- | --- |\n");
    for error in ERRORS {
        output.push_str(&format!("| `{}` | {} |\n", error.code, error.meaning));
    }
    output
}
