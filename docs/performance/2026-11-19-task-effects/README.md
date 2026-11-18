# Reactive task effect verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `9c7f6e80f3fa83aa335d3bbc8646ab95799277e8`.
See [decision and costs](../../ADR_RETAINED_TASK_EFFECTS.md) and
[public contract](../../RETAINED_UI_TASKS.md).
Complete RFC stays active; "99%" is figurative common fluid/intuitive behavior,
without a numeric success-rate or p99 gate. No new performance claim.

## Controls

- Final input equality coalesces 1→2→1 and repeated cloned-factory declaration;
  1→2→3 launches only3. Old completion rejects and Applied does not restart.
- Invalid final opacity/source unwind retain the old source/task/frame without
  starting work. Failed removal or factory replacement retains registration.
- Capacity1 adopts current input, cancels old work, holds its permit until worker
  exit, coalesces later input and retries only the latest. Ordinary frame, local
  frame, constraints and viewport entrypoints retry without producers as well.
- Component ReadProp changes restart work without remounting; scope retirement
  cancels. Source-driven same-candidate producer retirement prevents restart;
  declaration followed by retirement never reserves/launches work.
- Input from a retiring foreign scope rejects the candidate; manual/event task
  ownership conflicts reject; removal restores manual ownership. Event source
  changes automatically drive task replacement in the same owner transaction.
- Computed projection settles before task capture; equal projected value keeps
  work. New factory deliberately replaces even equal input. Post-commit ordinary
  effect panic leaves adopted state and cancels undispatched launch; no implicit
  retry of terminal work.
- GPUI TestWindow uses its real host background executor. Source1→2→9 starts1
  and9 only; old task cancels; ready result adopts count90 and retained fill,
  installs a newer complete CPU picture with matching input scene, no pending
  images/error/admission slots. App/GPUI integration, not OS injection or
  displayed cadence evidence.
- Real Noto task-effect request changes reuse the direct/event task reference
  harness. Width90/145/65 and long/short/long text match complete numeric Flex
  geometry/baselines, following sibling positions and all CPU/GPU pixels. Old
  frame stays consumable after font/UI owner destruction; retired work rejects.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_task_effect_real_noto -- --ignored --nocapture
```

Windows MSVC / Rust-Cargo1.98.1 / locked-offline debug correctness. Final complete
retained-only754passed/1ignored and text/WGPU789/67; each14compiler doctests.
Hello89/35. Explicit task-effect real-Noto GPU1passed (three complete images)
on GTX1650Ti/Vulkan/driver571.96. Thirteen regular core task-effect controls,
one regular Noto control and one GPUI executor/image-installation control pass.
Library Clippy-Dwarnings, CPU desktop without WGPU and full retained-gpu harness
checks pass. Scoped format/diff,1,110local links across291
reachable documents and five inherited WIP hashes pass. Ignored cases do not
count as hardware execution. No displayed cadence or OS input injection claim.

Settlement currently scans registered task effects; equality/source observation,
sparse map COW, input clone and ordinary candidate/layout costs remain. This is
automatic parameter-driven task lifetime, without private scheduling, automatic
result callbacks, byte quotas, priority/fairness, KeepAlive activation or independent
native presentation. Source schema, Taffy and GPU instance formats are unchanged.
