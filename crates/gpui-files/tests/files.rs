use gpui_app_data::{AppData, DirectoryRoots};
use gpui_files::*;
use gpui_system::{
    CancelReason, Completion, DiagnosticPhase, ErrorCode, Event, EventEnvelope, OperationContext,
    Request, RetryAdvice, ServiceResult, SurfaceId, UiOwner,
};
use std::{
    cell::RefCell,
    future::Future,
    io::{self, Cursor, Read},
    path::Path,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

fn poll_ready<T: Future + Unpin>(future: &mut T) -> Poll<T::Output> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

fn picker_outcome<T>(request: &mut Request<PickerOutcome<T>>) -> PickerOutcome<T> {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Completed(outcome))) => outcome,
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("picker was cancelled: {reason:?}")
        }
        Poll::Ready(Err(error)) => panic!("picker failed: {error}"),
        Poll::Pending => panic!("picker did not complete"),
    }
}

fn expect_error<T>(request: &mut Request<T>) -> gpui_system::SystemError {
    match poll_ready(request) {
        Poll::Ready(Err(error)) => error,
        Poll::Ready(Ok(Completion::Completed(_))) => {
            panic!("request completed instead of returning an error")
        }
        Poll::Ready(Ok(Completion::Cancelled(reason))) => {
            panic!("request was cancelled: {reason:?}")
        }
        Poll::Pending => panic!("request did not complete"),
    }
}

fn expect_cancelled<T>(request: &mut Request<T>) -> CancelReason {
    match poll_ready(request) {
        Poll::Ready(Ok(Completion::Cancelled(reason))) => reason,
        Poll::Ready(Ok(Completion::Completed(_))) => {
            panic!("request completed before cancellation")
        }
        Poll::Ready(Err(error)) => panic!("request failed before cancellation: {error}"),
        Poll::Pending => panic!("cancelled request did not complete"),
    }
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.unwrap() {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("operation was cancelled: {reason:?}"),
    }
}

fn cancelled<T>(result: ServiceResult<T>) -> CancelReason {
    match result.unwrap() {
        Completion::Cancelled(reason) => reason,
        Completion::Completed(_) => panic!("operation completed instead of being cancelled"),
    }
}

fn ui_context(owner: &UiOwner) -> OperationContext {
    OperationContext {
        ui: Some(owner.context(None).unwrap()),
        ..Default::default()
    }
}

fn next_event(subscription: &EventSubscription<FileEvent>) -> EventEnvelope<FileEvent> {
    let mut next = subscription.try_next().unwrap();
    match poll_ready(&mut next) {
        Poll::Ready(Some(event)) => event,
        Poll::Ready(None) => panic!("event stream closed"),
        Poll::Pending => panic!("event did not arrive"),
    }
}

fn next_file_event(subscription: &EventSubscription<FileEvent>) -> FileEvent {
    match next_event(subscription).payload {
        Event::Value(event) => event,
        Event::ResyncRequired => panic!("file stream unexpectedly required resync"),
    }
}

fn scratch_provider() -> (tempfile::TempDir, Rc<ScratchFileProvider>) {
    let root = tempfile::tempdir().unwrap();
    let provider = Rc::new(ScratchFileProvider::open(root.path().join("scratch")).unwrap());
    (root, provider)
}

fn forged_local_grant(path: &Path, access: u8) -> PersistedGrant {
    let mut bytes = b"GPFL1".to_vec();
    bytes.push(access);
    bytes.push(1);
    bytes.extend_from_slice(path.to_string_lossy().as_bytes());
    PersistedGrant::from_bytes(bytes).unwrap()
}

fn app_data() -> (tempfile::TempDir, AppData) {
    let root = tempfile::tempdir().unwrap();
    let app = AppData::open(DirectoryRoots::under(root.path().join("app"))).unwrap();
    (root, app)
}

