# Native Windows retained surface qualification

The ordinary Windows `--retained` gallery with `retained-wgpu` now paints shared
GPU snapshots through the native D3D11 scene. `WgpuFrameRenderer::new()` remains
the independent headless API; use `new_for_window` to select interoperability.
The implementation and remaining limits are in
[the decision record](ADR_RETAINED_NATIVE_SURFACE.md).

```powershell
mise exec -- cargo run --locked --offline -p gpui-box-example-desktop --features retained-wgpu -- --retained
./tooling/rendering-perf/run-native-surface.ps1 -Output target/native-run-1
./tooling/rendering-perf/run-native-surface.ps1 -Output target/native-run-2 -SkipBuild
./tooling/rendering-perf/run-native-surface.ps1 -Output target/native-delivery -SkipBuild -Delivery
./tooling/rendering-perf/run-native-surface.ps1 -Output target/native-delivery-control -SkipBuild -Delivery -ReadbackControl
```

The diagnostic feature enables upstream framebuffer readback for **verification**.
It is not needed by the ordinary native GPU painter. The runner hashes sources
before/after building and running, records the binary hash, checks the process exit
and result, and samples compiler processes every 500 ms. It launches hidden as
required for background helpers; actual visibility and explicit diagnostic draws
are reported. Native framebuffer checks do not establish an OS-visible present
latency or compositor buffer-age contract.

The native fixture compares three retained outputs and their CPU-bridge controls
against premultiplied alpha blending over a known backdrop, with tolerance one
channel value. It includes images, glyphs, shadows, isolated opacity, clipping,
overlapping normal GPUI quads, fractional content placement and changed extents.
The retained producer, resource rasterizer and source trees are dropped before these snapshots enter the
native scene. Replaying the scene must preserve its pixels. Eight retained
snapshots reject a ninth export; releasing owners allows retry. Reset must preserve
the native backend. A separate device-replacement check invokes the platform's
real device-recovery resource path with a new D3D11 identity, retains an old
snapshot without sampling it on the new device, then submits the next complete
frame through the normal `submit_for_window` entry point. It verifies exactly one
consumer recovery, a new surface on the new device, no redundant recovery for the
same identity, and all 3,072 recovered pixels with channel tolerance one. This is
a deterministic device replacement rather than a driver-induced TDR. If creation
of a matching hardware DX12 consumer fails, the implementation selects the
ordinary headless backend and leaves native-surface statistics absent so the
ordinary window-image readback bridge remains available; a no-DX12 adapter has
not yet been exercised on hardware.

A separate 40-update native framebuffer sequence samples each
new snapshot **before any producer finish/readback/poll**, then checks its pixels
and requires bounded texture reuse after native reads retire.

The normal gallery delivery check drives an actual background source burst through
the foreground executor, computed state, transaction, frame publication and native
painting. With the diagnostic feature it also crops the native D3D11 framebuffer
at the actual snapped canvas rectangle and compares every pixel byte-for-byte
with the retained target. Its native and readback-control runs report snapshot
copies separately. Existing equality, idle-render and completion-retirement checks
remain in that diagnostic.

The performance CSV records all samples at 128×96, 800×600 and 1600×900. Each
process uses four alternating rounds, six warmups and twenty timed samples per
mode per size: eighty samples/mode/size. Both modes use the same DX12 consumer,
frame input, full redraw and opaque root-color updates. The timer covers owner
commit/publication/submission and completed export. `finish` waits through a queue
sentinel after the export's raw ready signal. Native scene drawing, presentation,
the legacy sprite upload, framebuffer verification and setup are excluded.
Consequently these measurements assess export overhead, not whole-UI frame rate.

The allocation-per-frame prototype is preserved under
`performance/2026-11-12-native-surface/initial-probe` with its source ZIP, manifest
and raw observations. It regressed small outputs and motivated completion-safe
pooling. Final measurements and validation are recorded alongside it; no sample
filtering or removal of the initial regression is allowed.

The device-replacement qualification passed in three independent processes. Each
run checked the existing 97,792 fixture/control pixels, cache replay and 3,072
post-replacement pixels. The runs shared binary SHA-256
`3dbf355955e19cbb0bf984a8acfff2f568c721ccf33585945a8f88a5a8860893`; the source
manifest was unchanged before and after each run and no compiler or competing GPU
helper was observed. Raw qualification directories are
`target/native-recovery-verified-1`, `target/native-recovery-verified-2` and
`target/native-recovery-verified-3`; the compact durable record is
`performance/2026-11-12-native-surface/device-recovery.json`.
The same revision also reran the complete retained hardware matrix: 1,939
scenarios and 1,849 pixel comparisons passed on the Vulkan headless path,
confirming that the Windows recovery API did not regress the existing retained
pixel and lifecycle coverage. The normal gallery source-delivery diagnostic also
passed in native and readback-control modes, each comparing all 107,136 canvas
pixels against the actual D3D11 framebuffer.
