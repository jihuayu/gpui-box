# Rendering performance experiments

The follow-up [single-quad GPU upload prototype](GPU_PROTOTYPE.md) runs actual
hardware WGPU comparisons and pixel checks through an opt-in backend feature.

This is a research tool for the [rendering RFC](https://app.notion.com/p/415d83088d4e818290cddca2f104e65c).
It changes no engine behavior. Layout cases execute the same pinned Taffy 0.15.0
as GPUI. `gpui-scene` cases execute the checked-in `Scene` implementation directly.

From the repository root on Windows:

```powershell
./tooling/rendering-perf/run.ps1 -Output target/rendering-perf -Runs 3
```

The wrapper builds first, records toolchain/source/hardware metadata, then runs
serially. Stop other compilation and heavy workloads while measuring. There is
no attempt to control clocks, temperatures or background system processes.

For dependency-light layout experiments on any Rust host:

```sh
mise exec -- cargo run --release --locked -p gpui-box-rendering-perf -- --output target/layout-perf
```

For layout and the real GPUI Scene on supported hosts:

```sh
mise exec -- cargo run --release --locked -p gpui-box-rendering-perf --features gpui-scene -- --output target/scene-perf
mise exec -- cargo test --locked -p gpui-box-rendering-perf --features gpui-scene
```

Offline builds require dependencies to be cached; remove `--offline` in the
wrapper if provisioning a new machine. `mise` selects the pinned Rust toolchain.
The Scene feature needs GPUI's normal platform build prerequisites, but creates
no native window and executes no GPU API.

## What the cases mean

- `layout_clear_build_compute`: reuse tree allocation, clear all nodes, recreate
  the same flat wrapping Flex tree and compute. This models the lifecycle in
  `TaffyLayoutEngine`, not the rest of a GPUI frame.
- `layout_cached_unrounded`: compute an unchanged persistent root with rounding
  disabled, as in GPUI. No geometry extraction or world-coordinate updates.
- `layout_equal_style_unguarded`: clone and set one identical leaf style, then
  compute. Taffy's setter invalidates without checking equality.
- `layout_equal_style_guarded`: compare the same style and skip the setter and
  layout when equal. This is a local CPU control, not an implemented UI router.
- `layout_one_intrinsic_change`: alternate one measured leaf's intrinsic width
  between 24 and 48; `set_node_context` marks it dirty, then compute from the root.
- `layout_viewport_resize`: alternate 1000 and 984 logical units of available width.
- `layout_cached_rounded_control`: unchanged root with Taffy's default rounding
  enabled. This control is intentionally different from GPUI configuration.
- `scene_fresh_record_finish`: clear a reused Scene, generate visible separated
  quads, insert and finish. Includes primitive creation and bounds-tree work.
- `scene_cached_replay_finish`: clear a reused Scene, replay all previous paint
  operations, finish. Includes per-primitive cloning, insertion and ordering.
- `scene_one_color_cached_rest`: alternate one quad's background and replay all
  others; finish as usual. Isolates the Scene stage of a one-color update with
  maximally reused surrounding paint, without component/interaction work.
- `scene_finish_already_sorted`: repeat `finish` on an already sorted scene;
  this favorable input does not measure worst-case sorting.
- `scene_batch_iteration`: count batches on a completed Scene. Few draw batches
  do not imply that batch iteration avoids visiting individual primitives.
- `scene_grouped_cached_replay` / `scene_grouped_one_color`: group the same
  non-overlapping quads in one existing GPUI paint layer, then replay unchanged
  content or change one color. This avoids a bounds-tree insertion per quad but
  still records/copies every primitive. This layer is an ordering group, not a
  texture, raster cache, or native compositor surface.

The leaf measure callback returns owned scalar intrinsic sizes. It does not shape
text, consult fonts, or emulate Paragraph. The flat, wide tree is a stress case;
do not extrapolate its timings to a nested application or every Flex layout.
Nodes in CSV denote leaf/quad count (layout has one additional root).

`leaf_layouts` counts Taffy's leaf callback entries and `measures` its actual
intrinsic-size closure calls. Neither is a count of all layout-node visits or
parent/sibling work. A low measure count can coexist with expensive Flex layout.
The counters and `black_box` have small CPU costs included in measurements.

Each case has eight warmup operations followed by adaptive batch calibration.
Each measured sample contains a fixed number of operations targeting at least
1ms per sample (slow operations use a batch of one). `samples.csv` contains the
raw duration and structural counters of every batch. `summary.csv` uses
nearest-rank p50/p95/p99 of **batch-average ns/op**. These are neither individual
update latency percentiles nor application frame p95/p99. Sub-microsecond cases
primarily demonstrate the absence of traversal, not portable latency guarantees.

`modeled_instance_bytes` is exactly quad count × `size_of::<gpui::Quad>()`, based
on the backend's full array upload. It excludes alignment, globals, textures,
paths and driver transfers. It is **not measured GPU traffic**; this tool performs
no upload. Layout-only builds have zero in this column.

Excluded: Signal routing, component construction, real text measurement, hit-test
and semantics rebuilding, snapshot allocation, GPU upload/encoding/execution,
resource fences, raster cache, swapchain presentation, input-to-photon, energy,
and retained-memory profiling. Those need separate instruments and real devices.

See [the research report](../../docs/RENDERING_PERFORMANCE_RESEARCH.md) for source
evidence, observed results and the next experiments.
