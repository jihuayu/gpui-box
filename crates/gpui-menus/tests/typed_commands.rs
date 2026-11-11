mod common;

use common::*;
use gpui_menus::{
    CancelReason, CommandDescriptor, CommandHandle, CommandId, CommandPayload, CommandTaskState,
    Completion, ConcurrencyPolicy, ErrorCode, ExecutionDomain, FakeMenuProvider, InvocationContext,
    MenuCommandEvent, MenuCommandService, OperationContext, SurfaceId, UiCommandCompletion,
    UiCommandOutcome, WorkerPoolConfig,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

fn command(value: &str) -> CommandId {
    CommandId::new(value).expect("valid command id")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Echo {
    value: u32,
}

#[test]
fn ui_commands_require_a_live_ui_context_on_the_owning_thread() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let background =
        CommandDescriptor::background(command("worker.run"), "Run").expect("descriptor");
    assert!(
        service.register_ui(background, |_: (), _| Ok(())).is_err(),
        "register_ui must reject a background descriptor"
    );

    let counter = Rc::new(RefCell::new(0_u32));
    let captured = counter.clone();
    let descriptor =
        CommandDescriptor::ui(command("editor.increment"), "Increment").expect("descriptor");
    let registration = service
        .register_ui(descriptor, move |value: Echo, _| {
            *captured.borrow_mut() += 1;
            Ok(value.value + 1)
        })
        .expect("registration");
    assert_eq!(registration.handle().domain(), ExecutionDomain::Ui);

    let missing = block_on(service.invoke_typed(
        registration.handle(),
        Echo { value: 1 },
        InvocationContext::default(),
    ));
    assert_eq!(
        missing.expect_err("missing UI context").code,
        ErrorCode::InvalidInput
    );

    let (owner, context) = ui_context(91);
    let completed =
        block_on(service.invoke_typed(registration.handle(), Echo { value: 41 }, context.clone()));
    assert_eq!(completed.expect("typed result"), Completion::Completed(42));
    assert_eq!(*counter.borrow(), 1);

    owner.advance_generation().expect("advance owner");
    let stale = block_on(service.invoke_typed(registration.handle(), Echo { value: 1 }, context));
    assert_eq!(
        stale.expect_err("stale owner").code,
        ErrorCode::ContextExpired
    );

    let (owner, context) = ui_context(92);
    service
        .advance_surface_generation(SurfaceId(92))
        .expect("advance service surface");
    let stale = block_on(service.invoke_typed(registration.handle(), Echo { value: 1 }, context));
    assert_eq!(
        stale.expect_err("stale service surface").code,
        ErrorCode::ContextExpired
    );
    drop(owner);
}

#[test]
fn foreign_thread_ui_context_is_rejected_before_the_handler_runs() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let descriptor = CommandDescriptor::ui(command("window.close"), "Close").expect("descriptor");
    let runs = Rc::new(RefCell::new(0_u32));
    let captured = runs.clone();
    let registration = service
        .register_ui(descriptor, move |_: (), _| {
            *captured.borrow_mut() += 1;
            Ok(())
        })
        .expect("registration");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let owner = gpui_menus::UiOwner::new(SurfaceId(700));
        let context = owner.context(None).expect("foreign context");
        sender.send((owner, context)).expect("send context");
    });
    let (_owner, foreign) = receiver.recv().expect("foreign context received");
    let operation = OperationContext {
        ui: Some(foreign),
        ..OperationContext::default()
    };
    let result = block_on(service.invoke_typed(
        registration.handle(),
        (),
        InvocationContext::new(operation),
    ));
    assert_eq!(
        result.expect_err("wrong-thread context").code,
        ErrorCode::ContextExpired
    );
    assert_eq!(*runs.borrow(), 0);
}

