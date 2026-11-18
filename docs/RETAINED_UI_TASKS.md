# Scoped UI tasks and result adoption

Status: experimental explicit Rust API, 2026-11-19. Complete RFC remains active.
The [decision and limits](ADR_RETAINED_UI_TASKS.md) describe ownership and costs.

## Start and receive

`LogicalUi::start_task(scope, key, launch)` and
`LogicalEdit::start_task(scope, key, launch)` replace one task in that scope/key.
`ComponentContext::start_task(key, launch)` uses the component's scope. Mount runs
once, so keyed reorder or prop changes do not automatically restart its task.
Applications can explicitly replace tasks for changed requests, or declare a
reactive task with the typed input API below. Ambient async hooks remain open.

The [event task stage](ADR_RETAINED_EVENT_TASKS.md) adds these same operations
to EventContext. They use that handler's registered scope automatically. Start or
cancel is part of the event's source/producer/default/frame candidate, including
Busy fallback and retirement. This provides explicit task replacement.
Parameter-driven work uses task_effect.

```rust,ignore
EventHandler::new(move |event, cx| {
    if event.kind == UiInputEventKind::Activate {
        let executor = executor.clone();
        match cx.start_task(4, move |context: UiTaskContext<Preview>| {
            executor.spawn(async move {
                let _ = context.complete(prepare_preview());
            }).detach();
        }) {
            Ok(_) => { cx.set(loading, true)?; }
            Err(Error::TaskCapacity) => { cx.set(waiting_for_capacity, true)?; }
            Err(error) => return Err(error),
        }
    }
    Ok(())
})
```

`cx.task::<T>(key)` observes the same event draft; `cx.cancel_task(key)?` cancels
after successful adoption only. Ancestor handlers own their own scope's tasks,
even for equal keys. Host readiness/result adoption remains explicit, as below.

The launch closure runs only after the complete candidate and ordinary effects
succeed. Schedule short-lived work through the existing host executor; do not do
synchronous IO or heavy CPU work in this callback. For example, in a GPUI owner:

```rust,ignore
let executor = cx.background_executor().clone();
let mut task = ui.start_task(scope, 4, move |context: UiTaskContext<Result<Preview, LoadError>>| {
    executor.spawn(async move {
        if context.cancellation_requested() { return; }
        let output = prepare_preview();
        let _ = context.complete(output); // cancelled work returns its output
    }).detach();
})?;
```

Move only owned Send+Sync results and the worker context across threads. Signal
writers, arbitrary frontend callbacks and LogicalUi remain on the owner. Ready
output holds one Arc; no result fields are independently merged. Business errors
are ordinary result payloads. Dropping/panicking without complete reports Abandoned.

`UiTask::ready(&mut self)` yields the terminal/ready status and wakes on completion,
cancellation or abandonment. A host task then serially updates its owner:

```rust,ignore
match task.ready().await {
    UiTaskStatus::Ready => view.update(cx, |view, cx| {
        match view.ui.apply_task(&task, |output, tx| {
            tx.set(view.preview, output.clone())?;
            Ok(())
        })? {
            TaskApply::Applied(commit) if commit.commit.request_frame => cx.notify(),
            _ => {}
        }
        Ok::<_, Error>(())
    })?,
    _ => {} // cancelled or abandoned; application may expose its error policy
}
```

The GPUI test in retained_owner_demo exercises this with the real host background
executor and owner wake, followed by complete software-image and input-scene
installation. No separate task pool or polling timer is added.

## Tasks driven by current parameters

`LogicalUi::task_effect(scope,key,input,effect)` and the matching LogicalEdit
method declare an ordinary `Signal<I>` input. `task_effect_prop` accepts a
component's `ReadProp<I>`; ComponentContext methods use its own scope. Mount once
and follow future props without remounting. Multiple parameters can be combined
through a pure computed source into one typed request.

```rust,ignore
let executor = cx.background_executor().clone();
let effect = UiTaskEffect::new(move |request: PreviewRequest, context| {
    executor.spawn(async move {
        let output = prepare_preview(request);
        let _ = context.complete(output);
    }).detach();
});
ui.task_effect(scope, 4, request_signal, effect.clone())?;
// Later source changes automatically replace this task for the final value.
```

Clone a factory to preserve its identity across redeclarations. Creating a new
factory deliberately restarts even equal input. Final source equality, including
computed values and props, is checked after producer/responsive settlement and
scope retirement. Writes 1→2→1 retain the task; 1→2→3 replace it once with3. Only
after full candidate/frame adoption and ordinary effects does the callback run.
It must schedule owned work; the callback itself still executes on the owner.
Inputs can be owner-local; move only selected Send data to the worker. Keep input
values compact because cloning happens on the owner.

`task_effect_status` returns Task(UiTaskStatus) or WaitingForCapacity. When a new
input cannot be admitted, its UI/source state still commits, the old task cancels,
and task::<T> returns None. Escaped old worker holds its slot until exit and its
late completion rejects. The host uses the existing capacity receiver, then calls
`retry_task_effects`; ordinary logical edits/frame production also retry waiting
inputs once. There is no capacity polling timer, automatic wake-to-owner bridge
or queued copy of each intermediate input. Only the latest input is retained.

`remove_task_effect` removes the declaration and cancels its task transactionally.
While registered, the key belongs to that declaration; explicit start/cancel on
LogicalUi/LogicalEdit/EventContext returns InvalidTask. Retiring its scope removes
it automatically. A surviving effect cannot keep a dependency from a retired
scope: the candidate rejects and old state stays live.

