# ReadProp verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `a96c48eab87d599ce499a4bf38bf9e8b2973184e`.
See [contract and costs](../../RETAINED_READ_PROPS.md).
Full RFC remains active. No wall-time/FPS/memory or native smoothness gain claimed.

## Controls

Eight regular core cases plus one explicit ignored hardware wrapper:

- A Content parent updates scope-owned Row props for2keys. Child mount creates
  local height, computed layout/color and bindings once. Changed widths/colors,
  reverse order and key2→key3 replacement preserve key1's scope/state/output/
  prop handle. Mount counts2→2→3; retained event-style handles read current Row.
  Resolved/local session ×3models yields6complete CPU/GPU comparisons against
  independent ordinary full Flex. Old frames survive owner retirement.
- Two props feed one derived sum and one effect. Repeated draft writes expose
  current derived values37then40, but sinks/effect publish once with final
  tuple(15,25,40). Restore-to-original emits no effect and preserves exact frame.
  ReactiveTransaction read_prop sees the adopted input.
- Parent updates one prop consumed by a direct color binding and persistent
  child Content producer. Ordinary ancestry orders parent before child; logs
 20→35 and output color35 verify source and sink propagation together.
- State/computed namespace collision, changed T, wrong runtime/scope and retired
  generations reject. Recreate same key gives a different prop identity/default.
- Failed derived calculation returns InvalidStyle with unchanged source/counters/
  frame/effect state; invalid final opacity also rolls back a successful tentative
  input write. A handled stage error leaves the old prepared input and permits a
  later valid replacement in the same edit.
- A late producer changes an input already consumed by an earlier producer.
  Final source validation rejects without replay and retains old input/pixels.
- A failed provisional prop handle never revives. A newly created prop with a
  newly created computed source/direct binding is rewritten10→20→10→30 in the
  same draft; final derived31 and color30 adopt. Undeclared computed prop reads
  reject by the ordinary dependency contract.
- Surviving root computed depends on a child prop; child owner retirement rejects
  with SignalInUse and preserves both source and scope instead of dangling it.

Two compile-fail doctests require ReadProp not be accepted by transaction.set and
not satisfy Send. They verify the real type/API compiler boundary.

Real Noto uses one mounted text node and ReadProp<Option<TextPaint>> binding
inside Auto Grid with Unbounded normal axes. A Content parent replaces long→
short→long prepared paragraph with three colors, while mount count stays1 and
the same captured prop reads current paint. Independent numeric full Flex uses
actual paragraph natural metrics. Complete title LayoutResult/baseline and every
CPU/GPU pixel agree;3hardware submissions/images. Old glyph output survives UI/
font/raster retirement.

Production factors responsive source staging and reuses it for typed prop
replacements. No Source field, layout/kernel/GPU record, source schema or renderer
algorithm changes. Whole regression suites cover responsive propagation and
existing source/computed/producer lifecycle as well as new prop controls.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 622 passed,1ignored |
| Final complete text/WGPU | 656 passed,55ignored |
| Final complete hello | 70 passed,21ignored |
| ReadProp compile-fail doctests | 2 passed in both core configurations |
| Eight core cases / real-Noto CPU | Included in suites above |
| Explicit keyed-payload GPU | 1 passed,6submissions/6complete images |
| Explicit real-Noto GPU | 1 passed,3submissions/3complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness check | Passed |
| Scoped format/diff and286local links across5documents | Passed |
| Five unrelated WIP hashes | Unchanged |

All production/test changes were fixed before final complete suites and hardware
controls. Counts overlap; ignored cases count only when explicitly executed.
Existing retained-only SpatialUpdate unused-field and dependency linker warnings
remain without suppression. Shared responsive source staging is covered by the
complete responsive/computed/lifecycle regressions as well as these prop cases.

Prop replacement clones the prepared map and scans/sorts the derived graph.
Ordinary producer planning/candidate/geometry and scope-order costs remain.
Per-row writes may settle the graph repeatedly; no batch or throughput benefit
is claimed. Finite offscreen references do not prove visible input latency,
modern desktop effects, sustained cadence or continuous complete CPU frames.
"99%" stays figurative direction, not a success-rate or p99 gate.

## Development notes

The first targeted retained run passed6cases. Two further new-source/retirement
controls and an owner-local PhantomData marker were added before final validation.
An initial unused mut in test scaffolding was removed. Compile-fail docs explicitly
verify setter and thread boundaries; no lint exemptions are introduced.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_read_prop_keyed -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_read_prop_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