#[test]
fn deferred_ui_completion_keeps_running_until_the_completion_port_resolves() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let slot: Rc<RefCell<Option<UiCommandCompletion<u64>>>> = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let descriptor =
        CommandDescriptor::ui(command("editor.deferred"), "Deferred").expect("descriptor");
    let registration = service
        .register_ui_deferred(descriptor, move |_value: u64, _| {
            let (outcome, completion) = UiCommandOutcome::<u64>::deferred();
            *captured.borrow_mut() = Some(completion);
            Ok(outcome)
        })
        .expect("registration");
    let (owner, context) = ui_context(93);
    let subscription = service.subscribe();
    let mut request = service.invoke_typed(registration.handle(), 7, context.clone());
    let started = next_event(&subscription);
    let MenuCommandEvent::CommandStarted(started) = started else {
        panic!("expected started event");
    };
    assert_eq!(started.state, CommandTaskState::Running);
    assert!(poll_once_ref(&mut request).is_none());
    assert!(
        poll_once(
            subscription
                .try_next()
                .expect("only one event reader is active")
        )
        .is_none(),
        "CommandFinished must not be emitted while the completion port is pending"
    );
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Running
    );

    let completion = slot.borrow().as_ref().expect("completion").clone();
    assert!(completion.complete(8));
    assert_eq!(
        block_on(request).expect("deferred result"),
        Completion::Completed(8)
    );
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("expected finished event");
    };
    assert_eq!(finished.id, started.id);
    assert_eq!(finished.state, CommandTaskState::Completed);
    drop(owner);
}

#[test]
fn background_worker_slots_are_bounded_and_report_busy() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(1, 0).expect("config"),
    )
    .expect("service");
    let descriptor = CommandDescriptor::new(
        command("worker.slow"),
        "Slow",
        ExecutionDomain::Background,
        ConcurrencyPolicy::AllowParallel { max_in_flight: 2 },
    )
    .expect("descriptor");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(descriptor, move |_: (), _| {
            let _ = started_tx.send(());
            release_rx.lock().expect("gate").recv().expect("release");
            Ok(1_u32)
        })
        .expect("registration");

    let first = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("worker started");
    let second =
        block_on(service.invoke_typed(registration.handle(), (), InvocationContext::default()));
    assert_eq!(second.expect_err("pool saturated").code, ErrorCode::Busy);
    assert_eq!(
        service
            .tasks()
            .into_iter()
            .filter(|task| task.state == CommandTaskState::Running)
            .count(),
        1
    );
    release_tx.send(()).expect("release first");
    assert_eq!(
        block_on(first).expect("first result"),
        Completion::Completed(1)
    );
}

#[test]
fn replace_in_flight_cancels_the_superseded_invocation() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(1, 1).expect("config"),
    )
    .expect("service");
    let descriptor = CommandDescriptor::new(
        command("worker.replace"),
        "Replace",
        ExecutionDomain::Background,
        ConcurrencyPolicy::ReplaceInFlight,
    )
    .expect("descriptor");
    let (started_tx, _started_rx) = mpsc::channel::<gpui_menus::InvocationId>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(descriptor, move |_: (), context| {
            let _ = started_tx.send(context.invocation);
            release_rx.lock().expect("gate").recv().expect("release");
            Ok(2_u32)
        })
        .expect("registration");

    let subscription = service.subscribe();
    let first = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    let MenuCommandEvent::CommandStarted(first_started) = next_event(&subscription) else {
        panic!("first start");
    };
    let second = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    let MenuCommandEvent::CommandFinished(superseded) = next_event(&subscription) else {
        panic!("superseded finished");
    };
    assert_eq!(superseded.id, first_started.id);
    assert_eq!(
        superseded.state,
        CommandTaskState::Cancelled(CancelReason::ProcessPolicy)
    );
    let MenuCommandEvent::CommandStarted(second_started) = next_event(&subscription) else {
        panic!("second start");
    };
    assert_eq!(
        service
            .command_task(second_started.id)
            .expect("second task")
            .state,
        CommandTaskState::Running
    );
    assert_eq!(
        block_on(first).expect("cancelled result"),
        Completion::Cancelled(CancelReason::ProcessPolicy)
    );

    release_tx.send(()).expect("release one");
    release_tx.send(()).expect("release two");
    assert_eq!(
        block_on(second).expect("second result"),
        Completion::Completed(2)
    );
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("second finished");
    };
    assert_eq!(finished.id, second_started.id);
    assert_eq!(finished.state, CommandTaskState::Completed);
}

