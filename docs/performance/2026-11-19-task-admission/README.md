# Scoped task admission verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `db06c2c75fc8a9d6d729baf75d6c63dbdef6a752`.
See [decision and costs](../../ADR_RETAINED_UI_TASK_ADMISSION.md) and
[public usage](../../RETAINED_UI_TASKS.md). Complete RFC stays active.

## Controls

- Capacity1 rejects replacement before changing its old Running task. Explicit
  cancel changes status but leaves in_use1 until the cancelled context completes
  or drops on a real worker thread; capacity wake then allows retry.
- Ready output keeps a slot through failed source apply, frees it on successful
  adoption or cancelled destruction. Payload destructor observes the reservation
  still held and can inspect pool stats without a mutex deadlock.
- Two independent logical owners share one capacity. Failed construction releases
  only the provisional new reservation, preserves old work and never launches it.
  Owner Drop cancels without freeing outstanding escaped worker contexts early.
- Eight actual threads each own a separate LogicalUi sharing capacity3. Exactly
  three accept and retain outputs, five reject; peak/in_use never exceed3.
- Busy is recoverable inside a complete edit: normal background color/loading
  fallback adopts while old work continues. Invalid final opacity releases the
  provisional task slot and preserves the old task/complete frame.
- Invalid capacity0, duplicate capacity receiver and live pool migration reject.
  Capacity observations do not reserve. Receiver drops/reopens and weak receiver
  closes after pool lifetime. All regular task lifecycle controls remain active.
- Real GPUI host TestWindow: capacity1 fills, replacement returns Busy, old worker
  cancellation retains slot, executor drops context and capacity wakes owner.
  Retry completes count73 and a retained color update; complete CPU image installs
  with newer revision, expected fill and matching router scene. No polling timer.

These are correctness/admission invariants. They do not benchmark execution time,
bound arbitrary memory/CPU duration, inject OS input or verify displayed cadence.
The default64 is a configurable conservative count, not a measured optimum or
number of executor threads. Hosts still choose actual concurrency and priorities.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_scoped_task_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Windows MSVC / Rust-Cargo1.98.1 / locked-offline debug correctness. Existing
real Noto task-output CPU/GPU full-reference control remains regression coverage,
including text width/wrapping and immutable old-frame lifetime. No new renderer
algorithm or pixel equivalence shortcut. Final retained-only734passed/1ignored,
text/WGPU769/66, each13compiler doctests; hello85/33. Explicit Noto GPU1passed,
three complete image comparisons on GTX1650Ti/Vulkan/571.96. Library
Clippy-Dwarnings, CPU desktop and retained-gpu harness checks pass. Scoped format/
diff,1108local links and five inherited WIP hashes pass. Eight new core admission
controls complement the existing task controls; final suites include all changes.
Ignored is not
executed hardware coverage. No sustained60/120Hz or input-to-display latency claim.