#[test]
fn pick_stream_edit_atomic_save_and_leased_export() {
    let (_root, provider) = scratch_provider();
    let file = provider
        .create_file("documents/notes.txt", b"draft content")
        .unwrap();
    provider.queue_selection(vec![file]);
    provider.set_auto_complete(true);
    let service = FileService::new(provider.clone());
    let owner = UiOwner::new(SurfaceId(1));
    let context = OperationContext::default();

    let filter = FileTypeFilter::new("Text", ["txt"]);
    let mut picker = service.open_file(
        OpenFileOptions::new()
            .with_filter(filter)
            .with_prompt("Open note"),
        ui_context(&owner),
    );
    let selected = match picker_outcome(&mut picker) {
        PickerOutcome::Selected(resource) => resource,
        PickerOutcome::Cancelled => panic!("expected a selection"),
    };
    assert_eq!(selected.metadata().display_name, "notes.txt");
    assert_eq!(selected.metadata().kind, ResourceKind::File);
    assert_eq!(selected.access(), AccessMode::ReadWrite);

    let mut read = Vec::new();
    assert_eq!(
        completed(selected.copy_to_writer(&mut read, &context, 4)),
        13
    );
    assert_eq!(read, b"draft content");

    let edited = b"final content with more bytes";
    let receipt = completed(selected.save_from_reader(
        &mut Cursor::new(edited),
        WriteOptions::overwrite(),
        &context,
    ));
    assert_eq!(receipt.atomicity, WriteAtomicity::AtomicRename);
    assert_eq!(receipt.bytes_written, edited.len() as u64);
    let mut reread = Vec::new();
    completed(selected.copy_to_writer(&mut reread, &context, 5));
    assert_eq!(reread, edited);

    let (_app_root, app) = app_data();
    let mut lease = completed(ExportResourceLease::lease_resource_for_export(
        &app, &selected, 4096, &context,
    ));
    let export_path = lease.path().unwrap().to_owned();
    assert_eq!(std::fs::read(&export_path).unwrap(), edited);
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 0);
    assert!(lease.release());
    assert!(!lease.is_active());
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 1);
    assert!(!export_path.exists());
}

#[test]
fn picker_cancellation_serialization_and_host_lifetime() {
    let (_root, provider) = scratch_provider();
    let service = FileService::new(provider.clone());
    let owner = UiOwner::new(SurfaceId(10));

    // User dismissal is a typed outcome, not an error.
    provider.set_auto_complete(true);
    let mut dismissed = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(matches!(
        picker_outcome(&mut dismissed),
        PickerOutcome::Cancelled
    ));

    // Cooperative cancellation is an outer Completion::Cancelled.
    provider.set_auto_complete(false);
    let context = ui_context(&owner);
    let token = context.cancellation.clone();
    let mut cancelled_request = service.open_file(OpenFileOptions::new(), context);
    assert!(poll_ready(&mut cancelled_request).is_pending());
    token.cancel(CancelReason::User);
    assert_eq!(expect_cancelled(&mut cancelled_request), CancelReason::User);
    assert!(service.active_picker(SurfaceId(10)).is_none());
    // The provider retains its native completion until the platform dismisses
    // it; a stale callback is rejected and the fake queue can be drained.
    assert!(!provider.cancel_next());

    // Same surface serializes; another surface is independent.
    let first = provider.create_file("first.txt", b"1").unwrap();
    let second = provider.create_file("second.txt", b"2").unwrap();
    let third = provider.create_file("third.txt", b"3").unwrap();
    let mut first_request = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(poll_ready(&mut first_request).is_pending());
    let mut busy = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    let error = expect_error(&mut busy);
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(error.retry, RetryAdvice::AfterConditionsChange);

    let other_owner = UiOwner::new(SurfaceId(11));
    let mut other_request = service.open_file(OpenFileOptions::new(), ui_context(&other_owner));
    assert!(poll_ready(&mut other_request).is_pending());
    assert_eq!(provider.pending_count(), 2);
    assert!(provider.complete_next(vec![first]));
    assert!(provider.complete_next(vec![second]));
    assert!(matches!(
        picker_outcome(&mut first_request),
        PickerOutcome::Selected(_)
    ));
    assert!(matches!(
        picker_outcome(&mut other_request),
        PickerOutcome::Selected(_)
    ));
    assert!(service.active_picker(SurfaceId(10)).is_none());
    assert!(service.active_picker(SurfaceId(11)).is_none());

    // Owner disposal rejects a late native callback.
    let mut disposed_request = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(poll_ready(&mut disposed_request).is_pending());
    owner.dispose();
    assert!(!provider.complete_next(vec![third]));
    assert_eq!(
        expect_error(&mut disposed_request).code,
        ErrorCode::ContextExpired
    );
    assert!(service.active_picker(SurfaceId(10)).is_none());

    // Generation rotation rejects the old callback and frees the lane.
    let rotated = UiOwner::new(SurfaceId(12));
    let late = provider.create_file("late.txt", b"late").unwrap();
    let fresh = provider.create_file("fresh.txt", b"fresh").unwrap();
    let mut late_request = service.open_file(OpenFileOptions::new(), ui_context(&rotated));
    assert!(poll_ready(&mut late_request).is_pending());
    rotated.advance_generation().unwrap();
    assert!(!provider.complete_next(vec![late]));
    assert_eq!(
        expect_error(&mut late_request).code,
        ErrorCode::ContextExpired
    );
    let mut fresh_request = service.open_file(OpenFileOptions::new(), ui_context(&rotated));
    assert!(poll_ready(&mut fresh_request).is_pending());
    assert!(provider.complete_next(vec![fresh]));
    assert!(matches!(
        picker_outcome(&mut fresh_request),
        PickerOutcome::Selected(_)
    ));
}

