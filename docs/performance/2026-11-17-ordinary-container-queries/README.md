# Ordinary uncontained query evidence, 2026-11-17

Worktree: `D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch: `codex/rendering-performance-research`.
See [contract and ADR](../../RETAINED_ORDINARY_CONTAINER_QUERIES.md).
Full RFC goal remains active. No timing, peak-memory or sustained-FPS gain is claimed.

## Finite controls

- Ordinary uncontained Column host with padding3/vertical gutter6 receives
 80/111/112/140/80px. Its content threshold100 sees68/99/100/128/68, independent
  of500px environment and180px paint viewport. Selected content height20/60
  drives host26/66 and following footer positions. All CPU pixels match fresh
  complete ordinary Flex controls, resolved/local both; idle has0 layout and
  0 changed styles. Old frames survive UI destruction.
- Two ordinary nested queries are registered consumer-first. Outer100→120→100
  selects inner50%→stretch→50%, producing inner50/120/50 and content20/60/20.
  Producer-before-consumer DAG stages select from the current ancestor result.
- A percentage root under Unbounded/MinContent/MaxContent without an anchor
  fails without publishing source/stats/revision. Numeric80px root under normal
  Unbounded remains supported. Unsafe wrap ancestor alternatives reject at
  registration. Existing builder anchor tests exercise the now-shared helper.
- Scope-owned ordinary query and inline allocation builder settle in one atomic
  environment session. Builder reads140 border-box while the rule selects from
 128 content. A later ordinary typed host source80 update automatically selects
  the narrow style and publishes80×26. Post-observation root Row change rejects
  and preserves source/stats/revision.
- Tight-width ConstraintBox and enforced independent custom width phases carry
  ordinary query hosts without containment.100/200/100 root input yields full
  or half host widths, and corresponding20/60 dependent content heights.
- A computed source sink back into the queried host rejects as a cycle even
  when it currently returns an equal host style/width. A direct internal
  standalone-draft regression changes an observed width after reading, or
  replaces the custom algorithm with a new equal-width snapshot. Final checks
  reject both and preserve live sources/stats/revision. This explicitly exercises
  the new standalone final guard rather than only keyed final checks.
- Actual bundled Noto text on ordinary padded/guttered hosts receives
 80/112/140/80px. Content query selects normal text width or50% text width;
  complete host/text LayoutResult and all CPU glyph pixels match fresh full
  ordinary Flex references for resolved/local output. Old glyph frames survive
  UI owner retirement. No character-width approximation is used.

These are same-kernel protocol/full-output controls, not new independent CSS
container-query browser cases. The framework explicitly proves a broader ordinary
layout contract; browser CSS queries use containment semantics.

## Regressions and implementation failures

The first full446-test run had444 passes and two failures: old rejection fixtures
removed fixed min=max limits under an otherwise safe Column chain. That transition
is now intentionally supported. Rejection fixtures were changed to actual unsafe
nonstretched auto width and aspect coupling, while new positive ordinary cases
cover the accepted transitions. The complete62-case responsive-focused suite and
seven new regular cases passed after those changes. The initial real-glyph
focused case also passed. These partial runs do not replace final full suites.

## Actual checks and reproduction

| Check | Actual result |
| --- | --- |
| retained complete suite |449 passed |
| retained-text + retained-wgpu complete suite |482 passed,31 ignored |
| explicit ordinary query WGPU case |1 passed,4 complete-pixel submissions |
| hello retained-wgpu, including actual Noto glyphs |47 passed,3 ignored |
| framework library Clippy `-D warnings` |passed |
| scoped framework/glyph rustfmt and git diff check |passed |

Explicit hardware used NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96.
The80/112/140/80px local submissions matched all pixels of independently built
ordinary full Flex references. This is finite offscreen correctness, not native
display-visible, modern heavy-effects or sustained-frame acceptance. The final
449/482 counts include all seven new regular cases, including the direct
standalone final-guard regression and automatic-source continuation assertions.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_ordinary_container_query -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Suites overlap and ignored hardware cases are evidence only when explicitly
executed. No vendored kernel/manifest or feature changes occur in this stage;
vendor suite counts are not claimed as new runs. Existing retained-only unused
SpatialUpdate warnings and dependency linker messages remain; Clippy scope is
framework library. Public signatures and persistent formats do not grow, but
proof/style copies, standalone observation vectors, custom dependency snapshots,
final checks and whole-tree candidate costs remain unprofiled. All broader
RenderBox/dependency-scheduling/Flex/native/sustained-frame gates in the ADR stay open.
