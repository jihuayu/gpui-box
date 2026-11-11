//! Facade and cross-service integration tests for persistent background jobs.
//!
//! These tests are deterministic and use no sleeps. They open real private
//! application storage in a temporary directory, exercise power/network/
//! lifecycle/permission inputs and verify checkpoint recovery after restart.

#![cfg(feature = "jobs")]

use gpui_box::jobs::*;
use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

fn context() -> OperationContext {
    OperationContext::default()
}

fn completed<T>(result: ServiceResult<T>) -> T {
    match result.expect("operation succeeded") {
        Completion::Completed(value) => value,
        Completion::Cancelled(reason) => panic!("unexpected cancellation: {reason:?}"),
    }
}

fn key(name: &str) -> JobKey {
    JobKey::new(JobNamespace::application(), name).expect("job key")
}

fn observed_power(charging: ChargingState, saver: LowPowerMode) -> PowerSnapshot {
    PowerSnapshot {
        power_source: PowerSourceStatus::new(
            if matches!(charging, ChargingState::Charging | ChargingState::Full) {
                PowerSource::Ac
            } else {
                PowerSource::Battery
            },
            PowerSignalSource::Observed,
        ),
        battery: BatteryStatus::new(Some(80), charging, saver, None, PowerSignalSource::Observed)
            .expect("battery"),
        sleep_resume: gpui_box::power::SleepResumeStatus::new(
            gpui_box::power::SleepResumeState::Awake,
            1,
            PowerSignalSource::Observed,
        ),
        session_lock: gpui_box::power::SessionLockStatus::new(
            gpui_box::power::SessionLockState::Unlocked,
            PowerSignalSource::Observed,
        ),
        thermal: ThermalStatus::new(ThermalCondition::Nominal, PowerSignalSource::Observed),
        memory: gpui_box::power::MemoryStatus::new(
            gpui_box::power::MemoryCondition::Normal,
            PowerSignalSource::Observed,
        ),
        revision: 1,
    }
}

fn observed_network() -> NetworkSnapshot {
    NetworkSnapshot::routable(TransportKind::WiFi, false, true, true).expect("network")
}

fn foreground_lifecycle() -> LifecycleState {
    LifecycleState::from_parts(
        AppPhase::ForegroundActive,
        Vec::new(),
        gpui_box::lifecycle::SessionLockState::Unlocked,
        gpui_box::lifecycle::MemoryPressure::Normal,
    )
}

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("gpui-box-jobs-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn open_service(app: &AppData, provider: FakeJobProvider, clock: Arc<ManualClock>) -> JobsService {
    JobsService::open_with_options(
        app,
        Arc::new(provider),
        clock as Arc<dyn JobsClock>,
        DEFAULT_JOBS_EVENT_CAPACITY,
        Arc::new(NoopJobsStorageHook),
    )
    .expect("jobs service")
}

#[test]
fn jobs_facade_exposes_the_coordinated_contracts() {
    assert_eq!(
        gpui_box::jobs::UPSTREAM_GPUI_REVISION,
        gpui_box::UPSTREAM_REVISION
    );
    assert_eq!(gpui_box::jobs::PACKAGE_VERSION, gpui_box::VERSION);
    assert_eq!(JobsOperation::ALL.len(), 6);
    assert_eq!(PlatformTarget::ALL.len(), 6);
    assert_eq!(platform_support_matrix().len(), 36);
    assert_eq!(DEFAULT_JOBS_EVENT_CAPACITY, 64);
    assert_eq!(PROVIDER_MANIFEST.len(), 5);
    assert!(!durable_scheduling_available(PlatformTarget::Linux));
    assert!(
        platform_support_matrix()
            .iter()
            .all(|row| row.evidence == Evidence::ContractOnly)
    );
    assert_eq!(
        operation_support(PlatformTarget::Windows, JobsOperation::ScheduleDurableJob).support,
        Support::Native
    );
}

#[test]
fn durable_job_uses_app_data_and_recovers_its_checkpoint() {
    let directory = TestDirectory::new("recover");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let job = key("export-report");
    let checkpoint_payload = b"rows:1200".to_vec();
    let token;
    {
        let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
        let service = open_service(&app, provider.clone(), Arc::clone(&clock));
        completed(service.apply_power_snapshot(
            observed_power(ChargingState::Charging, LowPowerMode::Disabled),
            &context(),
        ));
        completed(service.apply_network_snapshot(observed_network(), &context()));
        completed(service.apply_lifecycle_state(&foreground_lifecycle(), &context()));
        completed(service.apply_permission_kind(
            PermissionKind::Notifications,
            PermissionStatus::Granted,
            &context(),
        ));
        completed(service.apply_device_state(true, false, &context()));
        let receipt = completed(
            service.register_job(
                &context(),
                job.clone(),
                JobConstraints::default()
                    .with_network(JobNetworkRequirement::Connected)
                    .requiring_charging(true)
                    .requiring_device_idle(true)
                    .requiring_storage_not_low(true)
                    .requiring_permission(PermissionKind::Notifications)
                    .with_execution_budget_ms(10_000)
                    .with_deadline_ms(60_000),
                ExistingJobPolicy::KeepExisting,
            ),
        );
        assert_eq!(receipt.status, JobStatus::Scheduled);
        token = match completed(service.handle_wakeup(&job, &context())) {
            WakeupOutcome::Started(token) => token,
            other => panic!("expected started, got {other:?}"),
        };
        completed(service.save_checkpoint(
            &token,
            checkpoint_payload.clone(),
            JobProgress::from_basis_points(4_000).expect("progress"),
            &context(),
        ));
        completed(service.report_progress(
            &token,
            JobProgress::units(4, Some(10)).expect("progress"),
            &context(),
        ));
        completed(service.complete_job(&token, &context()));
        assert_eq!(service.job_status(&job), Some(JobStatus::Succeeded));
    }
    {
        let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
        let service = open_service(&app, provider, clock);
        let report = completed(service.reconcile_after_restart(&context()));
        assert_eq!(report.jobs_scanned, 1);
        assert_eq!(report.running_reconciled, 0);
        assert_eq!(service.job_status(&job), Some(JobStatus::Succeeded));
        assert_eq!(
            service.checkpoint(&job).expect("checkpoint").payload,
            checkpoint_payload
        );
        // A completed job retains its last committed checkpoint but cannot be
        // restarted by an old attempt token.
        assert_eq!(
            service
                .complete_job(&token, &context())
                .expect_err("terminal fence")
                .code,
            ErrorCode::ContextExpired
        );
        let record = service.job_record(&job).expect("record");
        assert_eq!(record.progress.basis_points(), 10_000);
        assert_eq!(record.run_epoch, token.run_epoch);
    }
}

