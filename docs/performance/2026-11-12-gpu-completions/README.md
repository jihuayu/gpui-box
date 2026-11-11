# Native GPU completion evidence

This freezes [idle completion delivery](../../RETAINED_GPU_COMPLETIONS.md) on
base `1de199d`. All 70 hardware source hashes and 15 native source hashes match
the final sources. Adapter: GTX 1650 Ti / Vulkan 571.96. The core suite passes
72 tests, the gallery/font suite passes 11 including hardware clicks, and
combined and retained-WGPU-specific Clippy pass with `-D warnings`. The native
desktop diagnostic builds and passes; the hardware suite passes 145 pixel cases.

Three new hardware cases check idle snapshot/image retirement, late subscription
and cancellation/resubscription, and reset/new-epoch full resync. Renderer drop
with outstanding work closes the receiver. The owner awaits real worker wakes
without another submit/readback/finish/poll loop. After collection, frame and
GPU leases are zero, and weak references to old packet items and 16 decoded
image bytes expire. Pixel comparisons then match original full GPUI rendering.
Three `completion-*.png` files and `validation.json` record those scenarios.

The real Windows foreground executor also collects two diagnostic offscreen
submissions without another visual frame. It observes zero remaining leases,
zero additional view renders and stable driver counters over a subsequent
250 ms idle interval. The native window is hidden. The separate color pixel
check explicitly draws that hidden window, so it is not visible-window
automatic presentation evidence. Timers are diagnostic observation points;
the actual completion task is awaited and has no timer.

## Completed-work wall time

Identical 256×128 offscreen workloads compare owner-blocking `finish()` against
native worker wait → host wake → owner collection. Three alternating rounds
contain eight warmups plus sixty measured samples/mode/round (180 samples per
mode). Timing includes submit, GPU completion and host scheduling, excludes
tree commit/extraction/readback/presentation, and does not measure UI input
latency or CPU busy time. Each worker-mode round records exactly 68 scheduled
jobs, waits and delivered events including warmups. Raw samples/rounds are in
`completion-benchmark.json`.

| Children | Owner-blocking median / P95 (µs) | Completion wake median / P95 (µs) |
| --- | --- | --- |
| 1 | 318.0 / 538.5 | 307.6 / 713.1 |
| 1,000 | 535.8 / 927.8 | 516.2 / 621.3 |

These results do not establish a general speedup: small-case P95 increases and
external host load is uncontrolled. The implemented benefit is owner retirement
without waiting for another visual update or polling timer. Other regenerated
benchmark JSON files are controls, not improvements attributed to this stage.
Forced real device/surface loss, native GPU surface presentation, physical memory
budgets, energy/OS wake counts and cross-platform qualification remain open.
The complete RFC goal is still active.
