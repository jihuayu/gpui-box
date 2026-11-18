# Overflow viewport verification, 2026-11-18

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
Base: `e2ce04b1c11290a0649232f997c67b83a2344253`.
See [contract and costs](../../RETAINED_OVERFLOW_VIEWPORTS.md).
Full RFC remains active; this stage establishes capability/correctness, without
wall-time, native input/presentation p99, peak memory or FPS improvement claim.

## Controls

- Clip/Hidden/Scroll axis combinations keep Visible axes unrestricted, preserve
  host border/shadow/hits and clip descendant hits. Numeric expected padding
  rectangles include border, padding, explicit clip, gutters and RTL changes.
  Nine mixed-axis cases also compare every CPU/GPU pixel against explicit
  numeric child clips, including an unrestricted x or y axis.
- Resize propagates a changed viewport through cached child geometry with shared
  commands and zero new recording; old snapshots keep old pixels/hits.
- Four owner policies (Full/Selective/SelectiveReextract/SelectiveCopy), each
  with six states: initial, scroll, sparse scroll+translation, resize, remove
  overflow and device-pixel projection. Every CPU pixel matches a numeric
  independent tree using Visible overflow and only explicit child clipping.
  The hardware wrapper submits24images and compares all pixels with the same
  independent full reference; it does not reuse the automatic viewport helper.
- A70-level ancestry with an owned image crosses deep GPU anchors. Four scroll
  states compare complete GPU images, including the host's outer shadow, with a
  Visible tree using explicit leaf translation/clip. Geometry/hits run regularly;
  CPU Exact shadow raster remains unsupported and is not inferred from this test.
- Real Noto own-node glyphs exceed a fixed viewport. Five states cover motion,
  resize/wrap, disabling overflow and snapped output, including isolated opacity
  and border. Independent reference splits host border and explicit-clipped text
  child; complete CPU and GPU pixels match. It never enables automatic overflow.
- Existing nested wheel, actual GPUI ScrollEvent and real-Noto wheel fixtures no
  longer author source viewport clips. Their independent reference clips remain.
- Standalone rustc harness compiles the actual backend spatial_index.rs and its
  nine checked-in tests, with a matching plain SpatialNode stub. It covers
  indexed/full validation, ancestry, rejection rollback, inverse memberships and
  new independent numeric x/y overflow assertions. This is not a full vendor
  Cargo test or actual storage-layout test; production ABI/shader compilation and
  hardware controls separately cover those layers.

## Observed results

Paired Rust/Cargo1.98.1, Windows MSVC, locked/offline debug correctness checks.
Explicit hardware used NVIDIA GeForce GTX1650Ti, Vulkan, driver571.96.

| Check | Result |
| --- | --- |
| Complete retained suite | 566 passed, 1 ignored |
| Complete retained-text/retained-wgpu suite | 600 passed, 46 ignored |
| Complete hello suite | 61 passed, 13 ignored |
| Final overflow_tests filter (includes existing custom-parent cases) | 8 passed, 4 ignored |
| Explicit framework hardware_ filter | 46 passed, including all3new overflow controls |
| Four-policy viewport GPU control | 24 submissions/24 complete-image comparisons, included above |
| Mixed-axis GPU control | 9 submissions/9 complete-image comparisons, included above |
| Deep image/shadow GPU control | 4 submissions/4 complete-image comparisons, included above |
| Own real-Noto CPU/GPU | CPU1passed; GPU1passed, 5 submissions/5 complete-image comparisons |
| Existing wheel nested-chain/real-Noto GPU without manual source clips | Each1passed, 4 submissions/4 complete-image comparisons |
| Actual backend index source via saved rustc harness | 9 passed |
| Final framework library Clippy -D warnings | Passed |
| Complete performance harness check, retained-gpu | Passed |
| Scoped Rust formatting/diff, 272 local documentation links | Passed |
| Five unrelated WIP file hashes | Unchanged |

Counts overlap. Complete retained/hello preceded the final Option::and lint-only
rewrite. Complete text/WGPU includes it but precedes the last test-only mixed-axis
addition; final focused eight cases and46explicit hardware cases cover that
addition and the final shader. Shader follow-up preserves unrestricted-axis
width/height exactly, avoiding unrelated f32 reconstruction. The broadly selected
hardware_ filter also executed the old identical-output consumer cost fixture;
its debug timings were not retained and are not performance evidence here.
Existing retained-only SpatialUpdate warning and dependency linker messages
remain. No lint suppression or Taffy algorithm change was added.

## Implementation history and remaining scope

Initial test calls used nonexistent Transaction::layout; corrected to existing
layout_style. WGPU validation's spatial module name collided with its sibling;
the shared validator is now fully qualified. The Noto fixture passed a
WindowTextSystem instead of its cloned shared TextSystem; corrected the fixture.
These were compile-time errors, without relaxing behavior or lint rules.
Final library Clippy requested Option::and instead of an unnecessary lazy
closure; corrected directly. Checking the existing full performance harness also
exposed older Taffy Dimension/context and responsive-direction migrations; the
few call sites now use current public types and the whole harness checks again.
No historical performance run is reclassified as a fresh measurement.

The GPU node gains16bytes (96→112), not a demonstrated performance win. Explicit
property-map semantics, whole-scene candidate/validation costs, worker scheduling,
native adoption and CPU complete-frame fluency remain explicit in the contract.
Rounded clips, CSS axis coercion/overflow-clip-margin, own-leaf scroll-origin
extensions, focus reveal/inertia and independently presented scroll remain open.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_overflow_viewports -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_deep_overflow -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_mixed_overflow -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_overflow_own_noto -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
cargo +1.98.1 check -p gpui-box-rendering-perf --features retained-gpu --locked --offline --quiet
rustc +1.98.1 --edition 2024 --test docs/performance/2026-11-18-overflow-viewports/spatial_index_harness.rs -o target/overflow-spatial-index-tests.exe
./target/overflow-spatial-index-tests.exe
```
