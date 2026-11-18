# Combined component input verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `2e367145935ff443a81216e86a7555103bf7d64b`.
See [input order, ownership and costs](../../RETAINED_INPUT_PROCESSING.md).
Complete RFC stays active; no native cadence/latency or performance gain claim.

## Controls

- PrimaryDown focus callback, PrimaryUp bubbling and activation, then Wheel
  callbacks/default scrolling share ordinary state/feedback publication. Parent
  sees staged child writes. Three complete CPU/GPU images match independent full
  fixed Flex with numeric child translation and explicit clip. Input scene remains
  the installed revision until explicit adopt_input_scene; old frames outlive owner.
- stop_propagation suppresses only remaining ancestor delivery and wheel still
  consumes. prevent_default leaves bubbling active, later ancestors observe it,
  wheel consumes zero and returns residual. Focus remains transferred while reveal
  can be prevented. PointerUp cancellation suppresses Activate, releases capture
  and retains parent PointerUp callback.
- Handler error and final invalid opacity roll back all callback source writes,
  feedback, router focus/capture/hover, frame and queued business work. Retrying
  succeeds. Post-commit panic leaves adopted source/frame and router capture aligned.
- Producer disabling a formerly valid wheel target commits callback state and
  skips final default with zero consumption. Malformed/foreign default request and
  nonfinite position reject before callbacks or live router mutation.
- Real Noto ComponentContext label uses handle_input plus adopt_input_scene for
  hover/down/up/leave. Width 60→125 and pressed opacity 0.6 produce four complete
  CPU/GPU images and full LayoutResult/baseline matching independent numeric full
  Flex. A real callback queues owned text evidence after commit; text stays an
  independent binding. Old glyph frame outlives UI/font/raster owners.

Original standalone scoped-event/style/scroll controls remain in complete suites.
The shared Noto harness retains its previous explicit-feedback path as a control.

## Results and limits

Final retained suite: 713 passed, 1 ignored; 11 compiler doctests passed.
Final text/WGPU suite: 747 passed, 66 ignored; 11 compiler doctests passed.
Final hello suite: 81 passed, 32 ignored.
Explicit input-pump GPU: 1 passed, three complete image comparisons.
Explicit real Noto input-pump GPU: 1 passed, four complete image comparisons.
Hardware: GTX 1650 Ti / Vulkan / driver 571.96. These explicit executions overlap
ignored full-suite tests; ignored counts alone are not executed hardware evidence.
Final library Clippy with `-D warnings` passed without lint suppression.
Complete retained-gpu harness check, scoped rustfmt, diff whitespace, 334 local
Markdown links and five inherited WIP hashes passed. Final suites and explicit
GPU controls cover the final source. Rust/Cargo 1.98.1, Windows MSVC,
locked/offline debug correctness. Final validation completed 2026-11-19 +08:00.
Early six regular pump controls passed before subsequent panic/default/host-adopt
controls were added. Final suites cover the final source rather than treating
those early targeted counts as completion.

Debug correctness/offscreen equivalence does not prove native visible cadence or
input-to-display latency. Current map construction, registry lookup, effects copies,
default preflight and normal candidate/layout costs remain. Source schema 6 and
solver/GPU format unchanged; Wheel/wheel_delta extend public input types.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_input_pump -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_input_pump_real_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
