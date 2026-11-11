use gpui_app_data::{AppData, DataLocation, DataScope, DirectoryRoots};
use gpui_system::{Completion, ErrorCode, OperationContext, ServiceResult};
use serde_json::{Value, json};
use std::{fs, process::Command, time::Duration};
fn value<T>(result: ServiceResult<T>) -> T {
    match result.unwrap() {
        Completion::Completed(value) => value,
        _ => panic!("unexpected cancellation"),
    }
}
fn setup() -> (tempfile::TempDir, DirectoryRoots, AppData) {
    let root = tempfile::tempdir().unwrap();
    let roots = DirectoryRoots::under(root.path().join("有 空格"));
    let app = AppData::open(roots.clone()).unwrap();
    (root, roots, app)
}
#[test]
fn relaunch_migrate_and_rollback() {
    let (_root, roots, app) = setup();
    let ctx = OperationContext::default();
    let store = app
        .settings(DataScope::Application, "preferences", 1, 4096)
        .unwrap();
    value(store.save(&json!({"n":1}), &ctx));
    drop(store);
    drop(app);
    let app = AppData::open(roots).unwrap();
    let old = app
        .settings(DataScope::Application, "preferences", 1, 4096)
        .unwrap();
    assert_eq!(value(old.load::<Value>(&ctx)), Some(json!({"n":1})));
    let new = app
        .settings(DataScope::Application, "preferences", 3, 4096)
        .unwrap();
    assert_eq!(
        new.load::<Value>(&ctx).unwrap_err().code,
        ErrorCode::InvalidConfiguration
    );
    assert!(
        new.migrate(&ctx, |version, mut value| {
            if version == 2 {
                Err(gpui_system::SystemError::new(
                    ErrorCode::InvalidInput,
                    "migration rejected",
                ))
            } else {
                value["n"] = json!(2);
                Ok(value)
            }
        })
        .is_err()
    );
    assert_eq!(value(old.load::<Value>(&ctx)), Some(json!({"n":1})));
    value(new.migrate(&ctx, |_, mut value| {
        value["n"] = json!(value["n"].as_u64().unwrap() + 1);
        Ok(value)
    }));
    assert_eq!(value(new.load::<Value>(&ctx)), Some(json!({"n":3})));
}
#[test]
fn corrupt_size_schema_and_quarantine() {
    let (_root, _roots, app) = setup();
    let ctx = OperationContext::default();
    let store = app
        .settings(DataScope::Application, "prefs", 1, 128)
        .unwrap();
    value(store.save(&5, &ctx));
    let path = app
        .location(DataLocation::Config)
        .path()
        .join("application/settings-prefs.json");
    fs::write(&path, b"{corrupt").unwrap();
    assert_eq!(
        store.load::<u32>(&ctx).unwrap_err().code,
        ErrorCode::IntegrityFailure
    );
    assert!(store.save(&6, &ctx).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"{corrupt");
    value(store.quarantine(&ctx));
    assert!(!path.exists());
    value(store.save(&6, &ctx));
    assert_eq!(value(store.load::<u32>(&ctx)), Some(6));
    assert!(store.save(&"x".repeat(200), &ctx).is_err());
    assert_eq!(value(store.load::<u32>(&ctx)), Some(6));
    fs::write(&path, br#"{"format":1,"schema":0,"payload":6}"#).unwrap();
    assert!(store.load::<u32>(&ctx).is_err());
}
#[test]
fn account_cache_and_export_isolation() {
    let (_root, roots, app) = setup();
    let ctx = OperationContext::default();
    let a = app
        .settings(DataScope::Account("a".into()), "prefs", 1, 512)
        .unwrap();
    let b = app
        .settings(DataScope::Account("b".into()), "prefs", 1, 512)
        .unwrap();
    value(a.save(&1, &ctx));
    value(b.save(&2, &ctx));
    let export = value(app.temporary_export(b"hello", 100, &ctx));
    let path = export.path().to_owned();
    fs::write(
        app.location(DataLocation::Cache).path().join("cached"),
        "cache",
    )
    .unwrap();
    assert_eq!(value(app.clear_cache(&ctx)), 1);
    assert!(path.exists());
    assert_eq!(value(app.cleanup_exports(Duration::ZERO, &ctx)), 0);
    value(app.remove_account("a", &ctx));
    assert_eq!(value(a.load::<u32>(&ctx)), None);
    assert_eq!(value(b.load::<u32>(&ctx)), Some(2));
    drop(export);
    assert_eq!(value(app.cleanup_exports(Duration::ZERO, &ctx)), 1);
    let export = value(app.temporary_export(b"restart", 100, &ctx));
    let path = export.path().to_owned();
    drop(export);
    drop(a);
    drop(b);
    drop(app);
    let app = AppData::open(roots).unwrap();
    assert_eq!(value(app.cleanup_exports(Duration::ZERO, &ctx)), 1);
    assert!(!path.exists());
}
#[test]
fn reject_overlap_escape_and_cancellation() {
    let (_root, roots, app) = setup();
    let mut overlap = roots.clone();
    overlap.cache = overlap.data.join("cache");
    assert_eq!(
        AppData::open(overlap).err().unwrap().code,
        ErrorCode::InvalidConfiguration
    );
    assert!(app.location(DataLocation::Data).child("../config").is_err());
    assert!(
        app.location(DataLocation::Data)
            .child("C:/outside")
            .is_err()
    );
    assert!(
        app.settings(DataScope::Account("a".repeat(97)), "prefs", 1, 500)
            .is_err()
    );
    assert!(DirectoryRoots::native("CON").is_err());
    let ctx = OperationContext::default();
    ctx.cancellation.cancel(gpui_system::CancelReason::Caller);
    let store = app
        .settings(DataScope::Application, "prefs", 1, 500)
        .unwrap();
    assert!(matches!(store.save(&1, &ctx), Ok(Completion::Cancelled(_))));
    assert_eq!(value(store.load::<u32>(&OperationContext::default())), None);
}
#[test]
fn lock_child_process() {
    if let Some(root) = std::env::var_os("GPUI_STORAGE_LOCK_CHILD") {
        let result = AppData::open(DirectoryRoots::under(root));
        assert_eq!(result.err().unwrap().code, ErrorCode::Busy);
    }
}
#[test]
fn concurrent_process_and_changed_control_cannot_bypass_lock() {
    let (root, mut roots, _app) = setup();
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_child_process", "--nocapture"])
        .env("GPUI_STORAGE_LOCK_CHILD", root.path().join("有 空格"))
        .status()
        .unwrap();
    assert!(child.success());
    roots.control = root.path().join("other-control");
    assert_eq!(AppData::open(roots).err().unwrap().code, ErrorCode::Busy);
}
#[test]
fn migration_reentry_is_busy_and_dropping_lease_does_not_deadlock() {
    let (_root, _roots, app) = setup();
    let ctx = OperationContext::default();
    let one = app
        .settings(DataScope::Application, "prefs", 1, 4096)
        .unwrap();
    value(one.save(&1, &ctx));
    let two = app
        .settings(DataScope::Application, "prefs", 2, 4096)
        .unwrap();
    let export = value(app.temporary_export(b"lease", 100, &ctx));
    let mut export = Some(export);
    value(two.migrate(&ctx, |_, value| {
        assert_eq!(one.load::<u32>(&ctx).unwrap_err().code, ErrorCode::Busy);
        drop(export.take());
        Ok(value)
    }));
    assert_eq!(value(app.cleanup_exports(Duration::ZERO, &ctx)), 1);
}
#[test]
fn native_resolution_is_read_only() {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        let roots = DirectoryRoots::native("dev.gpui-box.storage-test").unwrap();
        assert!(roots.config.is_absolute());
        assert!(roots.data.is_absolute());
    }
}
