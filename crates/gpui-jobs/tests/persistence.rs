//! Durable record, checkpoint, integrity and restart persistence.

mod support;

use gpui_jobs::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
use support::*;
use tempfile::TempDir;

fn app_at(temp: &TempDir) -> AppData {
    AppData::open(DirectoryRoots::under(temp.path())).expect("app data")
}

fn open_service(
    app: &AppData,
    provider: FakeJobProvider,
    clock: Arc<ManualClock>,
    hook: Arc<JobsStorageFaultInjection>,
) -> JobsService {
    JobsService::open_with_options(
        app,
        Arc::new(provider),
        clock,
        DEFAULT_JOBS_EVENT_CAPACITY,
        hook,
    )
    .expect("jobs service")
}

fn files_with_prefix(directory: &Path, prefix: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn start_attempt(service: &JobsService, job: &JobKey) -> FencingToken {
    match completed(service.handle_wakeup(job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected a started attempt, got {other:?}"),
    }
}

#[test]
fn ui_and_renderer_owners_are_rejected_before_storage() {
    let clock = Arc::new(ManualClock::new(1_000));
    let service = service_with(FakeJobProvider::new(), clock);
    let owner = UiOwner::new(SurfaceId(1));
    let ui = OperationContext {
        ui: Some(owner.context(None).expect("ui context")),
        ..OperationContext::default()
    };
    assert_eq!(ExecutionOwner::from_context(&ui), ExecutionOwner::Ui);
    assert_eq!(
        ExecutionOwner::from_context(&context()),
        ExecutionOwner::Background
    );
    let failure = service
        .register_job(
            &ui,
            key("ui-rejected"),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        )
        .expect_err("ui context is rejected");
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    assert!(service.list_jobs().is_empty());
    assert!(
        ExecutionOwner::Renderer
            .require_background(&context())
            .is_err()
    );
    assert!(ExecutionOwner::Ui.require_background(&context()).is_err());
    assert!(
        ExecutionOwner::Background
            .require_background(&context())
            .is_ok()
    );
    assert!(owner.context(None).is_ok());
}

#[test]
fn checkpoint_round_trip_and_corrupt_latest_falls_back() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let hook = Arc::new(JobsStorageFaultInjection::new());
    let job = key("checkpoint-fallback");
    let namespace;
    let token;
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            provider.clone(),
            Arc::clone(&clock),
            Arc::clone(&hook),
        );
        namespace = service.namespace_path().to_path_buf();
        let subscription = service.subscribe();
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        token = start_attempt(&service, &job);
        completed(service.save_checkpoint(
            &token,
            b"first-payload".to_vec(),
            JobProgress::from_basis_points(1_000).expect("progress"),
            &context(),
        ));
        assert!(
            values(drain(&subscription))
                .iter()
                .any(|event| matches!(event, JobsEvent::CheckpointCommitted { sequence: 1, .. }))
        );

        hook.corrupt_next_write();
        completed(service.save_checkpoint(
            &token,
            b"second-payload".to_vec(),
            JobProgress::from_basis_points(2_000).expect("progress"),
            &context(),
        ));
        assert_eq!(
            service.checkpoint(&job).expect("checkpoint").sequence,
            2,
            "the in-memory commit is reported even when the disk slot is corrupt"
        );
    }

    {
        let app = app_at(&temp);
        let service = open_service(&app, provider, Arc::clone(&clock), Arc::clone(&hook));
        let subscription = service.subscribe();
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.running_reconciled, 1);
        assert!(report.checkpoints_recovered >= 1);
        assert!(report.corrupt_records_quarantined >= 1);
        let checkpoint = service.checkpoint(&job).expect("fallback checkpoint");
        assert_eq!(checkpoint.sequence, 1);
        assert_eq!(checkpoint.payload, b"first-payload");
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.checkpoint.expect("summary").sequence, 1);
        assert!(record.run_epoch > token.run_epoch);
        let events = values(drain(&subscription));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, JobsEvent::CheckpointRecovered { sequence: 1, .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, JobsEvent::CorruptRecordQuarantined { .. }))
        );
        let quarantine = service.namespace_path().join(QUARANTINE_DIR);
        assert!(
            fs::read_dir(quarantine)
                .expect("quarantine directory")
                .next()
                .is_some()
        );

        // The pre-restart fence is stale and cannot overwrite the recovered state.
        let stale_checkpoint = service.save_checkpoint(
            &token,
            b"stale".to_vec(),
            JobProgress::default(),
            &context(),
        );
        assert_eq!(
            stale_checkpoint.expect_err("stale fence").code,
            ErrorCode::ContextExpired
        );
        let stale_completion = service.complete_job(&token, &context());
        assert_eq!(
            stale_completion.expect_err("stale fence").code,
            ErrorCode::ContextExpired
        );
        assert_eq!(
            service.checkpoint(&job).expect("checkpoint").payload,
            b"first-payload"
        );
    }
    assert!(!files_with_prefix(&namespace, "checkpoint-").is_empty());
}

