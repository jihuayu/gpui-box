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
the native backend. A separate 40-update native framebuffer sequence samples each
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