#[test]
fn all_picker_operations_and_worker_rejection() {
    let (_root, provider) = scratch_provider();
    let first = provider.create_file("one.txt", b"one").unwrap();
    let second = provider.create_file("two.txt", b"two").unwrap();
    let directory = provider.create_directory("folder").unwrap();
    let save_target = provider.resource_at(
        provider.root().join("saved.txt"),
        AccessMode::ReadWrite,
        DurableGrantSupport::Unsupported,
    );
    provider.queue_selection(vec![first.clone()]);
    provider.queue_selection(vec![first.clone(), second.clone()]);
    provider.queue_selection(vec![directory.clone()]);
    provider.queue_selection(vec![save_target.clone()]);
    provider.set_auto_complete(true);
    let service = FileService::new(provider.clone());
    let owner = UiOwner::new(SurfaceId(20));

    let mut single = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(matches!(
        picker_outcome(&mut single),
        PickerOutcome::Selected(resource) if resource.metadata().kind == ResourceKind::File
    ));
    let events = service.subscribe();
    let mut many = service.open_files(OpenFilesOptions::new(), ui_context(&owner));
    match picker_outcome(&mut many) {
        PickerOutcome::Selected(resources) => {
            assert_eq!(resources.len(), 2);
            assert_eq!(resources[0].id(), first.id());
        }
        PickerOutcome::Cancelled => panic!("expected multiple selection"),
    }
    assert!(matches!(
        next_file_event(&events),
        FileEvent::PickerStarted {
            operation: FileOperation::OpenMultipleFiles,
            ..
        }
    ));
    assert!(matches!(
        next_file_event(&events),
        FileEvent::PickerSelected {
            operation: FileOperation::OpenMultipleFiles,
            resources: 2,
            ..
        }
    ));
    let mut picker = service.pick_directory(PickDirectoryOptions::new(), ui_context(&owner));
    match picker_outcome(&mut picker) {
        PickerOutcome::Selected(resource) => {
            assert_eq!(resource.metadata().kind, ResourceKind::Directory);
            assert_eq!(
                resource
                    .open_reader(&OperationContext::default())
                    .err()
                    .unwrap()
                    .code,
                ErrorCode::InvalidInput
            );
        }
        PickerOutcome::Cancelled => panic!("expected directory selection"),
    }
    let mut save = service.save_file(
        SaveFileOptions::new().with_suggested_name("saved.txt"),
        ui_context(&owner),
    );
    let target = match picker_outcome(&mut save) {
        PickerOutcome::Selected(resource) => resource,
        PickerOutcome::Cancelled => panic!("expected save target"),
    };
    completed(target.save_from_reader(
        &mut Cursor::new(b"saved"),
        WriteOptions::create_new(),
        &OperationContext::default(),
    ));
    assert_eq!(
        std::fs::read(provider.root().join("saved.txt")).unwrap(),
        b"saved"
    );

    let operations: Vec<FileOperation> = provider
        .requests()
        .into_iter()
        .map(|request| request.operation)
        .collect();
    for expected in [
        FileOperation::OpenFile,
        FileOperation::OpenMultipleFiles,
        FileOperation::PickDirectory,
        FileOperation::SaveFile,
    ] {
        assert!(operations.contains(&expected), "missing {expected:?}");
    }
    let filter = FileTypeFilter::new("Text", ["txt"]);
    assert!(filter.matches(&first.metadata()));
    assert!(service.capability(FileOperation::OpenFile).permission == PermissionState::NotRequired);

    // A UI context moved to a worker thread cannot begin a native picker.
    let worker_owner = UiOwner::new(SurfaceId(21));
    let worker_context = ui_context(&worker_owner);
    std::thread::spawn(move || {
        let provider = Rc::new(ScratchFileProvider::new());
        let service = FileService::new(provider.clone());
        let mut request = service.open_file(OpenFileOptions::new(), worker_context);
        assert_eq!(expect_error(&mut request).code, ErrorCode::ContextExpired);
        assert_eq!(provider.pending_count(), 0);
    })
    .join()
    .unwrap();
}

