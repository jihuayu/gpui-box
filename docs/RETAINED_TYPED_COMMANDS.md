# Typed business commands with bounded admission

Status: experimental explicit Rust application API,2026-11-18. Complete RFC stays
active. Extends [business Store](RETAINED_BUSINESS_STORE.md) with a separate
reliable request/result protocol. It is independent of retained/GPU/platform
features. No runtime, worker thread, IO handler or renderer callback is installed.

## Intention, admission and result

`gpui_box::command::Command` declares owned Send request data and associated
Output/Error types. Applications define domain intentions, such as saving one
document with an expected StoreRevision. `command_channel<C>(capacity)` creates
one cloneable typed sender and one application-owned receiver.

```rust,ignore
struct SaveDocument { expected: StoreRevision, draft: Document }
impl Command for SaveDocument {
    type Output = ();
    type Error = StoreError;
}
let (save, mut requests) = command_channel::<SaveDocument>(16)?;
let mut ticket = save.send(SaveDocument { expected, draft })
    .map_err(|rejected| (rejected.reason, rejected.command))?;

// On the application's executor; this code is never called by the UI builder.
if let Some(mut request) = requests.try_recv() {
    let save = request.take_command().unwrap();
    // Optional owned IO happens here. Revalidate expected revision on commit.
    match writer.commit(save.expected, save.draft) {
        Ok(commit) => request.succeed((), Some(commit.revision)),
        Err(error) => request.fail(error),
    }
}
let saved = ticket.result().await?;
// saved.committed_revision is business evidence. UI adoption occurs separately
// through ui.apply_store(input), then GPU submission/presentation later.
```

Capacity counts **all accepted unresolved requests, queued plus running**. Moving
payload to an IO task does not release it. Busy/Closed/ID-exhaustion rejection
returns the original command; no ticket is accepted, no work is silently dropped.
Every accepted request has a queue-qualified monotonically reserved CommandId.
Requests never coalesce, even if payloads compare equal. A compile control rejects
sending an unrelated intention through a typed sender.

Successful send establishes Accepted; try_recv dispatches in FIFO order and marks
Running. The application completes exactly once by consuming CommandRequest:

| Completion | Ticket status / result |
| --- | --- |
| succeed(output, optional StoreRevision) | Succeeded / owned CommandSuccess |
| fail(domain_error) | Failed / CommandFailure::Business |
| cancel() | Cancelled / CommandFailure::Cancelled |
| Running request dropped or unwinds | Failed / ExecutorAbandoned |
| Receiver dropped before queued dispatch | Failed / ExecutorClosed |

`try_result` or `result().await` consumes one owned result; status remains terminal.
Polling result again after consumption is a programming error; try_result then
returns None. Result payloads are not placed in the visual latest-only Store
subscription. Completion can carry a committed business revision without waiting
for the UI or display. No implicit retry, idempotency key generation or magical
exactly-once external IO guarantee is provided.

## Cancellation and closing

`ticket.request_cancel()` and request completion share queue→completion lock
order. If still Accepted, cancellation removes the queued envelope, returns
Cancelled and releases capacity immediately; it cannot later dispatch. Once
Running, it sets a cancellation request and wakes `request.cancelled().await`.
The worker chooses whether it can stop and report Cancelled, or whether committed
side effects require a success/business-error result. A late cancellation does
not overwrite success or roll back a Store/external operation.

Dropping a ticket only stops observing; it **does not cancel** accepted business
work when a component/window goes away. Application/window/component task scopes
must request cancellation explicitly according to the operation. A component
closing must not silently cancel a user's accepted save/download. This module
does not supply a generalized task-scope registry or activation generations.

`receiver.close()` or last sender disposal closes admission but drains accepted
FIFO work. After the pending queue drains ready returns false, even if independent
running requests still complete later. Receiver disposal makes queued tickets
terminal before releasing queue ownership, including cancellation races; already
running requests retain their completion capability. Dropped/unwinding running
requests report ExecutorAbandoned and release capacity. Cloned senders do not
keep an abandoned receiver open.

Receiver ready, ticket result and running cancellation futures integrate with a
host executor; no polling timer or background runtime is imposed. Mutexes protect
short transport state transitions only. Payload handlers never run under them;
payload destruction and Waker clone/drop/wake occur outside locks. Poison recovery
is defensive for transport internals; arbitrary handler panics remain application
errors and are reflected by request-drop completion.

## Ordering, scope and remaining work

One receiver gives FIFO **dispatch**, not automatic serial completion if the
application starts tasks concurrently. Conflicting domain operations must run
sequentially in that application executor or use explicit per-object coordination;
Store expected-revision checks remain mandatory after slow IO. Different objects
may run concurrently within the same outstanding-request capacity. There is no
hidden global pool, object-key scheduler or handler registry in this stage.

The UI integration controls run typed intentions through a business worker,
complete with StoreRevision, then explicitly apply Store input through existing
selectors/ComponentPlan/Flex. Real Noto wrapping and all CPU/GPU pixels compare
with independent complete references. See
[verification/reproduction](performance/2026-11-18-typed-commands/README.md).

Each request has completion Arc/Mutex storage; channel admission uses a Mutex and
VecDeque, queued cancellation scans/removes by ID (O(queued requests)), and close
visits queued requests. Capacity bounds unresolved transport work, not payload
bytes or completed results retained by application tickets. Callers still budget
large payloads/results and external IO. No latency/FPS/memory reduction is claimed;
no source schema, layout solver or GPU format change.

The follow-up [scoped provider API](RETAINED_SCOPED_PROVIDERS.md) now resolves
typed sending capabilities through logical ancestry, with fixed consumer identity.

Implemented: typed bounded admission, IDs/status, FIFO dispatch, explicit reliable
results, queued/running cancellation, close/abandon lifecycle and Store/UI evidence
boundaries; scoped providers add capability resolution. Remaining: event handler/task
integration, per-object execution policy, protocol serialization across languages,
resource byte/admission budgets, hooks/macros/KeepAlive and independent native
presentation. Complete RFC remains active. "99%" is figurative ordinary smooth,
intuitive behavior, not a numeric success ratio or p99 gate.
