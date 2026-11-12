# Same-extent isolation texture reuse — 2026-11-12

Windows / NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Source base is
`585d6d9`; the final validation and three focused runs each carry 151 selected
source hashes, all verified against final working-tree bytes. The binary hash,
process audits and verification counts are in [verification.json](verification.json).

The complete hardware run passes **224 scenarios**: **221 byte-exact pixel
comparisons**, **one independent analytical origin-only case**, and **two backend
preflight/retry cases**. Core tests pass 116, gallery/font/hardware-click tests 12,
selected core/gallery/perf Clippy targets pass `-D warnings`, and the desktop
WGPU executable builds. Logs, PNGs and raw validation records are preserved.

## Correctness and lifetime checks

The new mixed cases contain nested isolation, rounded quads, alpha images and
real Chinese/Latin grayscale text. At scales 1 and 1.5, twelve alternating group
moves reuse both layer textures, initialize no texture allocation, write 96
camera/global bytes, write no opacity bytes, reuse image/glyph textures, and
incur zero allocation-budget waits. Clips that change physical extents still
replace textures. Both settings survive reset and retain pixel equivalence with
a fresh full-viewport, empty-cache reference.

The direct backend case moves only target bounds while supplying no subsequent
instance/resource writes. Moving the crop away from a stationary red rectangle
must clear the old raster to transparent; restoring the crop restores red.
Independent expected black/red pixels catch missed invalidation. Twenty-four
queued origin flips additionally exercise persistent buffer/texture use through
multiple submissions. Existing native completion/idle retirement and pressure
checks remain in the suite; the pressure control explicitly disables reuse.

Each layer owns distinct target buffers. One 16-byte camera and one 32-byte quad
global write occur before that frame's submission. All passes use the same values
for the target; the same queue orders previous reads before these writes. No
single mutable camera is switched between targets or passes inside one encoder.
Texture extent changes still use new completion-held allocation reservations.

## Completed-work controls

The same tree/renderer/cap alternates physical origins of one 1200×900 group on a
1400×1000 surface. Both modes rerasterize contents and redraw the full main
target; the only control is same-extent reuse versus forced replacement. One
texture is 1202×902×4 = **4,336,816 bytes** including the conservative fringe.
The root target and other resources are excluded from this layer payload counter.

Three executions each contain three rounds rotating control order, six warmups
and twenty timed frames followed by final completion. The table averages nine
completed blocks per mode (180 timed frames/cell). It is completed throughput,
not individual frame latency. Owner transaction/publication/submission and
completion are timed; readback/native-window paint are excluded. Per-frame CPU
fields are elapsed spans that can include queue/budget waits.

| Quads | Allocation cap | Reuse µs/frame | Replace µs/frame | Reuse time change | Max post-submit reserved bytes, reuse / replace |
| ---: | --- | ---: | ---: | ---: | ---: |
| 4 | 256 MiB | 765.5 | 769.4 | -0.5% | 4,336,816 / 13,010,448 |
| 4 | one layer | 674.2 | 1172.6 | -42.5% | 4,336,816 / 4,336,816 |
| 128 | 256 MiB | 10853.3 | 10729.7 | +1.2% | 4,336,816 / 13,010,448 |
| 128 | one layer | 10747.3 | 11347.7 | -5.3% | 4,336,816 / 4,336,816 |

With the default cap, reuse eliminates replacement churn and reduces observed
maximum reservations threefold (12.41 → 4.14 MiB) but shows no material throughput
benefit: light overdraw changes about −0.5%, heavy overdraw about +1.2%. Keep the
heavy first-run reuse block at 11,541 µs and host variation visible; no outliers
were removed. The small default-cap changes do not establish a speedup.

With the one-layer cap, reuse removes the old allocation-pressure serialization.
Light overdraw improves **1.74×** (about 42.5% less completed time), heavy overdraw
**1.06×** (about 5.3% less). Replacement records 171 budget waits across 180 timed
frames, versus zero with reuse. Queue-length backpressure remains possible in
both modes. Each replacement case requests **780,626,880 texture payload bytes**
across the measured updates, versus **zero new texture payload** for reuse;
these are application allocation requests rather than driver allocation traffic.
Both modes supply 48 target parameter bytes per update; forced replacement also
initializes a new 16-byte opacity uniform.

Mode-specific maxima use post-submit reservation snapshots, because the budget's
lifetime peak can include an earlier control mode. After allocation, explicit
leases remain until a later owner collection, so the snapshot captures that
submission's reservations. Raw block times, CPU spans, allocation/rebase counts,
waits and all reference checks are preserved in run-1 through run-3 and
[summary.json](summary.json).

## Reproduction and boundaries

```powershell
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-reuse-validation -ValidationOnly
./tooling/rendering-perf/run-retained-gpu.ps1 -Output target/layer-reuse-run-1 -LayerReuseOnly
# Repeat with fresh output directories for run 2 and run 3.
```

Each run builds before execution. The final full and three focused executions
each have eight compiler observations at 500 ms intervals, with no observed
cargo/rustc/clippy process. Shorter processes and unrelated host/GPU activity are
not excluded. The selected 151-source manifests were checked again when freezing
these artifacts. Earlier budget evidence remains historical; its current control
explicitly opts out of origin reuse to continue exercising allocation pressure.

Origin reuse retains/rerasterizes texture contents at a new world crop. It does
not implement GPU affine property trees or transform-only raster reuse. Clipping
or fractional movement can change extents and still require replacement. Payload
counters exclude driver alignment/pooling, buffers, root targets, other resources
and other device epochs. Reference rendering shares primitive shaders; it does
not independently qualify all shader semantics or native surface behavior.

[Implementation contract](../../RETAINED_LAYER_REUSE.md) and
[full acceptance tracking](../../RETAINED_IMPLEMENTATION_PROGRESS.md) retain the
remaining native presentation, shared GPU properties, general primitives,
adaptive caching, global physical budgets and platform acceptance work.
