use std::process::Command;

#[test]
fn info_reports_the_distribution_and_upstream_revision() {
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .arg("info")
        .output()
        .expect("CLI should start");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("CLI output should be UTF-8");
    assert!(stdout.contains(gpui_box::VERSION));
    assert!(stdout.contains(gpui_box::UPSTREAM_REVISION));
    assert!(stdout.contains("Windows, macOS, Linux"));
    assert!(stdout.contains("Android, iOS"));
}

#[test]
fn unsupported_commands_fail_with_help_guidance() {
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .arg("unknown")
        .output()
        .expect("CLI should start");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--help"));
}

fn json_response(output: &std::process::Output, success: bool) -> serde_json::Value {
    assert_eq!(output.status.success(), success);
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "Invalid JSON response {error}: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["cli_version"], gpui_box::VERSION);
    assert_eq!(response["upstream_revision"], gpui_box::UPSTREAM_REVISION);
    assert_eq!(response["success"], success);
    assert_eq!(response["result"].is_null(), !success);
    assert_eq!(response["error"].is_null(), success);
    response
}

#[test]
fn json_info_help_and_version_have_one_versioned_stdout_value() {
    for arguments in [
        vec!["--json", "info"],
        vec!["--help", "--json"],
        vec!["--version", "--json"],
        vec!["--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .args(&arguments)
            .output()
            .unwrap();
        let response = json_response(&output, true);
        match response["command"].as_str().unwrap() {
            "info" => {
                assert_eq!(response["result"]["host"]["os"], std::env::consts::OS);
                assert_eq!(
                    response["result"]["desktop_targets"],
                    serde_json::json!(["windows", "macos", "linux"])
                );
            }
            "--version" => assert_eq!(response["result"]["version"], gpui_box::VERSION),
            _ => assert!(
                response["result"]["help"]
                    .as_str()
                    .unwrap()
                    .contains("build desktop|android")
            ),
        }
    }
}

#[test]
fn json_argument_failures_have_stable_codes_and_exit_status() {
    for arguments in [
        vec!["unknown", "--json"],
        vec!["--json", "info", "--json"],
        vec!["build", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .args(arguments)
            .output()
            .unwrap();
        let response = json_response(&output, false);
        assert_eq!(response["error"]["code"], "INVALID_ARGUMENTS");
        assert!(response["error"]["message"].as_str().unwrap().len() > 4);
    }
}

#[test]
fn json_streaming_commands_fail_before_starting_or_mutating_a_project() {
    let directory = tempfile::tempdir().unwrap();
    for arguments in [vec!["dev", "android", "--json"], vec!["logs", "--json"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        let response = json_response(&output, false);
        assert_eq!(response["error"]["code"], "STREAMING_JSON_UNSUPPORTED");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn json_project_generation_handles_spaces_customizations_and_errors() {
    let directory = tempfile::tempdir().unwrap();
    let distribution = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let destination = directory.path().join("JSON App 有 空格");
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .args([
            "--json",
            "new",
            destination.to_str().unwrap(),
            "--distribution",
            distribution.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let response = json_response(&output, true);
    assert_eq!(
        response["result"]["destination"],
        destination.to_str().unwrap()
    );
    let app_source = destination.join("app/src/lib.rs");
    std::fs::write(&app_source, "// application-owned\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&destination)
        .args(["generate", "--json"])
        .output()
        .unwrap();
    assert_eq!(json_response(&output, true)["result"]["completed"], true);
    assert_eq!(
        std::fs::read_to_string(app_source).unwrap(),
        "// application-owned\n"
    );
    std::fs::write(destination.join("gpui-box.json"), b"invalid project").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(&destination)
        .args(["generate", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        json_response(&output, false)["error"]["code"],
        "GENERATION_FAILED"
    );
    assert_eq!(
        std::fs::read(destination.join("gpui-box.json")).unwrap(),
        b"invalid project"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .args([
            "new",
            destination.to_str().unwrap(),
            "--distribution",
            distribution.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        json_response(&output, false)["error"]["code"],
        "PROJECT_CREATION_FAILED"
    );
}

#[test]
fn json_finite_command_failures_are_machine_readable() {
    let directory = tempfile::tempdir().unwrap();
    for (arguments, code) in [
        (vec!["doctor", "--json"], "DIAGNOSTICS_FAILED"),
        (vec!["build", "android", "--json"], "WORKFLOW_FAILED"),
        (vec!["install", "android", "--json"], "WORKFLOW_FAILED"),
        (
            vec!["package", "android", "--release", "--json"],
            "PACKAGING_FAILED",
        ),
        (
            vec!["package", "desktop", "--portable", "--json"],
            "PACKAGING_FAILED",
        ),
        (
            vec![
                "new",
                "App",
                "--distribution",
                "missing-distribution",
                "--json",
            ],
            "DISTRIBUTION_UNAVAILABLE",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(json_response(&output, false)["error"]["code"], code);
    }
}

#[test]
fn generated_reference_matches_markdown_and_help_from_any_directory() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .arg("reference")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let expected =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/CLI_REFERENCE.md");
    let markdown = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        markdown.replace("\r\n", "\n"),
        std::fs::read_to_string(expected)
            .unwrap()
            .replace("\r\n", "\n")
    );
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .args(["reference", "--json"])
        .output()
        .unwrap();
    let response = json_response(&output, true);
    assert_eq!(response["command"], "reference");
    let contract = &response["result"];
    assert_eq!(contract["schema_version"], 1);
    assert_eq!(contract["response_schema_version"], 1);
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .args(["--help", "--json"])
        .output()
        .unwrap();
    let help = json_response(&output, true)["result"]["help"]
        .as_str()
        .unwrap()
        .to_string();
    let mut syntaxes = std::collections::BTreeSet::new();
    for command in contract["commands"].as_array().unwrap() {
        let syntax = command["syntax"].as_str().unwrap();
        assert!(syntaxes.insert(syntax));
        assert!(help.contains(syntax));
        assert!(markdown.contains(&format!("### `{syntax}`")));
        for field in ["summary", "result", "effects", "offline"] {
            assert!(!command[field].as_str().unwrap().is_empty());
        }
        let streaming = syntax.starts_with("dev ") || syntax.starts_with("logs ");
        assert_eq!(command["json"], !streaming);
    }
    assert_eq!(syntaxes.len(), 27);
    let mut codes = std::collections::BTreeSet::new();
    for error in contract["errors"].as_array().unwrap() {
        let code = error["code"].as_str().unwrap();
        assert!(codes.insert(code));
        assert!(markdown.contains(code));
    }
    assert_eq!(codes.len(), 22);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

fn legacy_project_configuration() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "name": "Configuration 有 空格",
        "application_id": "dev.example.config",
        "version": "0.1.0",
        "version_code": 1,
        "distribution": "missing SDK 有 空格",
        "targets": ["desktop"],
        "update_channel": "development"
    })
}

#[test]
fn configuration_schema_exports_the_shipped_contract_without_a_project_or_tools() {
    let directory = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .env("PATH", "")
        .args(["config", "schema", "--json"])
        .output()
        .unwrap();
    let schema = json_response(&output, true)["result"].clone();
    let shipped: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schemas/gpui-box.project.v1.schema.json"
    ))
    .unwrap();
    assert_eq!(schema, shipped);
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["schema_version"]["const"], 1);
    assert_eq!(
        schema["properties"]["assets"]["default"],
        serde_json::json!(["assets"])
    );
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .args(["config", "schema"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        schema
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn configuration_validation_reports_raw_identity_and_legacy_defaults_without_writes() {
    use sha2::{Digest, Sha256};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gpui-box.json");
    let bytes = serde_json::to_vec_pretty(&legacy_project_configuration()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    // Future overlay/lock names have no precedence over the canonical project.
    for unrelated in ["gpui-box.local.json", "gpui-box.toml", "gpui-box.lock"] {
        std::fs::write(directory.path().join(unrelated), b"unrelated original").unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .env("PATH", "")
        .args(["config", "validate", "--json"])
        .output()
        .unwrap();
    let report = json_response(&output, true)["result"].clone();
    assert_eq!(report["valid"], true);
    assert_eq!(report["runtime_executed"], false);
    assert_eq!(
        report["configuration_sha256"],
        format!("{:x}", Sha256::digest(&bytes))
    );
    assert_eq!(report["configuration_bytes"], bytes.len());
    assert_eq!(
        report["normalized_configuration"]["assets"],
        serde_json::json!(["assets"])
    );
    assert_eq!(
        report["normalized_configuration"]["services"]["schema_version"],
        1
    );
    assert_eq!(
        report["normalized_configuration"]["services"]["permissions"],
        serde_json::json!([])
    );
    assert_eq!(report["local_overrides"], "unsupported");
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 4);
    for unrelated in ["gpui-box.local.json", "gpui-box.toml", "gpui-box.lock"] {
        assert_eq!(
            std::fs::read(directory.path().join(unrelated)).unwrap(),
            b"unrelated original"
        );
    }
}

#[test]
fn configuration_validation_and_generation_share_semantic_failures_and_preserve_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gpui-box.json");
    let mut invalid = Vec::new();
    for (key, value) in [
        ("schema_version", serde_json::json!(2)),
        ("unknown", serde_json::json!(true)),
        ("targets", serde_json::json!(["android", "android"])),
        ("assets", serde_json::json!(["../outside"])),
        ("version", serde_json::json!("4294967296.0.0")),
        ("services", serde_json::json!({"url_schemes": ["https"]})),
        (
            "services",
            serde_json::json!({"permissions": [{"kind":"camera", "purpose":"Capture a photo"}]}),
        ),
    ] {
        let mut config = legacy_project_configuration();
        config[key] = value;
        invalid.push(serde_json::to_vec(&config).unwrap());
    }
    invalid.extend([b"{invalid".to_vec(), vec![b' '; 1024 * 1024 + 1]]);
    for bytes in invalid {
        std::fs::write(&path, &bytes).unwrap();
        let mut messages = Vec::new();
        for arguments in [
            vec!["config", "validate", "--json"],
            vec!["generate", "--dry-run", "--json"],
        ] {
            let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
                .current_dir(directory.path())
                .env("PATH", "")
                .args(&arguments)
                .output()
                .unwrap();
            let response = json_response(&output, false);
            if arguments[0] == "config" {
                assert_eq!(response["error"]["code"], "CONFIG_VALIDATION_FAILED");
            }
            messages.push(response["error"]["message"].clone());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
        assert_eq!(messages[0], messages[1]);
    }
}

#[test]
fn configuration_validation_reports_missing_files_and_finite_syntax_errors() {
    let directory = tempfile::tempdir().unwrap();
    for (arguments, code) in [
        (
            vec!["config", "validate", "--json"],
            "CONFIG_VALIDATION_FAILED",
        ),
        (
            vec!["config", "schema", "extra", "--json"],
            "INVALID_ARGUMENTS",
        ),
        (
            vec!["config", "validate", "--write", "--json"],
            "INVALID_ARGUMENTS",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(json_response(&output, false)["error"]["code"], code);
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn configuration_validation_accepts_the_size_boundary_and_preserves_explicit_native_values() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = legacy_project_configuration();
    config["targets"] = serde_json::json!(["android"]);
    config["assets"] = serde_json::json!([]);
    config["services"] = serde_json::json!({
        "schema_version": 1,
        "permissions": [{"kind": "camera", "purpose": "Capture a selected photo"}],
        "url_schemes": ["example-config"]
    });
    let mut bytes = serde_json::to_vec(&config).unwrap();
    bytes.resize(1024 * 1024, b' ');
    let path = directory.path().join("gpui-box.json");
    std::fs::write(&path, &bytes).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
        .current_dir(directory.path())
        .args(["config", "validate", "--json"])
        .output()
        .unwrap();
    let report = json_response(&output, true)["result"].clone();
    assert_eq!(report["configuration_bytes"], 1024 * 1024);
    assert_eq!(
        report["normalized_configuration"]["assets"],
        config["assets"]
    );
    assert_eq!(
        report["normalized_configuration"]["services"],
        config["services"]
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn reference_invalid_arguments_are_finite_and_read_only() {
    let directory = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["reference", "unexpected", "--json"],
        vec!["reference", "--json", "--json"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(
            json_response(&output, false)["error"]["code"],
            "INVALID_ARGUMENTS"
        );
    }
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn registry_discovery_and_resolution_are_read_only_from_an_empty_directory() {
    let directory = tempfile::tempdir().unwrap();
    let invoke = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap()
    };
    let response = json_response(&invoke(&["list", "--json"]), true);
    assert_eq!(response["result"]["schema_version"], 1);
    assert_eq!(response["result"]["installation_tracking"], "unavailable");
    let entries = response["result"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 12);
    let ids = entries
        .iter()
        .map(|entry| entry["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    let details = json_response(
        &invoke(&["info", "official/template/workbench", "--json"]),
        true,
    );
    assert_eq!(details["result"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(details["result"]["entries"][0]["delivery"], "source");
    let first = json_response(
        &invoke(&[
            "registry",
            "resolve",
            "official/template/workbench",
            "official/control/button",
            "--json",
        ]),
        true,
    );
    let reverse = json_response(
        &invoke(&[
            "registry",
            "resolve",
            "official/control/button",
            "official/template/workbench",
            "official/control/button",
            "--json",
        ]),
        true,
    );
    assert_eq!(first, reverse);
    assert_eq!(first["result"]["installation_applied"], false);
    assert_eq!(
        first["result"]["snapshot_sha256"],
        response["result"]["snapshot_sha256"]
    );
    assert_eq!(
        json_response(&invoke(&["info", "--json"]), true)["result"]["version"],
        gpui_box::VERSION
    );
    for (arguments, code) in [
        (
            vec!["info", "official/control/unknown", "--json"],
            "REGISTRY_QUERY_FAILED",
        ),
        (
            vec!["registry", "resolve", "official/control/unknown", "--json"],
            "REGISTRY_RESOLUTION_FAILED",
        ),
        (vec!["registry", "resolve", "--json"], "INVALID_ARGUMENTS"),
        (vec!["list", "extra", "--json"], "INVALID_ARGUMENTS"),
    ] {
        assert_eq!(
            json_response(&invoke(&arguments), false)["error"]["code"],
            code
        );
    }
    let output = invoke(&["list"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("official/control/button")
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn platform_inventory_distinguishes_host_selection_and_unprobed_tools_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let invoke = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_gpui-box"))
            .current_dir(directory.path())
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let response = json_response(&invoke(&["platform", "list", "--json"]), true);
    let result = &response["result"];
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["project_present"], false);
    let platforms = result["platforms"].as_array().unwrap();
    assert_eq!(platforms.len(), 6);
    assert!(
        platforms
            .iter()
            .all(|platform| platform["toolchain_readiness"] == "unprobed"
                && platform["project_enabled"].is_null())
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let config = serde_json::json!({
        "schema_version": 1, "name": "Policy test", "application_id": "dev.example.policy",
        "version": "0.1.0", "version_code": 1, "distribution": source,
        "targets": ["android"], "update_channel": "development", "permissions": [], "assets": ["assets"],
    });
    let bytes = serde_json::to_vec(&config).unwrap();
    let path = directory.path().join("gpui-box.json");
    std::fs::write(&path, &bytes).unwrap();
    let response = json_response(&invoke(&["platform", "list", "--json"]), true);
    assert_eq!(response["result"]["project_present"], true);
    for platform in response["result"]["platforms"].as_array().unwrap() {
        assert_eq!(platform["project_enabled"], platform["id"] == "android");
        assert_eq!(platform["toolchain_readiness"], "unprobed");
    }
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    std::fs::write(&path, b"invalid").unwrap();
    assert_eq!(
        json_response(&invoke(&["platform", "list", "--json"]), false)["error"]["code"],
        "PLATFORM_QUERY_FAILED"
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"invalid");
    for args in [
        vec!["platform", "add", "android", "extra", "--json"],
        vec!["platform", "list", "extra", "--json"],
    ] {
        assert_eq!(
            json_response(&invoke(&args), false)["error"]["code"],
            "INVALID_ARGUMENTS"
        );
    }
}
