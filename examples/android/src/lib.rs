#![cfg(target_os = "android")]
gpui_box::android::android_entrypoint!(gpui_box_example_hello::open);

/// Example-only instrumentation entry. Production consumers use app_data_roots
/// and AppData directly on their own background executor.
///
/// # Safety
/// Called by the JVM with a live local JNI environment and class. Storage
/// discovery rejects UI-thread calls before any AppData operation.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_dev_gpui_box_StorageHostTest_nativeStorageProbe(
    env: gpui_box::android::jni::JNIEnv,
    _: gpui_box::android::jni::objects::JClass,
) -> gpui_box::android::jni::sys::jstring {
    let outcome = std::panic::catch_unwind(storage_probe);
    let text = match outcome {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => serde_json::json!({"success": false, "error": error}).to_string(),
        Err(_) => {
            serde_json::json!({"success": false, "error": "Storage probe panicked"}).to_string()
        }
    };
    env.new_string(text)
        .map(|value| value.into_raw())
        .unwrap_or_default()
}

fn storage_probe() -> Result<String, String> {
    use gpui_box::android::app_data::{AppData, DataLocation, DataScope};
    use gpui_system::{Completion, OperationContext};
    use serde_json::json;
    fn completed<T>(result: gpui_system::ServiceResult<T>) -> Result<T, String> {
        match result.map_err(|error| format!("{error:?}"))? {
            Completion::Completed(value) => Ok(value),
            _ => Err("Storage probe cancelled".into()),
        }
    }
    let roots = gpui_box::android::app_data_roots().map_err(|error| format!("{error:?}"))?;
    let app = AppData::open(roots.clone()).map_err(|error| format!("{error:?}"))?;
    let context = OperationContext::default();
    let store = app
        .settings(DataScope::Application, "host-probe", 1, 4096)
        .map_err(|error| format!("{error:?}"))?;
    let previous = completed(store.load::<serde_json::Value>(&context))?;
    let visits = previous
        .as_ref()
        .and_then(|value| value["visits"].as_u64())
        .unwrap_or(0)
        + 1;
    completed(store.save(
        &json!({"visits": visits, "text": "Private 你好🙂"}),
        &context,
    ))?;
    let cache = app
        .location(DataLocation::Cache)
        .child("probe-cache")
        .map_err(|error| format!("{error:?}"))?;
    std::fs::write(cache, b"regenerable").map_err(|error| error.to_string())?;
    let export = completed(app.temporary_export(b"export", 64, &context))?;
    let export_path = export.path().to_owned();
    let cleared = completed(app.clear_cache(&context))?;
    if !export_path.exists() {
        return Err("Cache cleanup removed a leased export".into());
    }
    drop(export);
    completed(app.cleanup_exports(std::time::Duration::ZERO, &context))?;
    drop(store);
    drop(app);
    let app = AppData::open(roots.clone()).map_err(|error| format!("{error:?}"))?;
    let store = app
        .settings(DataScope::Application, "host-probe", 1, 4096)
        .map_err(|error| format!("{error:?}"))?;
    let reloaded =
        completed(store.load::<serde_json::Value>(&context))?.ok_or("Missing reloaded settings")?;
    Ok(
        json!({"success": true, "visits": visits, "reloaded": reloaded, "cleared": cleared,
        "control": roots.control, "config": roots.config, "data": roots.data, "cache": roots.cache,
        "logs": roots.logs, "temporary": roots.temporary, "sandboxed": roots.policy.sandboxed,
        "export_removed": !export_path.exists()})
        .to_string(),
    )
}