#[test]
fn truncated_latest_checkpoint_recovers_the_previous_commit() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let hook = Arc::new(JobsStorageFaultInjection::new());
    let job = key("checkpoint-truncated");
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            provider.clone(),
            Arc::clone(&clock),
            Arc::clone(&hook),
        );
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        let token = start_attempt(&service, &job);
        completed(service.save_checkpoint(
            &token,
            b"one".to_vec(),
            JobProgress::from_basis_points(1_000).expect("progress"),
            &context(),
        ));
        completed(service.save_checkpoint(
            &token,
            b"two".to_vec(),
            JobProgress::from_basis_points(2_000).expect("progress"),
            &context(),
        ));
        hook.truncate_next_write_to(24);
        completed(service.save_checkpoint(
            &token,
            b"three".to_vec(),
            JobProgress::from_basis_points(3_000).expect("progress"),
            &context(),
        ));
    }
    {
        let app = app_at(&temp);
        let service = open_service(&app, provider, clock, hook);
        let report = completed(service.reconcile_after_restart(&context()));
        assert!(report.checkpoints_recovered >= 1);
        let checkpoint = service.checkpoint(&job).expect("checkpoint");
        assert_eq!(checkpoint.sequence, 2);
        assert_eq!(checkpoint.payload, b"two");
    }
}

#[test]
fn corrupt_job_record_is_quarantined_without_panicking() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            FakeJobProvider::new(),
            Arc::clone(&clock),
            Arc::new(JobsStorageFaultInjection::new()),
        );
        let job = key("corrupt-record");
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        let job_file = files_with_prefix(service.namespace_path(), "job-")
            .into_iter()
            .next()
            .expect("job file");
        fs::write(job_file, b"{not-json").expect("corrupt write");
    }
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            FakeJobProvider::new(),
            clock,
            Arc::new(JobsStorageFaultInjection::new()),
        );
        let subscription = service.subscribe();
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 0);
        assert!(report.corrupt_records_quarantined >= 1);
        assert!(service.list_jobs().is_empty());
        assert!(
            values(drain(&subscription))
                .iter()
                .any(|event| matches!(event, JobsEvent::CorruptRecordQuarantined { .. }))
        );
        let quarantine = service.namespace_path().join(QUARANTINE_DIR);
        assert!(
            fs::read_dir(quarantine)
                .expect("quarantine directory")
                .next()
                .is_some()
        );
    }
}

#[test]
fn checksum_mismatch_and_schema_mismatch_are_quarantined() {
    for mutation in ["payload", "schema"] {
        let temp = TempDir::new().expect("temp");
        let clock = Arc::new(ManualClock::new(1_000));
        {
            let app = app_at(&temp);
            let service = open_service(
                &app,
                FakeJobProvider::new(),
                Arc::clone(&clock),
                Arc::new(JobsStorageFaultInjection::new()),
            );
            completed(service.register_job(
                &context(),
                key("tampered"),
                JobConstraints::default(),
                ExistingJobPolicy::KeepExisting,
            ));
            let job_file = files_with_prefix(service.namespace_path(), "job-")
                .into_iter()
                .next()
                .expect("job file");
            let mut envelope: serde_json::Value =
                serde_json::from_slice(&fs::read(&job_file).expect("read")).expect("json");
            if mutation == "payload" {
                envelope["payload"]["status"] = serde_json::Value::String("succeeded".into());
            } else {
                envelope["schema_version"] = serde_json::Value::from(999);
            }
            fs::write(&job_file, serde_json::to_vec(&envelope).expect("encode")).expect("write");
        }
        let app = app_at(&temp);
        let service = open_service(
            &app,
            FakeJobProvider::new(),
            clock,
            Arc::new(JobsStorageFaultInjection::new()),
        );
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 0, "mutation {mutation}");
        assert!(
            report.corrupt_records_quarantined >= 1,
            "mutation {mutation}"
        );
        assert!(service.list_jobs().is_empty());
    }
}

