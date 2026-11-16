# Automatic responsive session validation — 2026-11-17

Windows x64, Rust1.98.1, locked/offline, existing research worktree.
This validates public update flow and correctness. No new timing benchmark,
allocation reduction or continuous frame/presentation qualification is claimed.

## Reproduction

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline environment::tests -- --include-ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

The ordinary suites leave adapter-dependent tests ignored. They are not counted
as passed unless explicitly run. Existing Windows linker-message notices remain;
retained-only compilation also has existing unused SpatialUpdate fields.

Final results: retained **337 passed**; text/WGPU **370 passed,12 ignored**;
hello **27 passed,2 ignored**; explicit environment suite **8 passed**, including
the actual adapter test. Library Clippy **passed** with `-D warnings`.

## Framework checks

Seven ordinary session tests and one explicitly executed hardware test cover:

- Container host widths70/130 through alternating ordinary source transactions
  and worker-thread latest-value inbox updates. Burst intermediate width90 is
  coalesced before settlement. Final red leaf size20x20/40x30, hit, source reads
  and complete CPU pixels match a fresh ordinary forced-full tree with manually
  selected styles. Effects see selected final styles exactly once. One source
  transaction is published per drain. Equal final values change no source,
  effect, visual revision or frame intent. Empty drains do no transaction.
- Environment viewport500x300 versus layout180/unbounded versus paint200x200;
  RTL and device-scale1.5 metadata; viewport producers followed by dependent
  container consumers declared in the opposite order. Paint-only viewport
  changes do not change the query viewport. `frame`, `local_frame`, definite
  `frame_with_constraints`, separate local/resolved in-viewport APIs and explicit
  environment capture use their declared distinct inputs.
- Invalid scale, invalid paint viewport, invalid ordinary source and rejected
  inbox payload. Finite authored translations compose to overflow after a new
  direction/source settlement: final geometry rejects, preserving session,
  sources, rules, stats, layout and old published revision. Queue rejects and
  consumes its failed batch with no retry loop.
- New keyed scope/source/output and nested viewport/container declarations in
  an active draft. Inline builder reads width70/120 after automatic settlement.
  Changed environment after a builder invalidates the earlier observation and
  rolls back. Repeated branch replacement retires sources/writers. A full
  Size-contained ConstraintBuilder reads tight120x80 after automatic selection
  and invokes its callback once. Existing allocation-builder suite covers its
  full-axis proof; this stage installs the same pre-observation checkpoint there.
- Legacy reparent violating a registered container ancestry rejects before
  publication. Key reorder updates real sibling positions and sync is a no-op;
  scope retirement detaches rules and queued source generations retire rather
  than revive. Ordinary append and binding in an active session immediately
  publish matching local layout/hit geometry. No-session behavior remains
  explicit; disabling the session retains last styles until later changes.
- Earlier LocalFrame pixels remain identical after all UI owners are dropped.

## Real shaped text

The hello regression uses bundled Noto Sans CJK SC and actual platform shaping,
owned glyph masks and CPU raster. Environment widths100/200/100/200 select
host widths60/140, then a container rule chooses leaf padding0/5. Actual wrapping
uses final content widths60/130. Published height equals real paragraph height
plus selected insets; heights change, visible glyph pixels are asserted and all
CPU bytes match an independent fresh ordinary forced-full Flex tree. After the
initial explicit capture, ordinary `edit` forwards environment changes and
automatically publishes local output without a responsive call. The old frame
survives UI/text-system/raster/paragraph owner teardown.

## Actual GPU output

Explicit adapter run: NVIDIA GeForce GTX1650Ti, Vulkan, NVIDIA driver571.96,
discrete GPU. Four actual LocalFrame submissions alternate ordinary transactions
and inbox drains at widths70/130/70/130. Readback pixels match independent
fresh forced-full ordinary layout for every frame. This is offscreen correctness
on the named device; it does not establish visible presentation, modern-GPU
heavy effects, sustained60/120Hz, CPU smooth frame delivery or input tails.

## Static checks and limits

Library Clippy `-D warnings` passes. An additional `--all-targets` run fails on
four pre-existing test-only lints outside this stage: collapsible_if in
layout/alignment_tests.rs and layout/baseline_tests.rs, items_after_test_module
in local_ink.rs, manual_is_multiple_of in pixel_snap/tests.rs. No allow suppression
was added and that broader check is not reported as passed.

Active sessions still use whole private tree candidates, repeated per-stage
previews/layout, flat rule/proof scans, source savepoints and complete local
geometry checks. Automatic checkpoint coverage is not an incremental scheduler.
No browser was needed for this stage because it changes scheduling, not Flex
geometry; the existing independent browser matrices remain in the full suites.
See the [public contract and remaining scope](../../RETAINED_AUTOMATIC_RESPONSIVE.md).
