# Idle GPU completion and owner retirement

`WgpuFrameRenderer::open_completions()` opens one native completion receiver for
the current device epoch. The host awaits `receiver.ready()` without borrowing
the renderer, then calls its owner-only `poll()` to collect completed GPU and
FramePacket leases. Completion collection does not request a visual frame.
The retained-WGPU gallery holds this task alongside its source-delivery task.

Native WGPU completion callbacks require device polling; registering a callback
alone cannot wake an entirely idle owner. This opt-in subscription starts one
worker per renderer. A submission publishes its serial and native submission
index to a one-job mailbox. The worker blocks on `device.poll(Wait)` only while
work is outstanding, then publishes completion and wakes the host executor.
When there is no job it sleeps on a condition variable. There is no periodic
timer, continuous GPU polling, renderer-thread ownership transfer or UI callback
on that worker. Ordinary renderers that do not subscribe start no worker.

The pending job retains only the newest index; finishing that submission proves
completion of all earlier work. One completion event likewise replaces earlier
unconsumed successes. Native wait success advances the monotonic completion
serial independently of the callback, because concurrent WGPU polling can take
callbacks and fire them after another poll has returned. Owner collection still
checks native errors and releases leases in serial order. `lifecycle()` and
`completion_stats()` are observational counters and perform no polling.

`ready_receipt()` additionally preserves the worker's `Instant` observation and
covering serial; serial-only `ready()` retains its behavior. A coalesced receipt
is an upper bound for earlier submissions, not their exact GPU completion time.
Owner observation can occur later and is measured separately by the pipelined
harness. Neither instant establishes native presentation/display completion.
`poll_frame_capacity()` collects completions nonblockingly and reports actual
flight count, limit and device epoch before replaceable visual construction.
It is an owner-local observation, not a reservation or memory/deadline promise.

Opening a subscription after submission schedules the outstanding tail. Duplicate
live receivers are rejected. Receiver or renderer drop cancels pending jobs,
releases the stored waker and wakes an idle worker to exit. Renderer drop never
joins a native wait on the UI thread. An already active wait can keep a device
clone until completion or its 30-second error timeout; this is not immediate
native cancellation. Wait errors close delivery and are surfaced once, without
an automatic retry loop. The gallery records the failure. A reset closes the old
device subscription; a host continuing after reset must subscribe to the new
epoch and resubmit a complete snapshot. The hardware cases exercise this path.
The gallery now reopens a closed epoch's receiver and wakes its background
visual admission waiter. Native wait errors remain explicit terminal failures.

## Verification

The core suite includes the same mailbox source used by the vendor backend, with
the root workspace's locked dependencies. It tests waiter replacement, newest
completion coalescing, closure/waker release, and single-delivery errors. This
avoids resolving the separate upstream workspace's unavailable offline test
dependency closure. Hardware cases use the actual native worker/device:

- Two submissions replace and retire an image; after the host awaits completion,
  GPU/FramePacket lease counts are zero and weak references to the old packet
  and decoded pixel allocation expire before any later submit/readback/finish.
- Late subscription, cancellation with outstanding work, resubscription, reset
  and active renderer drop preserve epoch closure. Post-retirement pixels match
  original full GPUI or a fresh full retained consumer as appropriate.
- The Windows diagnostic submits separate offscreen packets through the gallery
  renderer, returns to the native foreground executor and observes retirement
  without another frame. There are zero extra view renders and stable completion
  counters during a subsequent 250 ms idle interval. Diagnostic timers trigger
  observations; normal completion delivery has no timer.

```powershell
./tooling/rendering-perf/run-delivery-native.ps1 -Output target/completion-native-check
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/retained-completion-gpu
```

The benchmark compares owner-blocking `finish()` with worker wait → host wake →
owner collection for identical completed work. Three alternating rounds include
eight warmups and sixty measured samples per mode/round. Submit, native GPU wait
and host scheduling are timed; readback/presentation and tree commit/extraction
are excluded. This exposes wake overhead, not a claim that asynchronous waiting
always completes faster. [Frozen samples and metadata](performance/2026-11-12-gpu-completions/README.md)
record the results. Forced real device loss, native surface loss/presentation,
sustained animation/input latency, OS-wide wake counts, energy, and other
platforms remain open in the [full goal](RETAINED_IMPLEMENTATION_PROGRESS.md).