#[test]
fn durable_grants_restore_recover_and_redact_faults() {
    let root = tempfile::tempdir().unwrap();
    let scratch = root.path().join("scratch");
    let (bytes, grant) = {
        let provider = Rc::new(ScratchFileProvider::open(&scratch).unwrap());
        let service = FileService::new(provider.clone());
        let file = provider
            .create_durable_file(
                "durable/report.txt",
                b"durable body",
                DurableGrantSupport::SecurityScopedBookmark,
            )
            .unwrap();
        let context = OperationContext::default();
        let grant = completed(service.persist_grant(&file, &context));
        assert!(grant.len() < 16 * 1024);
        (grant.to_bytes(), grant)
    };

    // Cross-restart restore from serialized grant bytes.
    let provider = Rc::new(ScratchFileProvider::open(&scratch).unwrap());
    let service = FileService::new(provider.clone());
    let context = OperationContext::default();
    let restored = completed(service.restore_grant_bytes(bytes.clone(), &context));
    let resource = match restored {
        GrantRestoreOutcome::Restored { resource, .. } => resource,
        other => panic!("expected restored grant, got {other:?}"),
    };
    let mut content = Vec::new();
    completed(resource.copy_to_writer(&mut content, &context, 4));
    assert_eq!(content, b"durable body");

    // Unsupported providers refuse to mint a grant.
    let plain = provider.create_file("plain.txt", b"plain").unwrap();
    let error = plain.persist_grant(&context).unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);

    // Revoked and moved/deleted grants are ordinary recovery outcomes.
    provider.revoke_grant(&grant);
    assert!(matches!(
        completed(service.restore_grant(&grant, &context)),
        GrantRestoreOutcome::Revoked
    ));
    let moved = provider
        .create_durable_file(
            "moved.txt",
            b"moved",
            DurableGrantSupport::SecurityScopedBookmark,
        )
        .unwrap();
    let moved_grant = completed(service.persist_grant(&moved, &context));
    std::fs::remove_file(provider.root().join("moved.txt")).unwrap();
    assert!(matches!(
        completed(service.restore_grant(&moved_grant, &context)),
        GrantRestoreOutcome::MissingOrMoved
    ));
    assert!(matches!(
        completed(service.restore_grant_bytes(b"not a valid grant".to_vec(), &context)),
        GrantRestoreOutcome::InvalidOrCorrupt
    ));
    assert!(matches!(
        completed(service.restore_grant_bytes(vec![b'x'; 20_000], &context)),
        GrantRestoreOutcome::InvalidOrCorrupt
    ));

    // A provider stream fault is a static, retryable, path-free error.
    let stream = provider.create_stream_with_fault(
        "cloud.bin",
        &[7u8; 64],
        DurableGrantSupport::PersistableUriGrant,
        FaultInjection::read_after(5),
    );
    let error = stream.read_chunks(&context, 16, |_| {}).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert_eq!(error.retry, RetryAdvice::AfterConditionsChange);
    assert!(!format!("{error:?}").contains("content://"));
    assert!(!format!("{error:?}").contains(provider.root().to_str().unwrap()));

    let write_fault = provider.create_stream_with_fault(
        "quota.bin",
        b"initial",
        DurableGrantSupport::Unsupported,
        FaultInjection::write_after(2),
    );
    let error = write_fault
        .save_from_reader(
            &mut Cursor::new(b"0123456789"),
            WriteOptions::new(ConflictPolicy::Overwrite, AtomicityPolicy::BestEffort),
            &context,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PlatformFailure);
    assert_eq!(error.retry, RetryAdvice::AfterConditionsChange);

    // Provider-backed grants can also be minted and restored while the
    // simulated provider session remains alive.
    let stream_grant = completed(service.persist_grant(&stream, &context));
    assert!(matches!(
        completed(service.restore_grant(&stream_grant, &context)),
        GrantRestoreOutcome::Restored { .. }
    ));
}

