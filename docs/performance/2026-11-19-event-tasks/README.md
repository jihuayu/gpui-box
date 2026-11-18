# Event-scoped UI task verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `99ef72c68e2ec149095ebd21c5420608216bc72a`.
See [decision and ordering](../../ADR_RETAINED_EVENT_TASKS.md),
[task contract](../../RETAINED_UI_TASKS.md) and
[owner input](../../RETAINED_INPUT_PROCESSING.md).
Complete RFC stays active; no performance/cadence/latency gain claim.

## Controls

- Primary down/up Activate stages task/source update together; launch runs only
  after adoption. Ready worker result updates color through normal apply_task,
  complete CPU output matches independent full reference, and explicit hardware
  control compares the complete GPU image. Router stays on installed scene until
  explicit matching scene adoption.
- Handler error, invalid final opacity and handler unwind discard new task,
  permit, source writes and after_commit work, retaining prior router/capture/frame.
- Busy fallback adopts loading color while preserving old Running task. Explicit
  cancellation adopts only on success; failed replacement/cancel keeps live old
  work, while failed new declaration is Cancelled and never launches.
- Standalone dispatch shares staging and rejects stale delayed event before code.
  Source-triggered producer removal in that same event retires declared task and
  prevents launch. Bubbling parent/child handlers with equal keys own separate
  tasks; retiring child cannot cancel parent's task.
- Actual GPUI TestWindow simulates click, EventContext declares scope task,
  existing executor completes result and owner applies source/color. Complete CPU
  image has a newer revision and expected retained fill, with matching installed
  input scene, no error and no remaining admission slots. This is not OS input
  injection or displayed-cadence evidence.
- Real Noto harness retains direct task start as a control and adds input-event
  start. Long/short/long text and width90/145/65 give complete numeric Flex reference
  geometry/baselines, following sibling placement and full CPU/GPU pixels. Old
  glyph frames outlive all producer/font owners; retired late result rejects.

Registry entries now share an Rc map and copy only on actual mutation. Callback
lookup, input graph construction, sources/producers/candidate/layout costs remain.
No timing complexity or memory reduction is inferred from tests. The source schema,
Taffy solver and GPU instance formats stay unchanged.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline hardware_event_task_result -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_event_task_real_noto -- --ignored --nocapture
```

Windows MSVC / Rust-Cargo1.98.1 / locked-offline debug correctness. Final
retained-only741passed/1ignored, text/WGPU776/67, each13compiler doctests;
hello87/34. Explicit event-task GPU1passed (one complete image), real-Noto event-task
GPU1passed (three complete images) on GTX1650Ti/Vulkan/571.96. Seven regular core
event-task controls plus one ignored/explicit hardware control complement existing
task/admission/input suites. Library Clippy-Dwarnings, CPU desktop and retained-gpu
harness checks pass. Scoped format/diff,1121local links and five WIP hashes pass.
Final suites and hardware tests cover final source.
Ignored cases are not executed hardware coverage. Native independent presentation,
KeepAlive activation, automatic async hooks, resource byte limits and task priority
remain open; “99%” is figurative common fluid/intuitive behavior, not a numeric gate.
