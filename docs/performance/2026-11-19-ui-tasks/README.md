# Scope-owned UI task verification, 2026-11-19

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `5a4311d81097c535efc3c452281b540ae195c0ee`.
See [public contract](../../RETAINED_UI_TASKS.md) and
[decision/costs](../../ADR_RETAINED_UI_TASKS.md).
Complete RFC remains active. No performance, cadence or latency gain claim.

## Controls

- Cross-thread result completion; source callback runs only for current Ready
  output. Source error and invalid final opacity preserve old values/frame and
  Ready output for retry. Successful adoption cannot replay, including after an
  ordinary post-commit effect panic or a result retiring its own component.
- Same-key replacement, nested/manual/transactional component retirement,
  reused scope generations and owner destruction cancel old work. Keyed reorder
  and equal mount-once props retain tasks. Cancellation is visible to effect
  cleanup. Accepted business Command remains Accepted and can later succeed.
- Failed/unwinding construction never launches or cancels live work. Multiple
  starts at one key in one candidate keep the last only; same-candidate removal
  never launches. Worker/launch drop or panic wakes with Abandoned/Cancelled.
- Owner readiness and worker cancellation futures wake without polling timers.
  Foreign owners and wrong output types reject. Compile controls keep UiTask
  owner-local while UiTaskContext<u32> is Send+Sync.
- Actual GPUI background executor completes a task, ready wakes its owner, output
  updates source through normal adoption, and full CPU picture/input scene install
  settles with no active/pending/ready image backlog. This uses TestWindow, not OS
  input injection or native displayed-cadence measurement.
- Real Noto result payload contains owned TextPaint plus width. Width90→145→65,
  long→short→long paragraphs and different colors drive ordinary bindings/Flex.
  Complete LayoutResult/baselines, following sibling geometry and whole software
  images match fresh fixed numeric root/full reference. Explicit GPU compares the
  same three complete images. Retired tasks reject late completion and old text
  frames still render after UI/font/raster owners drop.

The real text control found and fixed missing layout-session reuse on task output
adoption: a no-producer UI must use its published constraints/viewport to validate
the complete result before marking it applied. An early color assertion was
corrected to inspect current fill_color, respecting the existing retained command
record/P1 material split. Current full suites supersede earlier partial runs.

## Reproduce

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_scoped_task_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Windows MSVC, Rust/Cargo1.98.1, locked/offline debug correctness. Explicit GPU
adapter GTX1650Ti / Vulkan / driver571.96; one hardware test passed with three
complete-image comparisons. Ignored full-suite counts are not executed hardware
coverage. Final retained-only726passed/1ignored and text/WGPU761/66; each runs
13compiler doctests. Hello84passed/33ignored. Library Clippy-Dwarnings, CPU desktop
and complete retained-gpu harness checks pass. Scoped rustfmt/diff,1097local links
and five inherited WIP hashes pass. Final suites cover13task-specific core controls.

No new native application mode was added. Existing owner host/event tests remain
regression controls. This validates task lifetime and result integration, not
application-wide concurrency budgets, KeepAlive, independent presentation or
sustained60/120Hz. Cancellation is cooperative and does not roll back external IO.
