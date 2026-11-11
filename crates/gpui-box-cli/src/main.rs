mod config;
mod generate;
mod workflow;
use std::{env, path::PathBuf, process::ExitCode};

fn execute(arguments: &[String]) -> Result<(), String> {
    let root = env::current_dir().map_err(|e| e.to_string())?;
    match arguments {
        [] => help(),
        [command] if matches!(command.as_str(), "--help" | "-h" | "help") => help(),
        [command] if matches!(command.as_str(), "--version" | "-V") => {
            println!("gpui-box {}", gpui_box::VERSION)
        }
        [command] if command == "info" => {
            println!(
                "gpui-box {}\nGPUI revision {}",
                gpui_box::VERSION,
                gpui_box::UPSTREAM_REVISION
            );
            println!(
                "Host {}-{}\nDesktop targets Windows, macOS, Linux\nMobile targets Android, iOS",
                env::consts::ARCH,
                env::consts::OS
            );
        }
        [command, destination, option, distribution]
            if command == "new" && option == "--distribution" =>
        {
            let distribution = PathBuf::from(distribution)
                .canonicalize()
                .map_err(|e| format!("Distribution: {e}"))?;
            let destination = root.join(destination);
            generate::create(&destination, distribution)?;
            println!(
                "Created {}. Run mise trust, mise install, then mise run dev:desktop.",
                destination.display()
            );
        }
        [command] if command == "generate" => {
            generate::generate(&root, false)?;
            println!(
                "Updated managed files. Application source and native customizations preserved."
            );
        }
        [command] if command == "doctor" => workflow::doctor(&root)?,
        [command] if command == "devices" => workflow::devices()?,
        [command] if command == "logs" => workflow::logs(&root, None)?,
        [command, option, serial] if command == "logs" && option == "--device" => {
            workflow::logs(&root, Some(serial))?
        }
        [command, platform]
            if matches!(command.as_str(), "dev" | "build" | "install" | "package") =>
        {
            workflow::workflow(&root, command, platform, None)?
        }
        [command, platform, option, serial]
            if matches!(command.as_str(), "dev" | "build" | "install" | "package")
                && option == "--device" =>
        {
            workflow::workflow(&root, command, platform, Some(serial))?
        }
        _ => {
            return Err(
                "Unknown command or invalid arguments. Run gpui-box --help for available commands."
                    .into(),
            );
        }
    }
    Ok(())
}
fn main() -> ExitCode {
    match execute(&env::args().skip(1).collect::<Vec<_>>()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
fn help() {
    println!(
        "gpui-box - desktop and mobile developer tools\n\nUsage gpui-box COMMAND\n\n  new PATH --distribution PATH  Generate a desktop/Android app\n  generate                      Update unmodified managed files\n  doctor                        Inspect config and prerequisites\n  dev desktop|android           Run the application\n  build desktop|android         Build a development artifact\n  install android               Install on the selected device\n  package android               Assemble a debug APK\n  devices                       List Android devices\n  logs [--device SERIAL]        Follow application logs\n  info | --version | --help\n\nAndroid workflows accept --device SERIAL. Release signing and updates follow M4/M5."
    );
}