#[test]
fn background_deadline_expiry_is_explicit_and_terminal() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(2, 2).expect("config"),
    )
    .expect("service");
    let descriptor =
        CommandDescriptor::background(command("worker.deadline"), "Deadline").expect("descriptor");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(descriptor, move |_: (), _| {
            let _ = started_tx.send(());
            release_rx.lock().expect("gate").recv().expect("release");
            Ok(1_u32)
        })
        .expect("registration");
    let deadline = Instant::now() + Duration::from_secs(60);
    let operation = OperationContext {
        deadline: Some(deadline),
        ..OperationContext::default()
    };
    let subscription = service.subscribe();
    let request =
        service.invoke_typed(registration.handle(), (), InvocationContext::new(operation));
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("worker started");
    assert_eq!(service.expire_invocations_at(deadline), 1);
    assert_eq!(
        block_on(request).expect_err("deadline").code,
        ErrorCode::TimedOut
    );
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Failed(ErrorCode::TimedOut)
    );
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("finished");
    };
    assert_eq!(finished.id, started.id);
    assert_eq!(
        finished.state,
        CommandTaskState::Failed(ErrorCode::TimedOut)
    );
    release_tx.send(()).expect("release worker");
}

#[test]
fn cancellation_token_reaches_a_running_background_handler() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(1, 0).expect("config"),
    )
    .expect("service");
    let descriptor =
        CommandDescriptor::background(command("worker.cancel"), "Cancel").expect("descriptor");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (observed_tx, observed_rx) = mpsc::channel::<CancelReason>();
    let registration = service
        .register_background(descriptor, move |_: (), context| {
            let _ = started_tx.send(());
            while context.cancellation.reason().is_none() {
                std::thread::yield_now();
            }
            let reason = context.cancellation.reason().expect("reason");
            let _ = observed_tx.send(reason);
            Ok(0_u8)
        })
        .expect("registration");
    let subscription = service.subscribe();
    let request = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("handler started");
    assert!(service.cancel_invocation(started.id, CancelReason::User));
    assert_eq!(
        block_on(request).expect("cancelled request"),
        Completion::Cancelled(CancelReason::User)
    );
    assert_eq!(
        observed_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("handler observed cancellation"),
        CancelReason::User
    );
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Cancelled(CancelReason::User)
    );
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("finished");
    };
    assert_eq!(finished.id, started.id);
}

#[test]
fn typed_and_dynamic_invocation_share_the_payload_bridge() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let descriptor = CommandDescriptor::ui(command("echo.value"), "Echo").expect("descriptor");
    let registration = service
        .register_ui(descriptor, |input: Echo, _| Ok(input.value * 2))
        .expect("registration");
    let (_owner, context) = ui_context(94);
    assert_eq!(
        block_on(service.invoke_typed(registration.handle(), Echo { value: 5 }, context.clone(),))
            .expect("typed"),
        Completion::Completed(10)
    );
    assert_eq!(
        block_on(service.invoke_command(
            &command("echo.value"),
            CommandPayload::json(serde_json::json!({"value": 6})),
            context,
        ))
        .expect("dynamic"),
        Completion::Completed(CommandPayload::json(12))
    );
}

#[test]
fn typed_handle_is_constructible_for_lookup_only() {
    let handle = CommandHandle::<Echo, u32>::new(command("echo.value"), ExecutionDomain::Ui);
    assert_eq!(handle.id(), &command("echo.value"));
    assert_eq!(handle.domain(), ExecutionDomain::Ui);
}

#[test]
fn concurrent_allow_parallel_limit_is_enforced() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(2, 2).expect("config"),
    )
    .expect("service");
    let descriptor = CommandDescriptor::new(
        command("worker.parallel"),
        "Parallel",
        ExecutionDomain::Background,
        ConcurrencyPolicy::AllowParallel { max_in_flight: 1 },
    )
    .expect("descriptor");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(descriptor, move |_: (), _| {
            let _ = started_tx.send(());
            release_rx.lock().expect("gate").recv().expect("release");
            Ok(1_u8)
        })
        .expect("registration");
    let first = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("started");
    let second =
        block_on(service.invoke_typed(registration.handle(), (), InvocationContext::default()));
    assert_eq!(second.expect_err("parallel cap").code, ErrorCode::Busy);
    release_tx.send(()).expect("release");
    assert_eq!(block_on(first).expect("first"), Completion::Completed(1));
}

