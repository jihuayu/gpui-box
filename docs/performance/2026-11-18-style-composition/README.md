# Typed Style IR composition evidence,2026-11-18

Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Base:`7a32767937fb7695f864fb528efd15e07354a813`.
See [contract and ADR](../../RETAINED_TYPED_STYLE_COMPOSITION.md).
Full RFC goal remains active. This stage adds capability and deterministic input
semantics; it makes no wall-time, memory, throughput or sustained FPS claim.

## Independent behavior controls

Eight regular framework cases cover:

- Ordered composition selects a complete final rule for each atomic property,
  then its last matching condition. Caller color replaces earlier default,
  hovered and pressed branches together. Winner diagnostics identify style,
  expanded declaration, condition case/default and Initial.
- Padding shorthand expands before ordering. Separate definitions can override
  one edge or all edges; shorthand/longhand duplicates inside one definition
  reject. Initial resets the selected field to its framework default, while an
  undeclared field retains authored base and removing overlays restores base.
- Conditions use conjunction and explicit logical environment width, with
  inclusive minimum/exclusive maximum checked at99/100/199/200. Unknown optional
  capabilities match neither requested false nor true. Environment is distinct
  from layout constraints and the paint viewport.
- All default/case values validate at definition creation, including unselected
  NaN, negative padding and invalid bounds. Selected cross-property combinations
  also validate: Grid plus a Flex-axis gap rejects. Name/expanded declaration/
  case/Grid-entry/shadow definition limits are checked.
- Owned Styles/StyleState/StyleEnvironment batches use normal retained routes:
  hovered fill uses P1 with zero layout; environment width changes actual layout;
  equal effective output keeps frame revision/no request. Caller replacement and
  overlay removal compare all CPU pixels to manually authored direct full trees.
- Invalid combined output and invalid environment preserve authored records,
  environment and frame. Document style-count/atom-count/case-count/value-work
  limits reject. Schema1 is explicitly rejected after the schema2 revision.
- Equal newly constructed definitions suppress visual work by value, exact batch
  replay remains idempotent and newer complete snapshot recovery recomputes
  authored rules/state/environment. Old frame geometry remains usable.
- Flex row-wrap to Grid transitions, mixed Calc gaps and opacity-only P2 changes
  compare every CPU pixel to direct numeric full trees. P2 changes run no layout;
  root sizes and existing source/property routing agree.

One regular real-font example uses bundled Noto paragraph/glyph resources and
five batches. Explicit style environment120/240 selects60px or
calc(-10px +75%)=125px width inside an independently180px root. Base padding3px,
caller left4px, hover opacity0.6 and caller removal/reinstallation are compared
with manually selected ordinary full trees. Complete LayoutResult/baselines,
actual paragraph wrap height and every CPU pixel agree. Initial glyph output
remains consumable after document/resources/text/raster owners are dropped.

Two explicit hardware cases repeat four width/color updates and five real-glyph
updates respectively. Each submit/readback compares **all pixels** to the direct
ordinary full reference. GPU controls use the same backend and resources; these
are finite offscreen correctness checks, not native display or frame-time tests.

## Actual final checks

| Check | Actual result |
| --- | --- |
| retained complete suite |510passed,1ignored manual benchmark |
| text+WGPU complete suite |543passed,40ignored |
| hello complete suite |55passed,9ignored |
| explicit style condition GPU |1passed,4complete-pixel submissions |
| explicit real-glyph style GPU |1passed,5complete-pixel submissions |
| framework library Clippy `-D warnings` |passed |
| scoped framework/text rustfmt and diff checks |passed |

Hardware:NVIDIA GeForce GTX1650Ti/Vulkan/driver571.96. Rust/Cargo1.98.1,
Windows MSVC, locked/offline debug verification. Eight regular core cases,
one real-font case and two explicit hardware cases were added. Suite counts
overlap; skipped hardware cases count as hardware evidence only when explicitly
run. No vendor algorithm/features changed, and no new vendor conformance run or
browser capture is claimed. Existing dependency linker messages and the
retained-only unused SpatialUpdate fields remain.

After the complete suites, the existing failure/limits case was extended with
receiver-side Grid-entry/shadow limits for unselected producer-accepted branches;
the style cases and explicit condition GPU control were rerun. Production code
did not change. Scoped rustfmt also reordered the two example test module
declarations; there was no runtime change from that formatting correction.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/style-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_style_conditions -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_style_ir_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

## Costs and remaining gates

StyleRef shares immutable normalized rules but shorthand expansion clones rule
values. Resolution allocates a winner map and copies base values. UiDocument
keeps authored and effective graphs, resolves all live nodes on every accepted
batch, validates both complete graphs and retains the existing candidate and
geometry verification costs. P0 output suppression is not zero planning work.
Limits bound structural counts/value sizes, not time or total bytes.

No tokens/theme/parts, text/font style cascade, logical inheritance, semantic
disabled, automatic event state, binding/handler/focus records, component macros,
portable ABI or automatic builder/query scheduler is provided. Native independent
presentation, modern desktop heavy effects, CPU continuous complete frames,
sustained60/120Hz and full input-to-presentation tails retain their own unmet
acceptance gates. The complete RFC goal is not closed by these tests.
