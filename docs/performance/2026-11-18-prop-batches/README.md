# Component input batch verification,2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `357b962c1e64aa904ec421af57db0e82cfa69be6`.
See [API/rollback/cost contract](../../RETAINED_PROP_BATCHES.md).
Complete RFC remains active. No wall-time/FPS/memory or native smoothness claim.

## Controls

Seven regular core cases plus one ignored explicit GPU wrapper:

- Valid interval10/20→30/40 with an invalid100/40 temporary declaration:
  derived runs once with final30/40, ordinary sink/effect publishes final tuple.
  The earlier single-field30/20 stage rejects as expected. Restoring100→30
  inside the next batch makes zero derived evaluations and preserves exact frame.
- A failed derived tuple mixes existing replacements with a new String input.
  Handling the error keeps old inputs, retires leaked new handle and permits a
  subsequent valid batch with a distinct generation. Catching an internal type
  declaration error cannot publish preceding writes; finish returns InvalidPropBatch.
- Closure panic and explicit error roll back source allocation slots/maps without
  restoring generations. A later valid declaration produces exactly1live source
  and no failed write counters. Panic is deliberately caught by the test.
- Inputs and computed sources created earlier in the same outer draft respond
  to a later coherent batch; derived evaluates exactly2times total(initial+
  final replacement), and direct binding publishes25. Heterogeneous String input
  is declared/replaced within the same batch.
- Content producer declares2rows×3inputs in one batch before mounting new rows.
  Intervals10/30,20/45→50/90,40/65→70/100,25/60 deliberately cross previous upper
  bounds so sequential field updates would reject. Data/order/membership changes
  preserve key1's node/scope; mount counts2→2→3. Resolved/local initial sessions
  ×3models compare6CPU/GPU complete images to independent numeric full Flex.
  Six total derived evaluations verify once per affected live expression, with
  initial/new mounts counted. Old packets survive owner drop.
- Final invalid opacity rejects a successful prop batch and provisional new input;
  sources/stats/complete frame remain old. Foreign/retired scopes reject with no
  input adoption, even when preceded by otherwise valid pending declarations.
- Panic inside a dependent calculation rolls back batch-only allocations and
  inputs while preserving an earlier successful value20and String in the same
  draft. A subsequent batch adopts30; no leaked handle revives.

One new compile-fail doctest verifies PropBatch has no read_prop capability.
Existing two ReadProp setter/non-Send doctests remain.

Real Noto Content parent batches lower/upper/text/color for one mounted label.
Intervals10/110→200/340→400/480 change width100→140→80, long→short→long paragraph
and3colors. A paint computation depends on all4inputs and validates the interval;
it runs once per state, total3; mount stays1. Independent numeric full Flex uses
actual wrapped paragraph metrics; complete title LayoutResult/baselines and CPU/
GPU pixels agree,3hardware images. Old glyph frames survive owner retirement.

No source schema/kernel/GPU layout/renderer changes. Shared source settlement's
restored-root check also serves ordinary responsive input vectors, covered by
the full responsive/computed/producer regression suites.

## Results and limits

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness.
Explicit GPU: NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Observed result |
| --- | --- |
| Final complete retained | 629 passed,1ignored |
| Final complete text/WGPU | 663 passed,56ignored |
| Final complete hello | 71 passed,22ignored |
| Compile-fail docs | 3passed in both core configurations |
| Seven new core / real-Noto CPU | Included in suites above |
| Explicit keyed-input GPU | 1passed,6submissions/6complete images |
| Explicit real-Noto GPU | 1passed,3submissions/3complete images |
| Final library Clippy -D warnings | Passed |
| Complete retained-gpu performance harness | Passed |
| Scoped format/diff and288local links across5documents | Passed |
| Five unrelated WIP hashes | Unchanged |

All production and test behavior was fixed before final full suites/hardware
controls. Counts overlap; ignored tests count as hardware evidence only when
explicitly run. Existing retained-only SpatialUpdate unused-field and dependency
linker warnings remain without suppression. A local report-count helper initially
used Windows default GBK to read a UTF-8 test file; explicit UTF-8 fixes the
helper only, without affecting test runs or source behavior.

The batch removes repeated map-copy/derived-scan checkpoints within one input
declaration set. Final changed-root comparison and scratch allocations still
cost work; full graph scans, ordinary previews/candidates/geometry, dense/keyed
sorting and output ownership costs remain. No throughput or displayed latency
improvement is inferred from evaluation counts or finite offscreen images.
Modern GPU effects and continuous complete CPU output remain active work.
"99%" is figurative direction, not a numeric success-rate/p99 gate.

## Development notes

The first5core cases passed before adding the outer-frame/foreign/retired-scope
and derived-unwind controls. Both panic diagnostics are expected caught panics,
not failed regressions. No validation or lint restriction is suppressed.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_prop_batch_keyed -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_prop_batch_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
```