#[test]
fn cross_service_constraint_loss_preempts_and_reschedules() {
    let directory = TestDirectory::new("constraints");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
    let service = open_service(&app, provider.clone(), clock);
    completed(service.apply_power_snapshot(
        observed_power(ChargingState::Charging, LowPowerMode::Disabled),
        &context(),
    ));
    completed(service.apply_network_snapshot(observed_network(), &context()));
    let job = key("sync-calendar");
    completed(
        service.register_job(
            &context(),
            job.clone(),
            JobConstraints::default()
                .with_network(JobNetworkRequirement::Connected)
                .requiring_charging(true),
            ExistingJobPolicy::KeepExisting,
        ),
    );
    let token = match completed(service.handle_wakeup(&job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    };
    let subscription = service.subscribe();
    completed(service.apply_power_snapshot(
        observed_power(ChargingState::Discharging, LowPowerMode::Disabled),
        &context(),
    ));
    let events = {
        let mut events = Vec::new();
        while let Ok(mut next) = subscription.try_next() {
            let waker = std::task::Waker::noop();
            let mut task = std::task::Context::from_waker(waker);
            match std::pin::Pin::new(&mut next).poll(&mut task) {
                std::task::Poll::Ready(Some(envelope)) => match envelope.payload {
                    Event::Value(value) => events.push(value),
                    Event::ResyncRequired => {}
                },
                std::task::Poll::Ready(None) | std::task::Poll::Pending => break,
            }
        }
        events
    };
    assert!(events.iter().any(|event| matches!(
        event,
        JobsEvent::PreemptionRequested {
            reason: PreemptReason::ConstraintsLost,
            ..
        }
    )));
    completed(service.save_checkpoint(
        &token,
        b"final".to_vec(),
        JobProgress::from_basis_points(3_000).expect("progress"),
        &context(),
    ));
    assert_eq!(
        completed(service.ack_preemption(&token, &context())).status,
        JobStatus::PendingConstraints
    );
    assert_eq!(
        service
            .complete_job(&token, &context())
            .expect_err("superseded")
            .code,
        ErrorCode::ContextExpired
    );
    completed(service.apply_power_snapshot(
        observed_power(ChargingState::Charging, LowPowerMode::Disabled),
        &context(),
    ));
    assert_eq!(service.job_status(&job), Some(JobStatus::Scheduled));
    let resumed = match completed(service.handle_wakeup(&job, &context())) {
        WakeupOutcome::Started(token) => token,
        other => panic!("expected started, got {other:?}"),
    };
    assert!(resumed.run_epoch > token.run_epoch);
    assert_eq!(
        service.checkpoint(&job).expect("checkpoint").payload,
        b"final"
    );
    completed(service.complete_job(&resumed, &context()));
    assert_eq!(provider.schedule_request_count(), 2);
}

#[test]
fn missing_databases_fall_back_to_pending_not_fake_scheduling() {
    let directory = TestDirectory::new("missing");
    let clock = Arc::new(ManualClock::new(1_000));
    let provider = FakeJobProvider::new();
    let app = AppData::open(DirectoryRoots::under(directory.path())).expect("app data");
    let service = open_service(&app, provider.clone(), clock);
    let job = key("needs-network");
    let receipt = completed(service.register_job(
        &context(),
        job,
        JobConstraints::default().with_network(JobNetworkRequirement::Unmetered),
        ExistingJobPolicy::KeepExisting,
    ));
    assert_eq!(receipt.status, JobStatus::PendingConstraints);
    assert!(
        receipt
            .unmet_constraints
            .iter()
            .any(|unmet| matches!(unmet, UnmetConstraint::NetworkUnknown))
    );
    assert_eq!(provider.schedule_request_count(), 0);
}

#[cfg(feature = "jobs-gpui")]
#[test]
fn gpui_adapter_registers_the_service_and_scopes_a_surface() {
    use gpui_box::jobs::gpui_adapter;
    let service = JobsService::in_memory(Arc::new(FakeJobProvider::new())).expect("service");
    let mut services = SystemServices::new();
    gpui_adapter::install_jobs_in_services(&mut services, service.clone()).expect("register");
    assert!(
        services
            .service::<JobsService>()
            .is_ok_and(|registered| service.shares_state_with(&registered))
    );
    let owner = UiOwner::new(SurfaceId(77));
    let scope = gpui_adapter::SurfaceJobsScope::new(owner.clone(), service);
    assert_eq!(scope.surface(), SurfaceId(77));
    assert_eq!(scope.generation(), 1);
    assert!(scope.context().is_ok());
    drop(scope);
    assert!(owner.context(None).is_err());
    assert!(std::mem::size_of::<gpui_adapter::JobsModel>() > 0);
}
