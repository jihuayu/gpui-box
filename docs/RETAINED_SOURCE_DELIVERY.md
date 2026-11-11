# Coalesced background source delivery

`ReactiveUi` and `LogicalUi` now expose `open_updates(capacity)`, typed
`update_sender(receiver, signal)` and owner-only `apply_updates(receiver)`.
The sender accepts owned `PartialEq + Send + 'static` values and can be cloned
across worker threads. The mutable runtime, render tree, computed closures and
effect callbacks stay on the UI thread. One live receiver belongs to one runtime.

## Queue and commit semantics

The inbox stores one pending final value per source identity/generation. A
replacement drops the previous payload after releasing the mutex. Same-value
pending writes are rejected before a new payload or wake is installed. Equality
against committed UI state still runs at owner drain; the producer never reads
the owner's value. A committed-equal delivery may wake the owner once but
dispatches no sinks/effects and requests no visual frame.

Capacity bounds distinct pending source slots. Replacement remains possible at
capacity, while another source returns `UpdateQueueFull`. This is not a physical
byte budget: individual payloads may be large. Senders validate runtime and
writable-source ownership at construction. Computed signals cannot have writable
senders. Weak source-lifetime tokens reject stale producer handles; owner drain
also checks source generation and discards values queued before scope removal.

`ready().await` registers the executor waker under the same mutex used for
publication, preventing a check/register lost wake. Empty→pending wakes once;
later writes coalesce until drain. It works with a host future rather than a
polling timer. Receiver drop or runtime drop closes the queue, releases pending
payloads, and wakes a waiting future to terminate. A closed receiver may be
replaced; old sender endpoints stay closed.

Drain removes one bounded pending set, releases the mutex, skips retired sources
and applies all live final values in one ordinary reactive transaction. That
transaction settles computed dependencies, validates all property sinks, commits
sources/render state, then runs effects once. Errors reject the complete live
batch and consume it: there is no automatic retry loop that can keep a bad
payload busy. Concurrent writes after the take belong to the next drain. Empty
and retired-only drains do not create reactive transactions.

Ordering among different sources is identity order; same-source replacement is
last successful mutex acquisition. A synchronous UI write and a later owner
drain have ordinary owner execution order; queued values can overwrite earlier
synchronous writes. This is a discardable visual-state channel. Ordered business
events, lifecycle/resource-release commands and intermediate-value side effects
require an explicit ordered channel. There is no implicit merging of unrelated
synchronous events.

## Ordinary host and native diagnostic

The gallery owns a receiver task and a color sender. A real background-color
button publishes a short OS-thread burst. The foreground future awaits readiness,
drains once, notifies only on a visual commit and yields once after each drain
to return control under sustained producers. GPUI notification runs inside an
App effect cycle. The task holds only a weak entity; cancellation drops its
receiver and closes delivery.

The GPUI integration test intentionally permits real worker threads using the
test scheduler's `allow_parking` API. It joins a 200-write worker while the
owner is occupied, then drains the host executor: one transaction, one computed
evaluation, one effect, one frame revision and one view render result. Sixteen
idle drains add no view render. A subsequent committed-equal worker message
wakes the owner but produces no new visual revision or render. Ordinary canvas
material changes record one local chunk because that consumer does not declare
instance-patch capability.

An opt-in desktop diagnostic runs a real native platform executor and window:

```powershell
./tooling/rendering-perf/run-delivery-native.ps1
```

The helper launches hidden, consistent with background process policy. The
observed Windows window is `Hidden`: the real executor consumes 1,000 messages
as one transaction and receives one source waker notification, but Windows does
not automatically redraw a hidden window. The diagnostic labels that state and
draws it explicitly through the normal prepaint/paint path to verify current
retained GPU pixels. It records zero automatic renders, one explicit render,
pixel equality and no further view renders during its idle intervals/equal
delivery. This proves native executor delivery and explicit hidden-window
rendering. Visible-window automatic presentation, OS-wide wake counts and energy
remain unverified. Diagnostic waits are excluded from any latency claim.

## Hardware and performance evidence

The hardware suite adds a real 999-message worker burst through `LogicalUi`.
One drain evaluates one derived color, dispatches one sink/effect and emits one
revision, zero layout/recording and 160 GPU instance bytes. It compares original
full GPUI pixels. Derived equality and queued source retirement produce no GPU
submit and match reference pixels. These checks run alongside prior primitive,
composition, resource and multi-frame regression cases.

The CPU benchmark compares the same final source sequence in three modes:
one transaction per message; one explicit transaction staging every message;
or mutex inbox enqueue of every message followed by one owner transaction.
The timed inbox workload uses one benchmark thread, so OS worker scheduling,
wake latency and contention are excluded. Three rotating rounds contain eight
warmups and forty measured batches per mode/round. Source/derived/sink commits
are timed; layout/extraction/GPU/presentation are excluded. Every round checks
equal geometry, materials and commands. Work counts verify that repeated-source
messages reduce computed evaluations and frame intents; unique-source workloads
retain queue overhead without removing those computations.

```powershell
./tooling/rendering-perf/run-delivery.ps1
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-delivery-gpu
```

[Frozen evidence](performance/2026-11-12-source-delivery/README.md) contains samples,
source/binary hashes, native diagnostic and hardware results. The
[complete RFC goal](RETAINED_IMPLEMENTATION_PROGRESS.md) stays open, including
native surface presentation, idle GPU retirement, physical resource budgets,
atomic structural updates and broad layout/primitive coverage.