#[test]
fn background_single_flight_rejects_a_second_run() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(2, 2).expect("config"),
    )
    .expect("service");
    let descriptor =
        CommandDescriptor::background(command("worker.single"), "Single").expect("descriptor");
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let registration = service
        .register_background(descriptor, move |_: (), _| {
            let _ = started_tx.send(());
            release_rx.lock().expect("gate").recv().expect("release");
            Ok(3_u8)
        })
        .expect("registration");
    let first = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("started");
    let second =
        block_on(service.invoke_typed(registration.handle(), (), InvocationContext::default()));
    assert_eq!(second.expect_err("single flight").code, ErrorCode::Busy);
    release_tx.send(()).expect("release");
    assert_eq!(block_on(first).expect("first"), Completion::Completed(3));
}

#[test]
fn background_deferred_completion_keeps_running_across_threads() {
    let provider = Rc::new(FakeMenuProvider::new());
    let service = MenuCommandService::with_worker_pool(
        provider,
        WorkerPoolConfig::new(1, 1).expect("config"),
    )
    .expect("service");
    let descriptor =
        CommandDescriptor::background(command("worker.deferred"), "Deferred").expect("descriptor");
    let slot: Arc<Mutex<Option<gpui_menus::BackgroundCommandCompletion<u64>>>> =
        Arc::new(Mutex::new(None));
    let captured = slot.clone();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let registration = service
        .register_background_deferred(descriptor, move |_: (), _| {
            let (outcome, completion) = gpui_menus::BackgroundCommandOutcome::<u64>::deferred();
            *captured.lock().expect("slot") = Some(completion);
            let _ = started_tx.send(());
            Ok(outcome)
        })
        .expect("registration");
    let subscription = service.subscribe();
    let mut request = service.invoke_typed(registration.handle(), (), InvocationContext::default());
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    started_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("handler started");
    assert!(poll_once_ref(&mut request).is_none());
    assert!(
        poll_once(
            subscription
                .try_next()
                .expect("only one event reader is active")
        )
        .is_none(),
        "CommandFinished must not be emitted while the completion port is pending"
    );
    assert_eq!(
        service.command_task(started.id).expect("task").state,
        CommandTaskState::Running
    );
    let completion = slot
        .lock()
        .expect("slot")
        .as_ref()
        .expect("completion")
        .clone();
    assert!(completion.complete(11));
    assert_eq!(
        block_on(request).expect("deferred background"),
        Completion::Completed(11)
    );
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("finished");
    };
    assert_eq!(finished.id, started.id);
    assert_eq!(finished.state, CommandTaskState::Completed);
}

#[test]
fn failed_commands_emit_terminal_failed_state() {
    let service = MenuCommandService::from_provider(FakeMenuProvider::new());
    let descriptor = CommandDescriptor::ui(command("app.fail"), "Fail").expect("descriptor");
    let registration = service
        .register_ui(descriptor, |_: (), _| {
            Err::<(), _>(gpui_menus::SystemError::new(
                ErrorCode::Unavailable,
                "handler failed",
            ))
        })
        .expect("registration");
    let (_owner, context) = ui_context(95);
    let subscription = service.subscribe();
    let error = block_on(service.invoke_typed(registration.handle(), (), context))
        .expect_err("failed invocation");
    assert_eq!(error.code, ErrorCode::Unavailable);
    let MenuCommandEvent::CommandStarted(started) = next_event(&subscription) else {
        panic!("started");
    };
    let MenuCommandEvent::CommandFinished(finished) = next_event(&subscription) else {
        panic!("finished");
    };
    assert_eq!(started.id, finished.id);
    assert_eq!(
        finished.state,
        CommandTaskState::Failed(ErrorCode::Unavailable)
    );
}
