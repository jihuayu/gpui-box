# Retained rendering implementation progress

The active implementation goal covers the complete runtime requested on
2026-11-11. Work continues in `codex/rendering-performance-research`.

The subsequent [GPU integration stage](RETAINED_GPU_INTEGRATION.md) connects
FramePacket deltas to persistent quad allocations and bounded in-flight submits.
The [clip, scroll and damage stage](RETAINED_PROPERTIES_DAMAGE.md) adds shared
property chains, normal UI scroll events and measured partial GPU redraw.
The [group-compositing stage](RETAINED_GROUP_COMPOSITING.md) adds isolated opacity,
content-cache invalidation and a correct readback bridge for normal window paint.
The [reactive update stage](RETAINED_REACTIVE_UPDATES.md) connects typed source/sink
bindings, nested event transactions, lifecycle cleanup and measured batch dispatch.
The [logical scope stage](RETAINED_LOGICAL_SCOPES.md) separates component identity
from render parentage, retaining keyed state and cleaning up portal output.
The [owned measurement stage](RETAINED_OWNED_MEASUREMENT.md) adds intrinsic image
and fixed custom inputs, baseline dependencies and actual measure-call counts.

## Current integrated foundation

`gpui-box/retained` owns persistent rectangle nodes and a Taffy tree. Nodes carry
tree identity, slot and generation. Removing a subtree invalidates its handles.
The owner validates property transactions before applying changes, coalesces
field writes and compares final values with committed state. A transaction that
restores the previous value requests no frame.

Color, layout style, translation, clip, scroll and child reordering produce independent update
intents. Style changes enter Taffy through its normal setters. Color and
translation skip layout. Child reorder preserves node identity. Immutable
`FramePacket` values contain owned paint items and shared local command chunks.
Translation and layout reuse existing commands. Hit testing reads the same
published geometry as painting. Root translation propagates to descendants.

The desktop entry `--retained` opens a GPUI gallery. Its normal click handlers
submit transactions, call `notify` when a commit requests output, and paint the
published frame in the GPUI canvas prepaint/paint lifecycle.

```powershell
mise exec -- cargo run --locked -p gpui-box-example-desktop -- --retained
mise exec -- cargo test --locked -p gpui-box --features retained
```

This adapter declares default backend capabilities and uses ordinary GPUI paint.
The earlier WGPU single-quad upload prototype remains a separate experiment.
`InstancePatch` is a routing intent for a consumer declaring that capability.
The normal canvas adapter currently selects `RepaintChunks` for color changes.
With the `retained-wgpu` example feature, the same click and prepaint flow also
submits the frame to a persistent offscreen GPU consumer. The explicit hardware
click test passed pixel comparison at the actual window scale.

## Full-goal acceptance tracking

| Requested area | Current implementation | Remaining acceptance work |
| --- | --- | --- |
| Normal UI update chain | Desktop click → signal → typed property sink → transaction → layout → FramePacket → paint/persistent WGPU, with hardware pixel checks | Native-surface presentation and complete general routing |
| P0 equality and transactions | Source equality before fanout, final-value coalescing, atomic sink validation, nested savepoints, one outer frame intent, idle-render assertions | Computed/effect graph, cross-event producer scheduling and real OS idle-wakeup/energy measurements |
| P0–P5 routing | Rectangle color/translation/style/reorder intents and backend capability input | General primitive capabilities, explicit fallback reasons, semantic/resource channels and actual GPU patch dispatch |
| Logical / Render Tree | Independent generation-checked scopes, zero/multiple outputs and portals, keyed reorder retaining state/render/chunks, subtree source/binding/output cleanup; gallery and hardware validation | Atomic structural batches, portal reparenting, event/focus/semantics lifetime and legacy islands; eliminate whole-scope/order scans |
| Persistent Taffy | Long-lived nodes, style/children setters, owned image intrinsic/fixed custom snapshots, baseline/overflow updates, actual leaf/measure-call counts, root cache and resize handling | Owned paragraph snapshots, constraint-dependent custom inputs, independent islands, full node visits/cache-hit counters |
| Paint chunks / property trees | Shared local fill commands; sparse transform/clip/scroll/effect chains with inherited state, clip-aware geometry/hit testing | Stable chunk IDs, full affine transforms, selective geometry extraction and spatial indices |
| GPU arena | Generic CPU allocation table with generations and deferred slot reuse, WGPU geometric growth and completion-held buffer/binding leases | General resource/material records, budgets, compaction and other backends |
| Batch GPU updates | Adjacent upload merging; 10,000-object translation and 10,000-signal color pixel verification; CPU per-signal/coalesced/typed-batch comparison; completed-work redraw A/B | Sustained 60/120Hz animation, general batch/fragmented order throughput and tuning |
| P2 property / composite | Translation/clip/scroll reuse commands and update GPU instances/hit geometry; isolated subtree opacity supports nesting and 16-byte property-only updates | GPU transform/clip indirection, texture import for window presentation and NativeView geometry |
| Text / images / complex primitives | Existing legacy GPUI functionality; owned intrinsic image and fixed-content measurement drives rectangle geometry | Actual retained primitive types, decoded resources, font epochs, shaping/wrapping and corresponding pixels |
| Flex / responsive contract | Pinned Taffy style input and persistent resize | Public subset validation, RTL/baseline/rounding tests, environment subscriptions, constraint/container queries and adaptive structures |
| Damage / cache / compositor | Old/new clipped ink/effect damage; preserved offscreen target, scissored clear/replay; Full/Damage/area-threshold Auto; isolated layer content cache and cached/uncached A/B; live-layer 256 MiB guard | Tight layer bounds, cross-frame physical memory accounting, cache admission/LRU, measured cost model, multiple damage regions, native surface buffer-age contract |
| FramePacket / frames in flight | Owned snapshots plus base-revision deltas, skip-frame resync, stale-frame rejection, bounded queue, completion retirement, explicit poll/finish and renderer reset | Forced device/surface loss, resource epochs, host idle-completion scheduling and producer queue coalescing |

## Verification and present limits

Core regression cases cover final-value equality, batch rejection without partial
application, color/translation route coexistence, shared chunks, inherited
translation hit testing, persistent-vs-fresh Flex geometry, zero-size viewport,
subtree removal, slot reuse, invalid viewport/translation and stable child reorder.

Frame publication currently walks all live nodes when visual output changes and
creates a new paint-item array. Chunk commands are shared across frames. The
completed CPU frame is reused on unchanged input. This traversal must be replaced
or segmented before claiming a sublinear complete scene-update path.

The initial style surface exposes the pinned Taffy Style. The full gpui-box Flex
API and constrained CSS subset remain pending. Geometry currently contains
floating-point border bounds, translation, rectangular clip, scroll and isolated opacity. Resources and
accessibility are tracked above for their subsequent implementation stages.

The progress table uses implementation and verification evidence. The active goal
remains open until the complete requested flow and corresponding acceptance work
are finished.

## Foundation validation

- Eight core tests passed with `cargo test -p gpui-box --features retained`.
- The GPUI `normal_click` integration test passed. It locates the rendered buttons,
  dispatches mouse clicks, runs the event loop and checks the actual published
  frame. Changing color changes the display commands. A temporary color restored
  within one transaction retains the frame revision and shared paint-item array.
- The desktop executable built successfully. Core and shared-example Clippy
  checks run with `-D warnings`.
- The existing CLI and Android host lifecycle tests passed, three cases total.

The GPUI integration test uses `gpui/test-support`, which adds its upstream test
and benchmark dependency closure to Cargo.lock. Native screenshot verification
and cross-device qualification remain in the acceptance table.
