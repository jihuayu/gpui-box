//! Recoverable export lease, versioned JSON bundle and six-platform matrix.

mod support;

use gpui_diagnostics::{
    CrashRecordSource, DiagnosticDomain, DiagnosticEvent, DiagnosticSeverity, DiagnosticsService,
    PlatformTarget, TerminationKind, platform_support_matrix,
};
use gpui_files::{
    AccessMode, ChunkSource, Completion, DurableGrantSupport, ErrorCode, FileResource,
    ResourceBackend, ResourceId, ResourceKind, ResourceMetadata, ResourceRevision, SystemError,
    WriteAtomicitySupport, WriteOptions, WriteReceipt,
};
use std::{fs, io::Read, path::PathBuf, sync::Arc};
use support::*;

struct ExportPathBackend {
    path: PathBuf,
}

impl ResourceBackend for ExportPathBackend {
    fn access(&self) -> AccessMode {
        AccessMode::ReadOnly
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::File
    }

    fn durable_grant(&self) -> DurableGrantSupport {
        DurableGrantSupport::Unsupported
    }

    fn write_atomicity(&self) -> WriteAtomicitySupport {
        WriteAtomicitySupport::AtomicRename
    }

    fn current_revision(&self) -> Result<ResourceRevision, SystemError> {
        Ok(ResourceRevision(1))
    }

    fn exists(&self) -> Result<bool, SystemError> {
        Ok(self.path.is_file())
    }

    fn open_read(&self) -> Result<Box<dyn Read + Send>, SystemError> {
        fs::File::open(&self.path)
            .map(|file| Box::new(file) as Box<dyn Read + Send>)
            .map_err(|_| SystemError::new(ErrorCode::PlatformFailure, "Test export reader failed"))
    }

    fn persist_grant(&self) -> Result<Vec<u8>, SystemError> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Test export is not a durable resource",
        ))
    }

    fn save(
        &self,
        _source: &mut dyn ChunkSource,
        _options: &WriteOptions,
        _context: &gpui_diagnostics::OperationContext,
    ) -> gpui_diagnostics::ServiceResult<WriteReceipt> {
        Err(SystemError::new(
            ErrorCode::Unsupported,
            "Test export is read-only",
        ))
    }
}

fn resource_for(path: PathBuf, size: u64) -> FileResource {
    FileResource::new(
        Arc::new(ExportPathBackend { path }),
        ResourceMetadata {
            id: ResourceId::new(),
            display_name: "diagnostic-export.json".to_owned(),
            extension: Some("json".to_owned()),
            mime_hint: Some("application/json"),
            size: Some(size),
            kind: ResourceKind::File,
            access: AccessMode::ReadOnly,
            write_atomicity: WriteAtomicitySupport::AtomicRename,
            durable_grant: DurableGrantSupport::Unsupported,
            revision: ResourceRevision(1),
        },
    )
}

fn read_resource(resource: &FileResource) -> Vec<u8> {
    let context = context();
    let mut reader = match resource.open_reader(&context).expect("open reader") {
        Completion::Completed(reader) => reader,
        Completion::Cancelled(reason) => panic!("reader cancelled: {reason:?}"),
    };
    let mut bytes = Vec::new();
    loop {
        match reader
            .read_chunk(&context, 64 * 1024)
            .expect("read export chunk")
        {
            Completion::Completed(Some(chunk)) => bytes.extend_from_slice(&chunk),
            Completion::Completed(None) => break,
            Completion::Cancelled(reason) => panic!("read cancelled: {reason:?}"),
        }
    }
    bytes
}

