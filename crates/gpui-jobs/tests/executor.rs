//! Bounded in-process CPU and I/O pools with surface generation fencing.

mod support;

use gpui_jobs::*;
use support::*;

#[test]
fn pools_reject_zero_bounds() {
    for (cpu, cpu_queue, io, io_queue) in [(0, 1, 1, 1), (1, 0, 1, 1), (1, 1, 0, 1), (1, 1, 1, 0)] {
        let failure =
            InProcessExecutor::new(cpu, cpu_queue, io, io_queue).expect_err("zero bound must fail");
        assert_eq!(failure.code, ErrorCode::InvalidConfiguration);
    }
    assert!(InProcessExecutor::new(2, 4, 4, 8).is_ok());
    assert_eq!(DEFAULT_CPU_CONCURRENCY, 2);
    assert_eq!(DEFAULT_IO_CONCURRENCY, 4);
    assert_eq!(DEFAULT_POOL_QUEUE_DEPTH, 64);
}

#[test]
fn cpu_and_io_pools_have_independent_concurrency_and_queue_bounds() {
    let executor = InProcessExecutor::new(1, 2, 2, 3).expect("executor");
    let cpu_one = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            None,
            || Ok(1u32),
        )
        .expect("cpu one");
    let cpu_two = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            None,
            || Ok(2u32),
        )
        .expect("cpu two");
    let failure = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            None,
            || Ok(3u32),
        )
        .expect_err("cpu queue is bounded");
    assert_eq!(failure.code, ErrorCode::Busy);
    assert_eq!(executor.queued_len(InProcessWorkClass::CpuBounded), 2);
    assert_eq!(executor.max_concurrency(InProcessWorkClass::CpuBounded), 1);
    assert_eq!(executor.max_queued(InProcessWorkClass::CpuBounded), 2);

    for value in [10u32, 11, 12] {
        executor
            .submit(
                InProcessWorkClass::IoBounded,
                ExecutionOwner::Background,
                &context(),
                None,
                move || Ok(value),
            )
            .expect("io task");
    }
    assert_eq!(executor.queued_len(InProcessWorkClass::IoBounded), 3);
    assert_eq!(
        executor
            .submit(
                InProcessWorkClass::IoBounded,
                ExecutionOwner::Background,
                &context(),
                None,
                || Ok(13u32),
            )
            .expect_err("io queue is bounded")
            .code,
        ErrorCode::Busy
    );

    let running_cpu = executor
        .start_next(InProcessWorkClass::CpuBounded)
        .expect("cpu running");
    let running_io_one = executor
        .start_next(InProcessWorkClass::IoBounded)
        .expect("io running one");
    let running_io_two = executor
        .start_next(InProcessWorkClass::IoBounded)
        .expect("io running two");
    assert!(
        executor
            .start_next(InProcessWorkClass::CpuBounded)
            .is_none()
    );
    assert!(executor.start_next(InProcessWorkClass::IoBounded).is_none());
    assert_eq!(executor.running_len(InProcessWorkClass::CpuBounded), 1);
    assert_eq!(executor.running_len(InProcessWorkClass::IoBounded), 2);
    assert!(running_cpu.finish().outcome.is_ok());
    assert!(running_io_one.finish().outcome.is_ok());
    assert!(running_io_two.finish().outcome.is_ok());
    let report = executor.run_ready();
    assert_eq!(report.completed, 2);
    assert_eq!(cpu_one.take_result().expect("cpu one result"), Ok(1));
    assert_eq!(cpu_two.take_result().expect("cpu two result"), Ok(2));
    assert_eq!(executor.queued_len(InProcessWorkClass::CpuBounded), 0);
    assert_eq!(executor.queued_len(InProcessWorkClass::IoBounded), 0);
}

#[test]
fn execution_owner_enforcement_rejects_ui_and_renderer_before_side_effects() {
    let executor = InProcessExecutor::new(1, 1, 1, 1).expect("executor");
    let owner = UiOwner::new(SurfaceId(4));
    let ui_context = OperationContext {
        ui: Some(owner.context(None).expect("ui")),
        ..OperationContext::default()
    };
    assert_eq!(
        ExecutionOwner::from_context(&ui_context),
        ExecutionOwner::Ui
    );
    let failure = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Ui,
            &ui_context,
            None,
            || Ok(()),
        )
        .expect_err("ui owner");
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    let failure = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &ui_context,
            None,
            || Ok(()),
        )
        .expect_err("ui context with background owner");
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    let failure = executor
        .submit(
            InProcessWorkClass::IoBounded,
            ExecutionOwner::Renderer,
            &context(),
            None,
            || Ok(()),
        )
        .expect_err("renderer owner");
    assert_eq!(failure.code, ErrorCode::InvalidInput);
    assert_eq!(executor.queued_len(InProcessWorkClass::CpuBounded), 0);
    assert_eq!(executor.queued_len(InProcessWorkClass::IoBounded), 0);
}

