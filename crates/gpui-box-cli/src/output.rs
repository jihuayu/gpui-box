use serde::Serialize;
use std::{
    fmt,
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

static JSON: AtomicBool = AtomicBool::new(false);

pub fn set_json(enabled: bool) {
    JSON.store(enabled, Ordering::Relaxed);
}

pub fn is_json() -> bool {
    JSON.load(Ordering::Relaxed)
}

/// Machine-readable mode reserves stdout for one final schema-versioned value.
pub fn message(arguments: fmt::Arguments<'_>) {
    if is_json() {
        eprintln!("{arguments}");
    } else {
        println!("{arguments}");
    }
}

#[derive(Serialize)]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
}

impl CliError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
pub struct Response<'a> {
    pub schema_version: u32,
    pub cli_version: &'static str,
    pub upstream_revision: &'static str,
    pub command: &'a str,
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<CliError>,
}

pub fn respond(command: &str, result: Result<serde_json::Value, CliError>) -> io::Result<()> {
    let (success, result, error) = match result {
        Ok(result) => (true, Some(result), None),
        Err(error) => (false, None, Some(error)),
    };
    let response = Response {
        schema_version: 1,
        cli_version: gpui_box::VERSION,
        upstream_revision: gpui_box::UPSTREAM_REVISION,
        command,
        success,
        result,
        error,
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &response)?;
    writeln!(stdout)
}
