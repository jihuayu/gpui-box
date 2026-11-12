# Cross-frame layer allocation budget evidence — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`2d017ac`; all four 150-source manifests match final tested working-tree bytes.
The measured binary hash and audit counts are in [verification.json](verification.json).

The complete hardware suite passes **213 scenarios**, including **211 byte-exact
pixel comparisons** and two backend preflight/retry cases. The suite covers all
previous mixed rendering/lifecycle paths plus new allocation-budget scenarios.
Core tests pass **116**, gallery/font/hardware-click tests **12**, selected
core/gallery/perf Clippy targets pass `-D warnings`, and the desktop WGPU build
passes. Test, Clippy and desktop build logs are preserved alongside the hardware
validation JSON, stdout and PNGs.

## Observed allocation/lifetime behavior

- Twenty-four opacity updates reuse one 8,400-byte texture, including while
  multiple frames reference it. Unique allocation count and peak remain one.
- Setting zero rejects without changing accounting. Positive reconfiguration
  completes pending work and starts a new cleared budget epoch.
- Removing isolation produces zero live cache bytes while 8,400 retired bytes
  are still completion-held. A native completion notification plus owner poll
  reduces resident/retired bytes and allocation count to zero without a new frame.
- Reset retains the configured limit and starts the new epoch peak at zero.
- Sixteen successive large-layer origin replacements exercise **16 budget waits**.
  Resident/peak texture payload stays at **4,336,816 bytes**, and final pixels
  match a fresh full-viewport reference.
- A new packet larger than a 4,096-byte cap rejects before cache mutation or
  serial advancement; a valid retry succeeds as serial two.

Pure shared-reservation tests additionally cover duplicate flight references,
last-reference release, retained peak history, zero/overflow rejection, and a
lease outliving its budget owner. The allocator checks reservations again after
preflight, so admission cannot silently overshoot the configured cap.

## Completed-work cost of a tighter cap

The workload alternates a 1200×900 group's physical origin on a 1400×1000 surface.
Each new origin currently replaces its exact texture/target allocation. One
conservative allocation is 1202×902×4 = **4,336,816 bytes**. Both modes use the same
renderer, Full root redraw, three frames in flight and equal primitive updates.
Only the allocation payload cap changes: default 256 MiB versus one-layer payload.

Three executions each contain three rotating-order rounds. Each round runs six
warmups, completes them, then times twenty owner transaction/publication/submit
updates and a final GPU completion. The table averages **nine completed blocks**
per mode (180 timed frames); it is not a median of individual completion latency.
Readback and byte-exact reference comparisons happen after each timed block.
All per-block completion means and per-frame submit wall spans are preserved.

| Overlapping quads | Default cap µs/frame | One-layer cap µs/frame | Completed-work cost | Peak reserved bytes |
| ---: | ---: | ---: | ---: | ---: |
| 4 | 617.3 | 1064.4 | +72.4% | 13,010,448 → 4,336,816 |
| 128 | 10872.5 | 11381.7 | +4.7% | 13,010,448 → 4,336,816 |

The cap reduces peak payload from **12.41 MiB to 4.14 MiB (threefold)**. Default
mode may retain 8,673,632 bytes of older allocations alongside the live texture;
the one-layer cap reports zero retired bytes after successful submissions. Each
case records 171 budget waits across 180 timed updates in narrow mode (the first
update of each timed block starts after completion), and zero budget waits in
default mode. The existing queue-length bound can still wait independently.

This is a measured memory/parallelism tradeoff: low-overdraw throughput costs
about 72.4%, while the 128-quad workload costs about 4.7%. A tight cap is not an
automatic performance improvement. Keeping the default cap preserves queue
parallelism until actual allocation pressure occurs. Buffer/texture pooling,
same-size texture reuse on origin changes, adaptive admission and an oversized
live-working-set fallback remain separate implementation work.

Per-frame fields labelled CPU are elapsed owner commit/publication/submission
spans and can contain queue/budget waits; they are not processor execution-time
samples. Completed-work means include driver scheduling and completion polling.
No GPU-timestamp, native-window FPS/input-latency or energy claim is established.

## Reproduction and limits

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-budget-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-budget-run-1 -LayerBudgetOnly
# Repeat with fresh output directories for runs 2 and 3.
```

Build completes before measurement. The final validation and three focused
executions have 8, 5, 5 and 5 compiler observations, respectively, sampled every
500 ms, with no observed cargo/rustc/clippy processes. Shorter compiler processes
and unrelated host/GPU activity are not excluded. Metadata, process observations
and raw data are retained in each run folder; all 150 selected source hashes
were checked again before freezing this report.

Resident bytes here mean unique application-held isolated texture payload
reservations. Driver alignment/pooling, views/buffers, main targets, glyph/image
resources, other renderers and other device epochs are outside that count/cap.
The cap is not a measurement of physical GPU residency. The fresh reference
forces full-viewport layer bounds and no reused cache but shares the primitive
shaders; earlier independent analytical opacity/CPU blend checks remain in the
complete suite.

[Implementation and public contract](../../RETAINED_LAYER_BUDGET.md) describe the
admission/wait order. [Full acceptance tracking](../../RETAINED_IMPLEMENTATION_PROGRESS.md)
keeps native presentation, global physical budgets/device epochs, shared GPU
properties, general primitives, adaptive policy and platform qualification open.