#[test]
fn surface_generation_disposal_cancels_queued_and_fences_running_output() {
    let executor = InProcessExecutor::new(1, 2, 1, 1).expect("executor");
    assert_eq!(
        SurfaceTaskScope::new(SurfaceId(7), 0)
            .expect_err("zero generation")
            .code,
        ErrorCode::InvalidInput
    );
    let generation_one = SurfaceTaskScope::new(SurfaceId(7), 1).expect("scope");
    let first = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(generation_one),
            || Ok(1u8),
        )
        .expect("first");
    let second = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(generation_one),
            || Ok(2u8),
        )
        .expect("second");
    let running = executor
        .start_next(InProcessWorkClass::CpuBounded)
        .expect("running");
    assert_eq!(executor.queued_len(InProcessWorkClass::CpuBounded), 1);
    let report = executor
        .dispose_surface_generation(SurfaceId(7), 1)
        .expect("dispose");
    assert_eq!(report.cancelled_queued, 1);
    assert!(executor.is_generation_disposed(SurfaceId(7), 1));
    assert!(second.take_result().expect("queued cancellation").is_err());
    assert_eq!(
        running.finish().outcome.expect_err("running fence").code,
        ErrorCode::ContextExpired
    );
    assert_eq!(
        first
            .take_result()
            .expect("running fence result")
            .expect_err("fenced")
            .code,
        ErrorCode::ContextExpired
    );
    assert_eq!(
        executor
            .dispose_surface_generation(SurfaceId(7), 0)
            .expect_err("zero generation")
            .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn newer_surface_generation_is_not_disposed_by_an_older_drop() {
    let executor = InProcessExecutor::new(1, 4, 1, 4).expect("executor");
    let first = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(SurfaceTaskScope::new(SurfaceId(8), 1).expect("scope")),
            || Ok(1u8),
        )
        .expect("gen one");
    let second = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(SurfaceTaskScope::new(SurfaceId(8), 2).expect("scope")),
            || Ok(2u8),
        )
        .expect("gen two");
    assert_eq!(executor.latest_generation(SurfaceId(8)), Some(2));
    let failure = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(SurfaceTaskScope::new(SurfaceId(8), 1).expect("scope")),
            || Ok(3u8),
        )
        .expect_err("stale generation");
    assert_eq!(failure.code, ErrorCode::ContextExpired);
    let failure = executor
        .dispose_surface_generation(SurfaceId(8), 1)
        .expect_err("stale disposal");
    assert_eq!(failure.code, ErrorCode::ContextExpired);
    assert!(!executor.is_generation_disposed(SurfaceId(8), 2));
    let report = executor.run_ready();
    assert_eq!(report.completed, 2);
    assert_eq!(first.take_result().expect("gen one").expect("ok"), 1);
    assert_eq!(second.take_result().expect("gen two").expect("ok"), 2);
}

#[test]
fn dispose_surface_reports_latest_generation_and_all_surface_generations() {
    let executor = InProcessExecutor::new(1, 4, 1, 4).expect("executor");
    let handle = executor
        .submit(
            InProcessWorkClass::IoBounded,
            ExecutionOwner::Background,
            &context(),
            Some(SurfaceTaskScope::new(SurfaceId(9), 3).expect("scope")),
            || Ok(5u8),
        )
        .expect("task");
    let report = executor.dispose_surface(SurfaceId(9));
    assert_eq!(report.surface, SurfaceId(9));
    assert_eq!(report.generation, 3);
    assert_eq!(report.cancelled_queued, 1);
    assert!(handle.take_result().expect("cancelled").is_err());
    assert_eq!(executor.latest_generation(SurfaceId(10)), None);
    assert_eq!(executor.dispose_surface(SurfaceId(10)).generation, 0);
}

#[test]
fn task_results_are_consumed_once_and_errors_are_preserved() {
    let executor = InProcessExecutor::new(1, 1, 1, 1).expect("executor");
    let ok = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            None,
            || Ok(42u32),
        )
        .expect("ok");
    executor.run_ready();
    assert!(ok.is_finished());
    assert_eq!(ok.take_result(), Some(Ok(42)));
    assert_eq!(ok.take_result(), None);

    let failed = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            None,
            || -> Result<u32, SystemError> {
                Err(SystemError::new(
                    ErrorCode::PlatformFailure,
                    "bounded task failed",
                ))
            },
        )
        .expect("failed");
    executor.run_ready();
    assert_eq!(
        failed
            .take_result()
            .expect("result")
            .expect_err("error")
            .code,
        ErrorCode::PlatformFailure
    );
}

#[test]
fn disposal_before_submission_advances_latest_generation() {
    let executor = InProcessExecutor::new(1, 1, 1, 1).expect("executor");
    let surface = SurfaceId(55);
    assert_eq!(executor.latest_generation(surface), None);
    let report = executor
        .dispose_surface_generation(surface, 3)
        .expect("dispose empty surface");
    assert_eq!(report.cancelled_queued, 0);
    assert_eq!(executor.latest_generation(surface), Some(3));
    assert!(executor.is_generation_disposed(surface, 3));
    assert_eq!(
        executor
            .submit(
                InProcessWorkClass::CpuBounded,
                ExecutionOwner::Background,
                &context(),
                Some(SurfaceTaskScope::new(surface, 3).expect("scope")),
                || Ok(()),
            )
            .expect_err("disposed generation")
            .code,
        ErrorCode::ContextExpired
    );
    let handle = executor
        .submit(
            InProcessWorkClass::CpuBounded,
            ExecutionOwner::Background,
            &context(),
            Some(SurfaceTaskScope::new(surface, 4).expect("scope")),
            || Ok(4u8),
        )
        .expect("newer generation");
    assert_eq!(executor.latest_generation(surface), Some(4));
    executor.run_ready();
    assert_eq!(handle.take_result().expect("result").expect("ok"), 4);
}