#[test]
fn oversized_checkpoint_is_rejected_before_any_write() {
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let service = service_with(provider.clone(), Arc::clone(&clock));
    let job = key("oversized-checkpoint");
    completed(service.register_job(
        &context(),
        job.clone(),
        JobConstraints::default(),
        ExistingJobPolicy::KeepExisting,
    ));
    let token = start_attempt(&service, &job);
    let failure = service
        .save_checkpoint(
            &token,
            vec![7u8; MAX_CHECKPOINT_BYTES + 1],
            JobProgress::default(),
            &context(),
        )
        .expect_err("oversized");
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    assert!(service.checkpoint(&job).is_none());
    let checkpoint = completed(service.save_checkpoint(
        &token,
        b"small".to_vec(),
        JobProgress::default(),
        &context(),
    ));
    assert_eq!(checkpoint.sequence, 1);
    assert!(
        JobCheckpoint::new(
            job,
            1,
            1,
            1,
            JobProgress::default(),
            vec![0u8; MAX_CHECKPOINT_BYTES + 1],
            1,
        )
        .is_err()
    );
}

#[test]
fn durable_registration_survives_a_clean_restart() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("durable");
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            provider.clone(),
            Arc::clone(&clock),
            Arc::new(JobsStorageFaultInjection::new()),
        );
        completed(service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        assert_eq!(provider.schedule_request_count(), 1);
    }
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            provider.clone(),
            clock,
            Arc::new(JobsStorageFaultInjection::new()),
        );
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.rescheduled, 1);
        assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
        assert_eq!(provider.schedule_request_count(), 2);
        assert_eq!(service.job_record(&job).expect("record").attempt, 0);
    }
}

fn envelope_checksum(payload_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gpui-jobs-envelope\0");
    hasher.update(JOBS_FORMAT_VERSION.to_le_bytes());
    hasher.update(payload_bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn semantically_invalid_record_with_valid_envelope_is_quarantined() {
    let temp = TempDir::new().expect("temp");
    let clock = Arc::new(ManualClock::new(1_000));
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            FakeJobProvider::new(),
            Arc::clone(&clock),
            Arc::new(JobsStorageFaultInjection::new()),
        );
        completed(service.register_job(
            &context(),
            key("semantic-tamper"),
            JobConstraints::default(),
            ExistingJobPolicy::KeepExisting,
        ));
        let job_file = files_with_prefix(service.namespace_path(), "job-")
            .into_iter()
            .next()
            .expect("job file");
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&fs::read(&job_file).expect("read")).expect("json");
        let mut record: JobRecord =
            serde_json::from_value(envelope["payload"].clone()).expect("record");
        record.constraints.execution_budget_ms = 0;
        let payload = serde_json::to_vec(&record).expect("payload");
        envelope["payload"] = serde_json::to_value(&record).expect("value");
        envelope["schema_version"] = serde_json::Value::from(JOBS_FORMAT_VERSION);
        envelope["checksum"] = serde_json::Value::String(envelope_checksum(&payload));
        fs::write(&job_file, serde_json::to_vec(&envelope).expect("envelope"))
            .expect("write tampered record");
    }
    {
        let app = app_at(&temp);
        let service = open_service(
            &app,
            FakeJobProvider::new(),
            clock,
            Arc::new(JobsStorageFaultInjection::new()),
        );
        let subscription = service.subscribe();
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 0);
        assert!(report.corrupt_records_quarantined >= 1);
        assert!(service.list_jobs().is_empty());
        assert!(
            values(drain(&subscription))
                .iter()
                .any(|event| matches!(event, JobsEvent::CorruptRecordQuarantined { .. }))
        );
    }
}