#[test]
fn forged_local_grant_outside_scratch_root_is_rejected() {
    let (_root, provider) = scratch_provider();
    let external_root = tempfile::tempdir().unwrap();
    let external = external_root.path().join("external.txt");
    std::fs::write(&external, b"outside the scratch root").unwrap();
    assert!(!external.starts_with(provider.root()));
    let service = FileService::new(provider);
    let context = OperationContext::default();
    let grant = forged_local_grant(&external, 1);
    assert!(matches!(
        completed(service.restore_grant(&grant, &context)),
        GrantRestoreOutcome::InvalidOrCorrupt
    ));
}

#[cfg(unix)]
#[test]
fn forged_local_grant_for_symlink_escape_is_rejected() {
    let (_root, provider) = scratch_provider();
    let external_root = tempfile::tempdir().unwrap();
    let external = external_root.path().join("external.txt");
    std::fs::write(&external, b"outside the scratch root").unwrap();
    let link = provider.root().join("escape.txt");
    std::os::unix::fs::symlink(&external, &link).unwrap();
    let service = FileService::new(provider);
    let context = OperationContext::default();
    let grant = forged_local_grant(&link, 1);
    assert!(matches!(
        completed(service.restore_grant(&grant, &context)),
        GrantRestoreOutcome::InvalidOrCorrupt
    ));
}

#[test]
fn tampered_stream_grant_access_is_rejected() {
    let (_root, provider) = scratch_provider();
    let stream = provider.create_read_only_stream(
        "readonly.txt",
        b"read only",
        DurableGrantSupport::PersistableUriGrant,
    );
    let service = FileService::new(provider);
    let context = OperationContext::default();
    let mut bytes = completed(service.persist_grant(&stream, &context)).to_bytes();
    assert_eq!(&bytes[..5], b"GPFS1");
    assert_eq!(bytes[5], 0);
    bytes[5] = 1;
    assert!(matches!(
        completed(service.restore_grant_bytes(bytes, &context)),
        GrantRestoreOutcome::InvalidOrCorrupt
    ));
}

