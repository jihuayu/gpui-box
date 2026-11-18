# Scoped UI tasks and result adoption

Status: experimental explicit Rust API, 2026-11-19. Complete RFC remains active.
The [decision and limits](ADR_RETAINED_UI_TASKS.md) describe ownership and costs.

## Start and receive

`LogicalUi::start_task(scope, key, launch)` and
`LogicalEdit::start_task(scope, key, launch)` replace one task in that scope/key.
`ComponentContext::start_task(key, launch)` uses the component's scope. Mount runs
once, so keyed reorder or prop changes do not automatically restart its task.
Applications explicitly replace tasks for changed requests; effect/hooks or
event-task restart helpers remain future work.

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
automatic async hooks, byte quotas, scheduling priority,
timeout/deduplication policy or independent native presentation. There is one
current task per registered key and bounded work admission; applications can still
create arbitrarily many inactive registry keys. Uncooperative cancelled workers
can keep computing while holding their slots. Hosts control executor concurrency.
Sparse map clones and output destruction remain owner costs. No performance or
sustained 60/120Hz claim is made.

See [verification and reproduction](performance/2026-11-19-ui-tasks/README.md).
