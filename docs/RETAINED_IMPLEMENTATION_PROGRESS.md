# Retained rendering implementation progress

The active implementation goal covers the complete runtime requested on
2026-11-11. Work continues in `codex/rendering-performance-research`.

The subsequent [GPU integration stage](RETAINED_GPU_INTEGRATION.md) connects
FramePacket deltas to persistent quad allocations and bounded in-flight submits.

## Current integrated foundation

`gpui-box/retained` owns persistent rectangle nodes and a Taffy tree. Nodes carry
tree identity, slot and generation. Removing a subtree invalidates its handles.
The owner validates property transactions before applying changes, coalesces
field writes and compares final values with committed state. A transaction that
restores the previous value requests no frame.

Color, layout style, translation and child reordering produce independent update
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
| Normal UI update chain | Desktop click → transaction → layout → FramePacket → ordinary paint plus optional persistent WGPU, with hardware pixel checks | Native-surface presentation, reactive sources/property sinks and complete routing |
| P0 equality and transactions | Final-value comparison, batch validation, duplicate field coalescing, no frame request on equality | Reactive sources, nested transaction boundaries, event scheduling and idle-wakeup measurements |
| P0–P5 routing | Rectangle color/translation/style/reorder intents and backend capability input | General primitive capabilities, explicit fallback reasons, semantic/resource channels and actual GPU patch dispatch |
| Logical / Render Tree | Persistent one-to-one rectangle identity, children, removal and generation | Separate logical scopes and render identities, portals, keyed reconciliation, event/semantics lifetime and legacy islands |
| Persistent Taffy | Long-lived nodes, style/children setters, root cache and resize handling | Owned text/image/custom measure snapshots, baseline/overflow dependencies, independent islands and layout visit counters |
| Paint chunks / property trees | Shared local fill commands and inherited translation | Stable chunk IDs, separate clip/effect/scroll trees, selective geometry extraction and spatial indices |
| GPU arena | Generic CPU allocation table with generations and deferred slot reuse, WGPU geometric growth and completion-held buffer/binding leases | General resource/material records, budgets, compaction and other backends |
| Batch GPU updates | Adjacent upload merging, actual 160/480-byte writes and 10,000-object batch translation pixel verification | Sustained 60/120Hz animation, throughput/latency comparison and tuning |
| P2 property / composite | Translation reuses command chunks and updates hit geometry | GPU transform/opacity/scroll, group isolation and NativeView geometry |
| Text / images / complex primitives | Existing legacy GPUI functionality and previous source audit | New retained primitive types, resource events, shaping and pixel regressions |
| Flex / responsive contract | Pinned Taffy style input and persistent resize | Public subset validation, RTL/baseline/rounding tests, environment subscriptions, constraint/container queries and adaptive structures |
| Damage / cache / compositor | Existing full-frame GPUI fallback | Old/new ink damage, buffer preservation rules, raster cache budgets, eviction and strategy A/B |
| FramePacket / frames in flight | Owned snapshots plus base-revision deltas, skip-frame resync, stale-frame rejection, bounded queue, completion retirement and explicit renderer reset | Forced device/surface loss, resource epochs, background completion pumping and producer queue coalescing |

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
floating-point border bounds and translation. Clip, opacity, resources and
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
