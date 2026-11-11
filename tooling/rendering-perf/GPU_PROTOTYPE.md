# Single-quad GPU upload prototype

The `gpu-prototype` feature runs the existing GPUI WGPU quad pipeline on a hardware
adapter. Three paths share the shader, blend mode, Quad ABI, target format and
1200×400 offscreen target.

| Path | Upload | Frame preparation |
| --- | --- | --- |
| `full` | N × 160 bytes | Existing `WgpuRendererCore::render_frame`, including globals, instance bindings and scene batch iteration |
| `retained_full` | N × 160 bytes | Stable buffer/binding, one quad draw, fixed globals |
| `patch` | 160 bytes | Same stable preparation as `retained_full`, one slot write |

`patch` versus `retained_full` isolates the local-write specialization. Comparison
with `full` also includes the benefits of stable bindings and bypassed full-scene
preparation. All three clear the full target and draw every quad.

Run from the repository root on Windows with cached dependencies.

```powershell
./tooling/rendering-perf/run-gpu.ps1 -Output target/quad-patch-gpu-final -Samples 201 -Rounds 3
```

On other hosts build and run the binary, then derive the summaries.

```sh
mise exec -- cargo run --release --locked -p gpui-box-rendering-perf --features gpu-prototype --bin quad-patch-gpu -- --output target/quad-patch-gpu --samples 201 --rounds 3
python tooling/rendering-perf/summarize_gpu.py target/quad-patch-gpu
```

An offline-cache miss can be provisioned by removing `--offline` from the wrapper.
The feature `gpu-backend` compiles the WGPU dependency with the prototype disabled
for checking the default backend configuration through the root workspace.

## Scope and ownership

The setter accepts normalized finite opaque HSLA for a fixed scene of solid quads.
Geometry, draw order, clip, count and target size are immutable. A session-scoped
handle identifies one sorted quad slot. Recreate the session for a new structure
or target. Session IDs prevent a retained handle from addressing a replacement
scene. Foreign handles, invalid slots, NaN and translucent colors return errors.
Equal values produce zero writes and zero draws. A failed GPU update poisons the
session so partial state cannot silently become the basis of another patch.

The private buffer belongs to its session. `queue.write_buffer` stages writes,
and the same queue executes them after earlier submissions and before the new
draw. This prototype waits for completion on every sample and never overwrites
mapped memory or reuses a slot for a different object. Frames in flight, deletion,
compaction and device/surface recovery need further lifecycle integration.

## Verification and measurement

The binary compares full pixel buffers after six updates per scene size. It also
requires exactly nine changed pixels for each 3×3 rectangle. It tests stale/foreign
handles, invalid slots, invalid colors and equality. Final images for all three
paths must match. PNGs preserve the validated output.

Each path receives 40 warmup updates. For every measured update, the three paths
run in rotating order. Each returns a single update sample, including a blocking
completion wait. `summary.csv` uses nearest-rank p50/p95/p99 per round.

- `instance_bytes` and `instance_writes` count the actual instance `write_buffer`
  calls and their payload. Globals and query buffers are separate.
- `upload_ns` measures CPU time preparing/writing instances. The original path
  includes instance-binding construction. GPU DMA is not timed separately.
- `submit_ns` measures encoder finish and queue submit.
- `cpu_ns` spans the typed color assignment through queue submission.
- `completion_ns` spans the same update through GPU completion/poll return.
- `gpu_draw_ns` uses GPU timestamps at the beginning/end of the render pass.
  It includes clear and drawing, excludes preceding upload copies and timestamp
  readback. A missing timestamp capability yields an empty CSV cell.

Timestamp resolve/copy commands are in the measured submission. Timestamp map/read
occurs after the completion metric. Pixel readback and PNG encoding happen outside
timing samples. One outstanding submission provides comparable update completion
latency, with host/driver wait overhead included. Continuous pipelined frame
throughput and native-window presentation require a separate experiment.

The hardware adapter is selected from Vulkan/GL by the existing GPUI instance
configuration, preferring discrete then integrated GPUs and rejecting CPU devices.
The measured adapter and driver are recorded. This Windows run exercises WGPU
Vulkan. The distribution's native Windows DirectX 11 renderer has a separate
upload implementation.
