# Coalesced source-delivery evidence

This freezes the [background visual source channel](../../RETAINED_SOURCE_DELIVERY.md)
on base `8afed1c`. All 12 CPU, 12 native and 67 hardware source hashes were
verified against the final sources before freezing. The hardware adapter is
GTX 1650 Ti / Vulkan 571.96. Validation passes 70 retained-text/WGPU core tests,
64 retained-only tests and 11 gallery/font tests. Combined and feature-specific
Clippy checks pass with `-D warnings`; the desktop WGPU diagnostic builds.
The hardware suite passes all 142 pixel scenarios.

## CPU comparison

Three rotating rounds use eight warmup and forty measured batches per mode
per round (120 measured samples per mode). All modes process the same messages
and verify equal final geometry, materials and commands. The inbox benchmark
enqueues and drains on the same benchmark thread. Timing includes transactions,
computed propagation and sinks; it excludes OS scheduling, contention, layout,
frame extraction, GPU work and presentation. Raw round samples and counts are
in `delivery-benchmark.json`; external host load remains uncontrolled.

| Sources | Messages / batch | Per-message transactions median / P95 (µs) | Explicit transaction median / P95 (µs) | Inbox median / P95 (µs) |
| --- | --- | --- | --- | --- |
| 1 | 1,000 | 819.90 / 983.90 | 103.10 / 119.30 | 60.80 / 135.70 |
| 1,000 | 1,000 | 1,050.30 / 1,344.40 | 1,161.90 / 1,537.40 | 1,597.90 / 2,398.80 |
| 1,000 | 10,000 | 9,734.00 / 11,636.60 | 2,492.00 / 2,820.40 | 2,638.70 / 3,319.70 |

Repeated writes benefit from dropping superseded values before computation.
One thousand distinct one-shot inputs retain mutex/map overhead and are slower
through the inbox. An explicit owner transaction remains preferable when the
caller already owns a synchronous batch. These are workload-specific CPU
results, not a frame-rate or cross-thread wake-latency claim.

## Real workers, owner executor and GPU

The gallery test uses an actual OS worker for 200 writes, then verifies one
transaction/computed evaluation/effect/visual revision/view render. Equal final
delivery and idle event-loop drains do not render again. Unit tests cover
concurrent producers, capacity, stale generations, atomic rejection, runtime
and receiver closure, and waiter replacement.

The hardware `delivery-worker-burst` case sends 999 messages from a real worker,
coalesces 998, then publishes one transaction and revision with zero layout,
zero chunk recording and 160 instance bytes. Its pixels match original full
GPUI rendering. Derived-equal and retired-source cases cause no GPU submit.
The three `delivery-*.png` images and `validation.json` record these cases.
Other regenerated benchmark JSON files are controls, not attributed speedups.

The native diagnostic creates a real Windows window and uses its foreground
executor. It receives 1,000 worker writes with 999 replacements, one waker
notification, one transaction, one computed evaluation and one effect. The
helper launches hidden; the observed native window is also hidden and records
zero automatic view renders. One explicitly requested hidden-window draw
verifies equal current GPU pixels. Equal delivery and both idle intervals add
no view render. `worker_to_delayed_check_us` includes an intentional delay and
is not delivery latency. Visible-window automatic presentation, OS-wide wake
counts and energy remain unverified. The complete RFC goal remains active.
