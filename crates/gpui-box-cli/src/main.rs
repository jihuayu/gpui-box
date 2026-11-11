use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args().skip(1).collect();
    match arguments.as_slice() {
        [] => print_help(),
        [argument] if matches!(argument.as_str(), "help" | "--help" | "-h") => print_help(),
        [argument] if matches!(argument.as_str(), "--version" | "-V") => {
            println!("gpui-box {}", gpui_box::VERSION);
        }
        [argument] if argument == "info" => {
            println!("gpui-box {}", gpui_box::VERSION);
            println!("GPUI revision {}", gpui_box::UPSTREAM_REVISION);
            println!("Host {}-{}", env::consts::ARCH, env::consts::OS);
            println!("Desktop targets Windows, macOS, Linux");
            println!("Mobile targets Android, iOS");
            println!("Desktop example mise run dev:desktop");
        }
        _ => {
            eprintln!("Unknown command. Run gpui-box --help for available commands.");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn print_help() {
    println!("gpui-box - GPUI distribution tools for desktop and mobile");
    println!();
    println!("Usage gpui-box [COMMAND]");
    println!();
    println!("Commands");
    println!("  info       Show distribution version, source revision, and host");
    println!("  --version  Show the CLI version");
    println!("  --help     Show this help");
}
