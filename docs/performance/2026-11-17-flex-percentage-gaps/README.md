# Flex percentage gap correctness, 2026-11-17

Branch/worktree: `codex/rendering-performance-research` /
`D:\oss\ggpui-worktrees\rendering-performance-research`.
See [contract and architecture decision](../../RETAINED_FLEX_PERCENTAGE_GAPS.md).
No benchmark speedup, p99, memory reduction or sustained display-visible frame
rate is claimed. The complete RFC goal remains active.

## Independent browser matrix

Chromium145.0.7632.6, headless Windows,1080 cases. Four directions × three wrap
modes × five main sizes (numeric/MinContent/MaxContent/Auto/Percent) × two cross
sizes × three box models × three gap combinations. Every case compares host
size and all three children's positions/sizes, tolerance0.02px. The host is
inline-flex in a300x200 outer, with explicit Start alignment and three30x20
nonshrinking children. Decorated models use padding3 and border2 on every side;
ContentBox and BorderBox are separate. Gaps are both10%, or one physical axis
7px and the other10%. Percent main size is50% of the outer.

**1008 browser matches and72 separately asserted normative deviations.** The
raw browser geometry is retained in TSV; JSON records the exact deviations and
their reasons. Wrapped Column/ColumnReverse MinContent containers have a
normative main extent equal to the largest item contribution (20px plus insets),
while Chromium keeps60px or74px plus insets and one column. The test first
asserts that raw browser discrepancy, then asserts the normative size and
three-column placement including WrapReverse. These72 do not count as browser
matches. This is finite conformance evidence, not universal CSS compatibility.

```powershell
node docs/performance/2026-11-17-flex-percentage-gaps/capture.mjs D:/Scoop/persist/nodejs/bin/node_modules/agent-browser/node_modules/playwright-core/index.mjs
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet percentage_gap -- --nocapture
```

The second argument is the locally installed Playwright module and may be
replaced with an equivalent installation. `browser-reference.html` can also be
opened directly. Capture uses a real Chromium layout independent of Taffy.

## Failures retained during development

`initial-run.txt` records the original648-case matrix failing168 cases, all
Column/ColumnReverse. `corrected-run.txt` is an **intermediate failing run** with
72 remaining normative/browser differences; its name does not mean passing.
Those logs predate the Auto/Percent expansion and are not1080-case evidence.

After separately encoding the72 normative cases, the648-case check passed.
Expanding to1080 exposed48 additional real failures: Row/RowReverse + Auto main
size + Wrap/WrapReverse + percentage main gap. Host width was correct, but the
third item overflowed because line membership still used the pre-resolution gap.
For the plain example the broken result was host90x20 with x=0/39/78;
the corrected result is90x40, third item at0/20. That expansion failure was
observed in tool output rather than saved as a new raw log. No failing matrix
combination was removed. Final tests include both fixes and normative handling.

## Integration controls

- Measured and complete independent parent policies assign the same84x64 border
  box/padding2 while cycling definiteness true/false on both axes. Independent
  assignment overrides a conflicting999px request. Row and Column, resolved
  and local frames, complete LayoutResult and CPU pixels match fresh full trees
  with numeric horizontal8/0 and vertical6/0 gaps.
- Normal typed LayoutStyle sources change Auto Row/RowReverse gap0→10%→0 in
  Wrap/WrapReverse.90x20→90x40→90x20 and complete CPU pixels match separately
  authored fixed90px/numeric-gap full controls. Restoring the final source value
  within one transaction retains revision and0 layout. Old packets work after
  owner destruction.
- Pure dry baseline queries keep selected size, percentage flags, hard bounds
  and mixed numeric gaps independent. Content-derived block size uses zero
  percentage gap; tight bounds establish definiteness. First/last baseline and
  absence of frame publication are asserted.
- The hello test shapes actual bundled Noto text. A120px-wide MaxContent Column
  and a definite250px Column use numeric0/25px reference gaps. Complete host and
  text LayoutResult, all CPU glyph pixels and old-frame lifetime match.
- Vendor tests cover root/nested auto and MaxContent block gap updates, and
  root/nested fit-content inline layout at300px/70px opportunities. Gap0→10%→0
  exercises style/cache invalidation. Both balanced-feature configurations build.
- Explicit WGPU test compares all pixels of three assigned-definiteness submits
  and twelve Auto Row/RowReverse Wrap/WrapReverse submits with fixed numeric-gap
  full controls. This was explicitly executed on NVIDIA GeForce GTX1650Ti,
  Vulkan, driver571.96; all15 submits matched. Ignored tests alone do not
  establish hardware correctness. These are offscreen, finite pixel checks.

## Final regression results

| Check | Actual result |
| --- | --- |
| retained full suite |416 passed |
| retained-text + retained-wgpu full suite |449 passed,25 ignored |
| explicit percentage-gap hardware case |1 passed,15 submit/readback comparisons |
| hello retained-wgpu, including real Noto paragraph |42 passed,2 ignored |
| vendored Taffy default / project-config library suite |223 /217 passed |
| framework library Clippy `-D warnings` |passed |
| vendor no-default / alloc+tree+Flex / all-features checks |passed |
| scoped framework/glyph/vendor-test rustfmt and git diff check |passed |

Suites overlap; their counts are not an independent aggregate. Standalone vendor
tests use the existing temporary low-memory test profile: debug=0,
incremental=false, codegen-units=256. Production build settings were not changed.
Retained-only pre-existing unused SpatialUpdate fields and dependency linker
messages remain; the Clippy statement covers the framework library scope.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_percentage_gaps -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings

$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_PROFILE_TEST_INCREMENTAL='false'
$env:CARGO_PROFILE_TEST_CODEGEN_UNITS='256'
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --locked --offline --quiet
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --lib --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --no-default-features --features alloc,taffy_tree,flexbox --locked --offline --quiet
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --all-features --locked --offline --quiet
```

## Cost and acceptance boundaries

The common Flex algorithm reuses existing definite flags and ordinary line
collectors. It adds scalar basis checks and at most one extra line collection
when an eligible normal finite automatic inline size resolves a changed gap.
No iterative size equation, public/persistent style field, GPU/packet record,
measurement callback or resource lifetime is added. Peak transient allocations
and wall-time have not been profiled. Ordinary full/source/CPU/GPU routes remain
the reference, with the existing transaction/cache/publication ownership rules.

Modern GPU heavy effects, CPU continuous full-frame smoothness, sustained60/120Hz,
visible input tails and the rest of the full RFC are not accepted by this stage.
