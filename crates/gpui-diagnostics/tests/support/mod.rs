//! Shared portable test helpers. All paths are created under `tempfile::TempDir`.

#![allow(dead_code)]

use gpui_app_data::{AppData, DirectoryRoots};
use gpui_diagnostics::{
    BuildIdentity, Completion, ErrorCode, OperationContext, PACKAGE_VERSION, ServiceResult,
    SystemError, UPSTREAM_GPUI_REVISION,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn app() -> (tempfile::TempDir, AppData) {
    let root = tempfile::tempdir().expect("temporary root");
    let app = AppData::open(DirectoryRoots::under(root.path().join("app"))).expect("AppData");
    (root, app)
}

pub fn identity() -> BuildIdentity {
    BuildIdentity::for_current_build("test", "test", "diagnostics-build", "symbol-1")
}

pub fn assert_exact_identity(identity: &BuildIdentity) {
    assert_eq!(identity.package_version, PACKAGE_VERSION);
    assert_eq!(identity.package_version, "0.1.0-dev");
    assert_eq!(identity.upstream_gpui_revision, UPSTREAM_GPUI_REVISION);
    assert_eq!(
        identity.upstream_gpui_revision,
        "f8cfc7ed9e57503def9479138894f3139f5400cc"
    );
    assert!(!identity.target_triple.is_empty());
    assert_eq!(identity.profile, "test");
    assert_eq!(identity.channel, "test");
}

pub fn context() -> OperationContext {
    OperationContext::default()
}

pub fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation failed") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("operation was cancelled: {reason:?}"),
    }
}

pub fn cancelled<T>(result: ServiceResult<T>) -> gpui_diagnostics::CancelReason {
    match result.expect("cancelled operation returned an error") {
        Completion::Cancelled(reason) => reason,
        Completion::Completed(_) => panic!("operation completed instead of being cancelled"),
    }
}

pub fn failure<T>(result: ServiceResult<T>) -> SystemError {
    match result {
        Err(error) => error,
        Ok(Completion::Completed(_)) => panic!("operation completed instead of failing"),
        Ok(Completion::Cancelled(reason)) => {
            panic!("operation was cancelled instead of failing: {reason:?}")
        }
    }
}

pub fn assert_code<T>(result: ServiceResult<T>, code: ErrorCode) {
    let error = failure(result);
    assert_eq!(error.code, code, "unexpected error: {error:?}");
}

pub fn diagnostics_dir(app: &AppData) -> PathBuf {
    app.location(gpui_app_data::DataLocation::Data)
        .child("diagnostics")
        .expect("diagnostics namespace")
}

pub fn export_temp_dir(app: &AppData) -> PathBuf {
    app.location(gpui_app_data::DataLocation::Temporary)
        .path()
        .to_path_buf()
}

pub fn regular_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return files;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&entry_path) else {
            continue;
        };
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            files.push(entry_path);
        }
    }
    files
}

pub fn export_files(app: &AppData) -> Vec<PathBuf> {
    regular_files(&export_temp_dir(app))
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("export-"))
        })
        .collect()
}

pub fn canaries() -> Vec<&'static str> {
    vec![
        "CANARY_TOKEN_VALUE",
        "CANARY_SECRET_VALUE",
        "CANARY_PASSWORD_VALUE",
        "CANARY_AUTH_VALUE",
        "CANARY_COOKIE_VALUE",
        "CANARY_CREDENTIAL_VALUE",
        "CANARY_PRIVATE_KEY_VALUE",
        "CANARY_API_KEY_VALUE",
        "CANARY_SESSION_VALUE",
        "CANARY_UNIX_PATH",
        "CANARY_WIN_PATH",
        "CANARY_UNC_PATH",
        "CANARY_EXTENDED_PATH",
        "CANARY_VAR_PATH",
        "CANARY_TMP_PATH",
        "CANARY_DRIVE_PATH",
        "CANARY_BEARER_VALUE",
        "CANARY_BASIC_VALUE",
        "CANARY_JWT_PAYLOAD",
        "CANARY_JWT_SIGNATURE",
        "CANARY_URL_PASSWORD",
        "CANARY_URL_QUERY",
        "CANARY_QUERY_TOKEN",
        "CANARY_QUERY_SECRET",
        "CANARY_NESTED_API",
        "CANARY_NESTED_BEARER",
        "CANARY_NESTED_PATH",
        "CANARY_LIST_PATH",
        "CANARY_LIST_BEARER",
        "CANARY_PANIC_MESSAGE",
        "CANARY_PANIC_FRAME",
        "CANARY_ANR_REASON",
        "CANARY_ANR_FRAME",
        "CANARY_CRASH_DETAIL",
        "CANARY_OS_REASON",
    ]
}

pub fn assert_no_canaries(text: &str, context: &str) {
    for canary in canaries() {
        assert!(
            !text.contains(canary),
            "{context} leaked unredacted canary {canary}: {text}"
        );
    }
}

pub fn next_event<T>(
    subscription: &gpui_diagnostics::EventSubscription<T>,
) -> gpui_diagnostics::EventEnvelope<T> {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let mut next = subscription.try_next().expect("single event reader");
    match Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Some(envelope)) => envelope,
        Poll::Ready(None) => panic!("event stream closed"),
        Poll::Pending => panic!("event did not arrive"),
    }
}

pub fn assert_no_event<T>(subscription: &gpui_diagnostics::EventSubscription<T>) {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let mut next = subscription.try_next().expect("single event reader");
    match Pin::new(&mut next).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Pending => {}
        Poll::Ready(Some(_)) => panic!("received an unexpected service event"),
        Poll::Ready(None) => panic!("event stream closed unexpectedly"),
    }
}
