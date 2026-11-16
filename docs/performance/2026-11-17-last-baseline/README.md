# First/last baseline validation — 2026-11-17

Windows x64, Rust1.98.1 locked/offline, existing research worktree. This is
framework capability and correctness work, without new timing measurements or
continuous frame/presentation qualification.

## Independent browser oracle

```powershell
node docs/performance/2026-11-17-last-baseline/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
```

Capture uses installed Playwright-core and HeadlessChrome145.0.7632.6. The fixture
uses zero-size inline-block markers in real browser line boxes. It reads their
first/last baseline offsets rather than guessing font metrics. Four decorated or
undecorated items have differing line heights, line counts and fixed box heights;
negative margins and actual overflowing content are included.

The 1,440 cases combine four directions, LTR/RTL, nowrap/wrap/wrap-reverse,
first/last/mixed/single groups, auto top/bottom/both/left/right/horizontal margins,
auto/positive/overflow container height and two decoration/size sets. Per-case
root size and every child position/size are saved in browser-reference.tsv;
browser-reference.json defines the schema and engine version. An additional
outer Flex container and zero-size sibling probe record the tested container's
exported first and last baseline in container-baselines.tsv. The native test
constructs public LayoutStyle and explicit baseline metrics from the markers;
it compares all geometry and both exports at 0.02px tolerance. This is a finite
conformance matrix, not broad Flex/CSS acceptance.

The initial 720-case comparison found four existing negative auto-margin overflow
errors. After the normal logical-end overflow rule was fixed, geometry matched.
Outer baseline probes then exposed shared-group export choices, especially mixed
wrap-reverse first/last groups. Expanding to 1,440 cases found 24 RTL wrapping
column fallback errors when a wider sibling makes line cross-size larger than
an item. The kernel now retains inline reversal for wrapping columns as well.
There are no expected-error exclusions or browser-dependent production paths in
this matrix. Final checks below are authoritative for the saved final fixture.

## Real source/frame/resource paths

Normal reactive measure sources change only a last baseline at fixed first
baseline and fixed size, with four updates including equality. Resolved/local
atomic edits both match independently Start+translation positioned ordinary full
trees in every CPU pixel, follow hit geometry, reject invalid source metrics,
retain old revision/stats on failure, and preserve old-frame pixels after owner
drop. Paint-only origins use existing commands and layout publication.

Additional regressions check last-only propagation through cached ancestors,
isolated measure queries, hidden restoration/containment suppression, legacy
single-metric leaf behavior, nested ConstraintBox/Block inset propagation and
custom-parent measure/output transport. Invalid custom last baselines reject
final atomic publication without replacing prior output. LastBaseline with
Safe/Unsafe modifiers rejects as InvalidStyle.

Real bundled Noto Sans CJK SC paragraphs use widths60/140/90/60, with different
actual wrapped heights. The last baseline follows the actual final-width layout;
the peer's final line aligns while its first baseline can differ. CPU complete
pixels match independently positioned ordinary full trees. Old frames retain
actual glyph pixels after UI/font/shaping/raster owners drop. This is separate
from the browser's synthetic line markers.

One default-ignored WGPU regression explicitly passed with four actual LocalFrame
submissions and independent ordinary full position references on **NVIDIA GeForce
GTX 1650 Ti / Vulkan / NVIDIA driver571.96**, Windows x64. All complete output
pixels matched. GPU readback correctness is not screen-visible latency or modern
desktop GPU heavy-effects performance.

## Reproduce native checks

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib baseline_tests -- --include-ignored --nocapture
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --features serde,parse --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

| Check | Actual result |
| --- | --- |
| Framework retained | 330 passed |
| Framework retained-text + retained-wgpu | 363 passed, 11 ignored |
| Focused baselines, explicitly including hardware | 12 passed |
| Hello including real Noto paragraphs | 26 passed, 2 ignored |
| Clippy text/WGPU library | Passed with `-D warnings` |
| Owned Taffy default library | 204 passed |
| Owned Taffy project feature set | 198 passed |
| Owned Taffy with serde + parse | 220 passed |
| Owned Taffy no-default / alloc+tree+flexbox / all-features | All three checks passed |

Ordinary default-ignored hardware cases are not counted as passes unless
explicitly run. The final browser fixture's **1,440 geometry and paired container
baseline comparisons all passed**, with no expected-error exclusions.

## Costs and limits

See [public contract and ADR](../../RETAINED_LAST_BASELINE.md): source API migration,
extra result/measurement fields, transient FlexItem float, bounded linear sharing
group scans, singleton baseline layout and paragraph final-width memo lookup.
No new renderer record or per-frame baseline-owned heap collection is added.
Cache misses and existing full candidates/preview/rollback still cost work; no
performance gain is claimed. Vertical writing, baseline content distribution,
broad OOF/scroll/Grid/Block/rich-text semantics, full RenderBox and full RFC
performance/lifecycle acceptance remain open.