Ready results still use explicit `task.ready`/`apply_task` and host image/scene
adoption. Applied/Abandoned/Cancelled terminal tasks do not automatically restart
on unrelated edits or capacity retry; change an explicit request revision or
replace the factory for deliberate retry. A post-commit effect panic leaves
adopted state intact and cancels unlaunched work. This is a typed task effect,
without ambient async hook/macro or automatic result callbacks. See
[ordering, alternatives and owner costs](ADR_RETAINED_TASK_EFFECTS.md).

## Bounded work admission

Each owner now defaults to a `UiTaskPool` of64. It bounds declared work and
retained results, not executor threads or bytes. To share an explicit limit
across logical owners/windows before work exists:

```rust,ignore
let pool = UiTaskPool::new(4)?;
ui.set_task_pool(pool.clone())?;
let mut capacity = pool.open_capacity()?;
```

`start_task` returns `Error::TaskCapacity` when full before changing the old task
or running its launch callback. Busy can be handled within a candidate without
poisoning component construction; publish an ordinary loading state and retry
later. Provisional declarations also reserve capacity, so a candidate cannot
temporarily allocate unbounded launches and rely on later rollback.

A cancelled worker continues to occupy its slot until its context drops. A Ready
result occupies it until adopted or cancelled/destroyed; a failed result apply
keeps the reservation for retry. Capacity release follows payload destruction and
wakes `capacity.ready().await` without a timer. It returns Some(stats) when space
is available, or None when the pool is gone. This is an observation: retry can
still return Busy after another owner wins the slot. One capacity receiver is
allowed; a host sharing admission across windows centralizes retry ordering.

`pool.stats()` reports capacity/in_use/peak/admitted/rejected. Counts include
candidate reservations that later roll back. Registry entries without live work
do not hold slots, but still require explicit key/scope cleanup. No live pool
migration is permitted. An apply callback/producer that starts more work may
need a spare slot while it borrows the current result. See
[decision and tradeoffs](ADR_RETAINED_UI_TASK_ADMISSION.md).

## Status and cancellation

| Status | Meaning |
| --- | --- |
| PendingCommit | Launch declaration exists only in an unadopted candidate |
| Running | Launch accepted; host may still be scheduling its worker |
| Ready | One complete output available for explicit owner adoption |
| Applied | Output source/layout transaction successfully adopted |
| Cancelled | Replaced, explicitly cancelled, retired, or launch discarded |
| Abandoned | Worker/launch dropped or panicked after starting without output |

`task::<T>(scope,key)` retrieves the current typed handle; wrong output type is
InvalidTask. Task keys are independent of state/effect/event keys. `cancel_task`
is transactional and removes the registry entry after success. Escaped/cloned
handles do not retain cancellation ownership. Worker `cancelled().await` supports
cooperative async cancellation; check cancellation_requested between CPU chunks.
One owner ready waiter and one worker cancellation waiter are supported; clones
are not broadcast notification subscriptions.

Replacement cancels older work even for equal scope/key. Scope generation checks
prevent remount from reviving old results. Reorder preserves tasks; removal of a
scope cancels its entire subtree. Owner destruction cancels before effect cleanup.
Cancelling a Ready result releases its payload. Applied and Abandoned stay terminal
for diagnostics after retirement. All external effects remain the application's
responsibility; cancellation cannot undo IO already performed.

Business Command lifetime is independent. Component retirement does not cancel an
accepted save/download; use its explicit ticket cancellation policy. Short-lived
UI queries and previews belong here, while durable business intent belongs in
[typed Commands](RETAINED_TYPED_COMMANDS.md).

## Result transaction and errors

`apply_task` rejects foreign owners, skips no-longer-current identities, returns
Pending without running the callback for active work, and ignores terminal work.
For current Ready it borrows the immutable output and executes one source closure
through the existing reactive candidate, component producers and layout. Current
published constraints and viewport are reused even without a persistent producer.
Normal final-frame validation occurs before result consumption and effects.

Source/producer/layout failure or pre-adoption panic rolls back all UI and leaves
the result Ready for explicit retry. Successful adoption marks Applied before
ordinary cleanup/effects, so an effect panic cannot replay it. If that same result
removes its own component through a producer, it remains Applied. Host image
installation and input scene adoption are separate, unchanged steps.

Starting/replacing/cancelling tasks in a failed candidate never affects the old
live task. Multiple starts at one key in one candidate keep only the last launch;
tasks declared in a scope removed in that same candidate never launch. A post-
commit effect panic discards not-yet-launched work; launch panic leaves already
adopted UI intact and marks undelivered launches cancelled. Callbacks, wakers and
user output destruction are not run while a task mutex is held.

## Remaining work

This is scope-owned task/result lifetime, not KeepAlive activation generations,
ambient async hooks/macros, automatic result callbacks, byte quotas, scheduling priority,
timeout/deduplication policy or independent native presentation. There is one
current task per registered key and bounded work admission; applications can still
create arbitrarily many inactive registry keys. Uncooperative cancelled workers
can keep computing while holding their slots. Hosts control executor concurrency.
Sparse map clones and output destruction remain owner costs. No performance or
sustained 60/120Hz claim is made.

See [verification and reproduction](performance/2026-11-19-ui-tasks/README.md).
Parameter-driven controls are recorded in
[task effect verification](performance/2026-11-19-task-effects/README.md).
