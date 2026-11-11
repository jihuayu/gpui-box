//! End-to-end authorized document workflow without any OS dialog.
//!
//! Demonstrates pick -> bounded read -> edit -> atomic save -> durable grant
//! restore -> consumer export lease cleanup. All printed values are public-safe
//! metadata; no scratch path or provider URI is shown.

use gpui_app_data::{AppData, DirectoryRoots};
use gpui_files::{
    AccessMode, Completion, DurableGrantSupport, ExportResourceLease, FileOperation, FileResource,
    FileService, GrantRestoreOutcome, OpenFileOptions, PickerOutcome, ResourceKind,
    ScratchFileProvider, ServiceResult, SurfaceId, UiOwner, WriteOptions,
};
use gpui_system::OperationContext;
use std::{error::Error, time::Duration};

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation failed") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("operation was cancelled: {reason:?}"),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let scratch = root.path().join("authorized");
    let provider = ScratchFileProvider::open(&scratch)?;
    let file = provider.create_durable_file(
        "notes/plan.txt",
        b"first draft",
        DurableGrantSupport::SecurityScopedBookmark,
    )?;
    provider.queue_selection(vec![file]);
    provider.set_auto_complete(true);

    let service = FileService::new(std::rc::Rc::new(provider));
    let owner = UiOwner::new(SurfaceId(1));
    let ui = OperationContext {
        ui: Some(owner.context(None)?),
        ..Default::default()
    };

    // Foreground picker: a dismissal is `PickerOutcome::Cancelled`, never an error.
    let mut picker = service.open_file(OpenFileOptions::new().with_prompt("Open plan"), ui);
    let picked = match completed_picker(&mut picker) {
        PickerOutcome::Selected(resource) => resource,
        PickerOutcome::Cancelled => {
            println!("picker cancelled");
            return Ok(());
        }
    };
    print_resource("picked", &picked);

    // Blocking read on a background context in bounded 64 KiB chunks.
    let background = OperationContext::default();
    let mut bytes = Vec::new();
    let read = completed(picked.copy_to_writer(&mut bytes, &background, 64 * 1024));
    println!("read {read} bytes");

    bytes.extend_from_slice(b"\nedited locally");
    let receipt = completed(picked.save_from_reader(
        &mut std::io::Cursor::new(bytes),
        WriteOptions::overwrite(),
        &background,
    ));
    println!(
        "saved {} bytes with {:?}",
        receipt.bytes_written, receipt.atomicity
    );

    let grant = completed(service.persist_grant(&picked, &background));
    println!("persisted grant of {} opaque bytes", grant.len());

    // Simulate the next application session: drop the service, picker lane and
    // provider, then reopen the deterministic scratch root.
    drop(service);
    drop(picked);
    let provider = ScratchFileProvider::open(&scratch)?;
    let service = FileService::new(std::rc::Rc::new(provider));
    let restored = completed(service.restore_grant_bytes(grant.to_bytes(), &background));
    let restored = match restored {
        GrantRestoreOutcome::Restored { resource, .. } => resource,
        other => {
            println!("grant recovery: {other:?}; prompt the user again");
            return Ok(());
        }
    };
    print_resource("restored", &restored);

    let app_root = root.path().join("app-data");
    let app = AppData::open(DirectoryRoots::under(&app_root))?;
    let mut lease = completed(ExportResourceLease::lease_resource_for_export(
        &app,
        &restored,
        1024 * 1024,
        &background,
    ));
    println!(
        "export lease active={} bytes={} resource={:?}",
        lease.is_active(),
        lease.bytes(),
        lease.resource_id()
    );
    // A consumer still holds the lease: cleanup cannot remove the export.
    let cleaned = completed(app.cleanup_exports(Duration::ZERO, &background));
    println!("cleanup while leased removed {cleaned} export(s)");
    lease.release();
    let cleaned = completed(app.cleanup_exports(Duration::ZERO, &background));
    println!("cleanup after release removed {cleaned} export(s)");

    assert_eq!(
        service.capability(FileOperation::StreamingIo).permission,
        gpui_files::PermissionState::NotRequired
    );
    Ok(())
}

fn completed_picker<T>(request: &mut gpui_system::Request<PickerOutcome<T>>) -> PickerOutcome<T> {
    match futures_lite_block_on(request) {
        Ok(Completion::Completed(outcome)) => outcome,
        Ok(Completion::Cancelled(reason)) => panic!("picker cancelled: {reason:?}"),
        Err(error) => panic!("picker failed: {error}"),
    }
}

fn futures_lite_block_on<T>(request: &mut gpui_system::Request<T>) -> ServiceResult<T> {
    // The core owns no executor; this example resolves immediately because the
    // deterministic provider completes inline. Poll once without a waker.
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(request).poll(&mut context) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("picker did not resolve inline"),
    }
}

fn print_resource(label: &str, resource: &FileResource) {
    let metadata = resource.metadata();
    assert_eq!(metadata.kind, ResourceKind::File);
    assert_eq!(metadata.access, AccessMode::ReadWrite);
    println!(
        "{label}: id={} name={} bytes={:?} revision={} atomic={:?} durable={:?}",
        metadata.id,
        metadata.display_name,
        metadata.size,
        metadata.revision.value(),
        metadata.write_atomicity,
        metadata.durable_grant
    );
}
