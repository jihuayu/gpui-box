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