#[test]
fn export_bundle_is_versioned_redacted_recoverable_and_platform_complete() {
    let (_root, app) = app();
    let build = identity();
    assert_exact_identity(&build);
    let context = context();

    let first_store = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
    let first_report = completed(first_store.recover(&context));
    completed(first_store.record_event(
        DiagnosticEvent::new(
            DiagnosticDomain::Command,
            DiagnosticSeverity::Error,
            "command failed at /home/alice/private/CANARY_UNIX_PATH with Bearer CANARY_BEARER_VALUE",
        )
        .with_text("token", "CANARY_TOKEN_VALUE")
        .with_text("file_path", "C:\\Users\\alice\\CANARY_WIN_PATH.txt"),
        &context,
    ));
    let crash = TerminationKind::rust_panic(
        "panic at /home/alice/CANARY_PANIC_MESSAGE",
        Some("/home/alice/CANARY_PANIC_FRAME.rs:7".to_owned()),
        vec!["\\\\server\\share\\CANARY_UNC_PATH".to_owned()],
    );
    completed(first_store.write_crash_marker(crash.clone(), &context));
    drop(first_store);

    let second_store = gpui_diagnostics::DiagnosticStore::open(&app, build.clone()).expect("store");
    let recovery = completed(second_store.recover(&context));
    assert_eq!(
        first_report.current_session_id,
        recovery.prior_session.as_ref().unwrap().session_id
    );
    assert_eq!(recovery.recovered_crash_records.len(), 1);
    assert_eq!(
        recovery.recovered_crash_records[0].source,
        CrashRecordSource::CrashMarker
    );
    let service = DiagnosticsService::from_store(second_store.clone());
    let lease = completed(service.export_bundle(&app, &context));
    assert!(lease.is_active());
    assert_eq!(lease.resource_id(), None);
    assert!(lease.bytes() > 0);
    let path = lease.path().expect("staged export path").to_path_buf();
    assert!(path.is_file());
    assert_eq!(export_files(&app).len(), 1);
    assert_no_canaries(&format!("{lease:?}"), "ExportResourceLease Debug");

    let resource = resource_for(path.clone(), lease.bytes());
    let bytes = read_resource(&resource);
    assert_eq!(bytes.len() as u64, lease.bytes());
    let json = String::from_utf8(bytes.clone()).expect("export JSON is UTF-8");
    assert_no_canaries(&json, "exported diagnostic JSON");

    let bundle: gpui_diagnostics::DiagnosticExportBundle =
        serde_json::from_slice(&bytes).expect("parse exported bundle");
    assert!(bundle.is_current_schema());
    assert_eq!(bundle.format, "gpui-box.diagnostics");
    assert_eq!(bundle.format_version, 1);
    assert_eq!(bundle.schema_version, 1);
    assert_eq!(bundle.build_identity, build);
    assert_exact_identity(&bundle.build_identity);
    assert_eq!(
        bundle.session_id,
        second_store.current_session_id().expect("current session")
    );
    assert_eq!(bundle.platform.target, PlatformTarget::current());
    assert_eq!(bundle.platform.os, std::env::consts::OS);
    assert_eq!(bundle.platform.arch, std::env::consts::ARCH);
    assert_eq!(bundle.capabilities.len(), 5);
    assert_eq!(bundle.platform_matrix.len(), 30);
    assert!(bundle.has_all_platforms());
    assert_eq!(bundle.platform_matrix, platform_support_matrix());
    assert!(!bundle.unresolved_host_adapters.is_empty());
    assert!(
        bundle
            .unresolved_host_adapters
            .iter()
            .any(|adapter| adapter.link == "#43")
    );
    assert!(!bundle.unavailable_evidence.is_empty());
    assert!(bundle.recovery.current_session_id == bundle.session_id);
    assert_eq!(bundle.recovery.recovered_crash_records.len(), 1);
    assert!(bundle.recovery.retained_events >= 1);
    assert_eq!(bundle.recovery.evicted_events, 0);
    assert_eq!(bundle.events.len(), bundle.recovery.retained_events);
    assert_eq!(
        bundle.crash_records.len(),
        bundle.recovery.retained_crash_records
    );
    assert!(
        bundle
            .crash_records
            .iter()
            .any(|record| matches!(record.termination, TerminationKind::RustPanic(_)))
    );
    let prior = bundle
        .recovery
        .prior_session
        .as_ref()
        .expect("prior session");
    assert!(!prior.clean);
    assert!(prior.crash_marker_promoted);
    assert_eq!(prior.termination, crash);
    for event in &bundle.events {
        assert_no_canaries(
            &serde_json::to_string(event).expect("event JSON"),
            "bundle event",
        );
    }
    for record in &bundle.crash_records {
        assert_no_canaries(
            &serde_json::to_string(record).expect("crash JSON"),
            "bundle crash record",
        );
        assert_no_canaries(&format!("{record:?}"), "bundle crash record Debug");
    }
    assert_no_canaries(&format!("{bundle:?}"), "DiagnosticExportBundle Debug");
    assert!(bundle.platform_matrix.iter().any(|row| {
        row.platform == PlatformTarget::Web
            && row.operation == gpui_diagnostics::DiagnosticsOperation::ExportBundle
            && !row.is_supported()
    }));
    assert!(bundle.platform_matrix.iter().any(|row| {
        row.platform == PlatformTarget::Android
            && row.operation == gpui_diagnostics::DiagnosticsOperation::WriteCrashMarker
            && row.support == gpui_diagnostics::Support::Conditional
    }));

    // A lease remains usable until released; release is idempotent.
    let mut lease = lease;
    assert!(lease.release());
    assert!(!lease.release());
    assert!(!lease.is_active());
    assert!(path.is_file(), "releasing the pin never deletes the export");
}