#[test]
fn write_conflict_atomicity_access_and_release_semantics() {
    let (_root, provider) = scratch_provider();
    let service = FileService::new(provider.clone());
    let context = OperationContext::default();
    let file = provider.create_file("target.txt", b"original").unwrap();

    let conflict = file
        .save_from_reader(
            &mut Cursor::new(b"replacement"),
            WriteOptions::create_new(),
            &context,
        )
        .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::InvalidInput);
    assert_eq!(
        std::fs::read(provider.root().join("target.txt")).unwrap(),
        b"original"
    );

    let expected = file.metadata().revision;
    std::fs::write(provider.root().join("target.txt"), b"changed externally").unwrap();
    let stale = file
        .save_from_reader(
            &mut Cursor::new(b"must not land"),
            WriteOptions::replace_if_unchanged(expected),
            &context,
        )
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::ResourceExpired);
    assert_eq!(
        std::fs::read(provider.root().join("target.txt")).unwrap(),
        b"changed externally"
    );

    let receipt = completed(file.save_from_reader(
        &mut Cursor::new(b"overwritten"),
        WriteOptions::overwrite(),
        &context,
    ));
    assert_eq!(receipt.atomicity, WriteAtomicity::AtomicRename);
    assert_eq!(
        completed(file.write_chunks(
            vec![b"chunked".to_vec()],
            WriteOptions::overwrite(),
            &context
        ))
        .bytes_written,
        7
    );

    let fresh_path = provider.root().join("fresh.txt");
    let target = provider.resource_at(
        &fresh_path,
        AccessMode::ReadWrite,
        DurableGrantSupport::Unsupported,
    );
    let receipt = completed(target.save_from_reader(
        &mut Cursor::new(b"fresh"),
        WriteOptions::create_new(),
        &context,
    ));
    assert_eq!(receipt.bytes_written, 5);
    assert_eq!(std::fs::read(&fresh_path).unwrap(), b"fresh");

    // Provider streams are non-atomic. RequireAtomic rejects before writing.
    let stream = provider.create_stream(
        "stream.txt",
        b"stream-original",
        DurableGrantSupport::Unsupported,
    );
    let error = stream
        .save_from_reader(
            &mut Cursor::new(b"atomic"),
            WriteOptions::overwrite(),
            &context,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    let receipt = completed(stream.save_from_reader(
        &mut Cursor::new(b"stream-updated"),
        WriteOptions::new(ConflictPolicy::Overwrite, AtomicityPolicy::BestEffort),
        &context,
    ));
    assert_eq!(receipt.atomicity, WriteAtomicity::NonAtomicTruncate);
    let mut content = Vec::new();
    completed(stream.copy_to_writer(&mut content, &context, 4));
    assert_eq!(content, b"stream-updated");

    let read_only_stream = provider.create_read_only_stream(
        "readonly-stream.txt",
        b"keep-stream",
        DurableGrantSupport::Unsupported,
    );
    let error = read_only_stream
        .save_from_reader(
            &mut Cursor::new(b"nope"),
            WriteOptions::new(ConflictPolicy::Overwrite, AtomicityPolicy::BestEffort),
            &context,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);

    // Read-only grants reject writes before touching the target.
    let read_only = provider
        .create_read_only_file("readonly.txt", b"keep")
        .unwrap();
    let error = read_only
        .save_from_reader(
            &mut Cursor::new(b"nope"),
            WriteOptions::overwrite(),
            &context,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(
        std::fs::read(provider.root().join("readonly.txt")).unwrap(),
        b"keep"
    );

    // Explicit release is idempotent and revokes all clones immediately.
    let clone = file.clone();
    assert!(service.release_resource(&file));
    assert!(!service.release_resource(&file));
    assert!(clone.is_released());
    assert_eq!(
        clone.open_reader(&context).err().unwrap().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        clone.persist_grant(&context).err().unwrap().code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn large_streaming_cancellation_cleanup_and_ui_context_rejection() {
    let (_root, provider) = scratch_provider();
    let context = OperationContext::default();
    const CHUNK: usize = 64 * 1024;
    let total = 3 * 1024 * 1024 + 17;
    let data: Vec<u8> = (0..total).map(|index| (index % 251) as u8).collect();
    let file = provider.create_file("large.bin", &data).unwrap();
    let mut reader = completed(file.open_reader(&context));
    let mut head = [0u8; 4];
    assert_eq!(reader.read(&mut head).unwrap(), 4);
    assert_eq!(head, data[..4]);

    let mut chunks = 0usize;
    let mut sum = 0usize;
    let read = completed(file.read_chunks(&context, CHUNK, |chunk| {
        assert!(chunk.len() <= CHUNK);
        chunks += 1;
        sum += chunk.len();
    }));
    assert_eq!(read, total as u64);
    assert_eq!(sum, total);
    assert!(chunks >= 48);

    let count = total.div_ceil(CHUNK);
    let receipt = completed(file.write_chunks(
        (0..count).map(|index| vec![(index % 251) as u8; CHUNK]),
        WriteOptions::overwrite(),
        &context,
    ));
    assert_eq!(receipt.bytes_written, (count * CHUNK) as u64);
    let mut rewritten = 0usize;
    completed(file.read_chunks(&context, CHUNK, |chunk| rewritten += chunk.len()));
    assert_eq!(rewritten, count * CHUNK);

    // Mid-stream cancellation abandons staging and preserves the target.
    let target = provider.create_file("target.txt", b"original").unwrap();
    let token = context.cancellation.clone();
    struct CancellingReader {
        remaining: usize,
        calls: usize,
        token: gpui_system::CancellationToken,
    }
    impl Read for CancellingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Ok(0);
            }
            self.calls += 1;
            if self.calls == 3 {
                self.token.cancel(CancelReason::User);
            }
            let read = buffer.len().min(1024).min(self.remaining);
            buffer[..read].fill(b'x');
            self.remaining -= read;
            Ok(read)
        }
    }
    let mut reader = CancellingReader {
        remaining: 1024 * 1024,
        calls: 0,
        token,
    };
    assert_eq!(
        cancelled(target.save_from_reader(&mut reader, WriteOptions::overwrite(), &context,)),
        CancelReason::User
    );
    assert_eq!(
        std::fs::read(provider.root().join("target.txt")).unwrap(),
        b"original"
    );
    let leftovers: Vec<_> = std::fs::read_dir(provider.root())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".gpui-files-")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "staging temp files were not cleaned up"
    );

    // UI-bound contexts cannot run blocking stream I/O.
    let owner = UiOwner::new(SurfaceId(60));
    let ui = ui_context(&owner);
    assert_eq!(
        file.open_reader(&ui).err().unwrap().code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        file.save_from_reader(&mut Cursor::new(b"x"), WriteOptions::overwrite(), &ui)
            .err()
            .unwrap()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        file.write_chunks(vec![b"x".to_vec()], WriteOptions::overwrite(), &ui)
            .err()
            .unwrap()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        file.read_chunks(&ui, CHUNK, |_| {}).err().unwrap().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn export_lease_pins_until_release_or_drop() {
    let (_app_root, app) = app_data();
    let context = OperationContext::default();
    let mut lease = completed(ExportResourceLease::stage_export_bytes(
        &app, b"handoff", 64, &context,
    ));
    let path = lease.path().unwrap().to_owned();
    assert_eq!(std::fs::read(&path).unwrap(), b"handoff");
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 0);
    assert!(path.exists());
    assert!(lease.release());
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 1);
    assert!(!path.exists());

    let dropped = completed(ExportResourceLease::stage_export_bytes(
        &app, b"drop", 64, &context,
    ));
    let dropped_path = dropped.path().unwrap().to_owned();
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 0);
    drop(dropped);
    assert_eq!(completed(app.cleanup_exports(Duration::ZERO, &context)), 1);
    assert!(!dropped_path.exists());

    let reader = completed(ExportResourceLease::stage_export_from_reader(
        &app,
        &mut Cursor::new(b"streamed"),
        64,
        &context,
    ));
    assert_eq!(std::fs::read(reader.path().unwrap()).unwrap(), b"streamed");
}

