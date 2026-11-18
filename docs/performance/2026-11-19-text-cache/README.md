# Bounded owner text preparation cache, 2026-11-19

Base: d770d33b1d726afccfb23fdb3ee818c57b159c3c.
Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
[Decision and accounting](../../ADR_RETAINED_TEXT_PREPARATION_CACHE.md) ·
[Text API](../../RETAINED_TEXT_COMPONENTS.md).
Complete RFC remains active; no numerical success-rate/p99 gate.

## Correctness controls

Real Noto tests exercise complete key equality across independently allocated
equal text, font family/weight/slant/size/line-height/scale/explicit revision,
different content, actual font database epoch, failed preparation, true LRU
refresh and entry cardinality. Separate payload exact-fit/one-byte-too-small,
clear and zero-entry disable controls preserve caller-held resources/chunks.

Four ordinary Text components share one owner preparation and identical resource
identity. Sizes 16→20→16 reuse the prior cached resource; clear plus explicit
font revision prepares once then shares again; width-only rewrap uses that
resource. Each stage compares all LayoutResults/baselines and complete CPU/GPU
pixels with four independently prepared uncached ordinary full Flex labels at
1.25 scale. Keyed scopes/nodes remain stable and the old frame outlives UI/cache.
Existing automatic text inheritance, custom adapter rejection and actual GPUI
CPU image-install/click tests also run in the full suite.

## Release A/B

`text-preparation-perf` compares the same adapter with cache disabled against
default cache. Each side has its own real Noto TextSystem; both use the existing
underlying GPUI caches/glyph rasterizer. It does not disable every GPUI cache.
Alternating 16/20 logical sizes, 32 retained samples after two warmup rounds,
alternating execution order. Timing includes preparation, key lookup, resource
replacement/release or ordinary source/Props/computed/Flex/full local-frame
adoption for the UI case. Correctness checks and resolving packets occur outside
timing. It excludes CPU/GPU raster, submit/present, OS input and display latency.

Cases: 32 repeated labels; 300 labels/12 distinct; 300 all distinct (working set
exceeds default entry bound); ordinary complete UI updates for 8/64 repeated
Text components. Resource/payload counts are diagnostics, not measured resident
memory. Default policies are not tuned per scene. Final measurements use Ryzen 7
4800H / Windows MSVC / Rust-Cargo 1.98.1 / locked-offline standard release. Two
separate invocations of the final binary ran after all cargo/rustc/link/tests had
finished. Binary SHA256:
`1ed57afa73da6a22da7e356e38346db24428581b41375b6f84775771ac62e5b8`.

| Work | Uncached p50 | Cached p50 |
| --- | --- | --- |
| Prepare 32 labels, 1 distinct | 320.2–334.1 µs | 6.5–6.6 µs |
| Prepare 300 labels, 12 distinct | 2514.3–2719.8 µs | 79.1–91.8 µs |
| Prepare 300 labels, 300 distinct | 2824.0–3100.4 µs | 2998.6–3359.5 µs |
| Ordinary UI/Flex update, 8 repeated Text | 247.4–265.6 µs | 63.6–69.3 µs |
| Ordinary UI/Flex update, 64 repeated Text | 1570.7–1797.5 µs | 663.7–733.0 µs |

The capacity-miss case is **6.2–8.4% slower** at p50, while performing the same
10,200 actual preparations per side. LRU/lookup/accounting/retention are real
costs, not removed by warm underlying GPUI caches. It retains 128 entries and
1,338,072 accounted payload bytes. No auto-tuning or scene-specific bypass was
added; hosts can explicitly disable storage when its measured working set does
not benefit. Repeated 300/12 prepares 24 resources (two sizes) vs 10,200; 64-Text
UI prepares two resources vs 2,176 over the initial/warm/measured sequence.
That UI still spends about 0.66–0.73ms on normal owner/source/Flex publication.

Sample p95 ranges: 64-Text UI 1817.9–2321.3→880.4–1014.7µs; 300-all-distinct
preparation 3471.1–3949.7→3728.5–4153.2µs. Only 32 retained observations per run,
so these describe these microbenchmarks and do not establish real user tails.
Raw CSV and summary JSON are [repeat 1](summary-repeat1.json) /
[repeat 2](summary-repeat2.json), with [raw 1](raw-repeat1.csv) /
[raw 2](raw-repeat2.csv). These are same-runtime cache-disabled controls, not a
direct whole-app upstream GPUI comparison or displayed FPS result.

## Final correctness / build verification

Windows MSVC, Rust/Cargo 1.98.1, locked/offline debug correctness. Logs under
`target/text-cache-validation/`.

| Check | Result |
| --- | --- |
| Text plus WGPU full suite | 835 passed, 69 ignored; 16 compiler doctests |
| Hello full suite | 103 passed, 41 ignored |
| Explicit repeated Text GPU | 1 passed; 5 complete images at 1.25 scale |
| Library and text-preparation harness Clippy `-D warnings` | Passed |
| CPU desktop without WGPU / retained-gpu harness | Passed |
| Scoped format / diff / links / inherited WIP | Passed; 309 docs, 1,176 links, 5 hashes |

GPU: NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96. Ignored tests do not
count as actual GPU execution. No retained-only rerun: modified production is
entirely behind retained-text; all affected text/host/harness paths are checked.
The original pure retained 800/1 evidence remains the preceding stage's result.

```powershell
cargo +1.98.1 build -p gpui-box-rendering-perf --features retained-text --bin text-preparation-perf --release --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline hardware_repeated_text_cache -- --ignored --nocapture
```

Run the built release binary directly after compiler/link activity has stopped;
its arguments are output directory and retained sample count. Repeat in separate
output directories. Full RFC, budgeted preparation and independent presentation stay
open; no sustained 60/120Hz or input-to-display claim follows from this stage.