std::thread_local! {
    static REENTRANT_SERVICE: RefCell<Option<FileService>> = const { RefCell::new(None) };
}

struct ReentrantWake {
    woken: Arc<AtomicBool>,
}

impl Wake for ReentrantWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        REENTRANT_SERVICE.with(|slot| {
            if let Some(service) = slot.borrow().as_ref().cloned() {
                let _ = service.revision();
                let _ = service.active_picker(SurfaceId(90));
            }
        });
        self.woken.store(true, Ordering::SeqCst);
    }
}

#[test]
fn multi_subscriber_reentrancy_and_strict_redaction() {
    let (_root, provider) = scratch_provider();
    let file = provider
        .create_durable_file(
            "private/secret-report.txt",
            b"secret body",
            DurableGrantSupport::SecurityScopedBookmark,
        )
        .unwrap();
    let stream = provider.create_stream_with_fault(
        "cloud-secret.txt",
        b"cloud body",
        DurableGrantSupport::PersistableUriGrant,
        FaultInjection::read_after(2),
    );
    provider.queue_selection(vec![file.clone()]);
    provider.set_auto_complete(true);
    let service = FileService::new(provider.clone());
    let owner = UiOwner::new(SurfaceId(80));
    let context = OperationContext::default();

    let first = service.subscribe();
    let second = service.subscribe();
    let mut picker = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(matches!(
        picker_outcome(&mut picker),
        PickerOutcome::Selected(_)
    ));
    assert!(matches!(
        next_file_event(&first),
        FileEvent::PickerStarted { .. }
    ));
    assert!(matches!(
        next_file_event(&first),
        FileEvent::PickerSelected { resources: 1, .. }
    ));
    assert!(matches!(
        next_file_event(&second),
        FileEvent::PickerStarted { .. }
    ));
    assert!(matches!(
        next_file_event(&second),
        FileEvent::PickerSelected { resources: 1, .. }
    ));

    // A waker may re-enter the service while an event is delivered.
    REENTRANT_SERVICE.with(|slot| *slot.borrow_mut() = Some(service.clone()));
    let woken = Arc::new(AtomicBool::new(false));
    let waker = Waker::from(Arc::new(ReentrantWake {
        woken: Arc::clone(&woken),
    }));
    let mut next = first.try_next().unwrap();
    assert!(
        Pin::new(&mut next)
            .poll(&mut Context::from_waker(&waker))
            .is_pending()
    );
    provider.queue_selection(vec![file.clone()]);
    let mut reentrant_picker = service.open_file(OpenFileOptions::new(), ui_context(&owner));
    assert!(matches!(
        picker_outcome(&mut reentrant_picker),
        PickerOutcome::Selected(_)
    ));
    assert!(woken.load(Ordering::SeqCst));
    assert!(matches!(
        Pin::new(&mut next).poll(&mut Context::from_waker(&waker)),
        Poll::Ready(Some(_))
    ));
    drop(next);
    REENTRANT_SERVICE.with(|slot| *slot.borrow_mut() = None);

    // Every public Debug/error surface must omit scratch paths and URIs.
    let root_text = provider.root().to_string_lossy().to_string();
    let full_path = provider.root().join("private/secret-report.txt");
    let full_text = full_path.to_string_lossy().to_string();
    let grant = completed(service.persist_grant(&file, &context));
    let stream_grant = completed(service.persist_grant(&stream, &context));
    let stream_error = stream.read_chunks(&context, 1, |_| {}).unwrap_err();
    let restored_outcome = completed(service.restore_grant(&grant, &context));
    let (_, port) = gpui_system::request::<()>(OperationContext::default());
    let diagnostic = port.diagnostic(DiagnosticPhase::Rejected);
    let (_app_root, app) = app_data();
    let mut lease = completed(ExportResourceLease::stage_export_bytes(
        &app, b"export", 64, &context,
    ));
    let export_path = lease.path().unwrap().to_string_lossy().to_string();

    let surfaces = [
        format!("{file:?}"),
        format!("{stream:?}"),
        format!("{grant:?}"),
        format!("{stream_grant:?}"),
        format!("{lease:?}"),
        format!("{stream_error:?}"),
        format!("{restored_outcome:?}"),
        format!("{diagnostic:?}"),
    ];
    for text in surfaces {
        assert!(!text.contains(&root_text), "leaked scratch root: {text}");
        assert!(!text.contains(&full_text), "leaked full path: {text}");
        assert!(!text.contains("content://"), "leaked provider URI: {text}");
        assert!(!text.contains(&export_path), "leaked export path: {text}");
    }
    assert!(lease.release());
}

#[test]
fn service_registration_capabilities_and_manifest() {
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| !manifest.native_provider_installed)
    );
    assert!(
        PROVIDER_MANIFEST
            .iter()
            .all(|manifest| manifest.evidence == gpui_system::Evidence::ContractOnly)
    );

    let (_root, provider) = scratch_provider();
    let service = FileService::new(provider);
    let capabilities = service.capabilities();
    assert_eq!(capabilities.len(), FileOperation::ALL.len());
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.permission == PermissionState::NotRequired)
    );
    let mut services = gpui_system::SystemServices::new();
    services.register(service.clone()).unwrap();
    assert!(services.register(service.clone()).is_err());
    assert!(services.service::<FileService>().is_ok());
}
