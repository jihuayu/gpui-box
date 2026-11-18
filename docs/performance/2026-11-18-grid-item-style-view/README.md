# Public Grid item-style view evidence,2026-11-18

Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Base:`751efe4b69a756bc784e1f282ca6e75e66db68da`.
See [contract and ADR](../../RETAINED_GRID_ITEM_STYLE_VIEW.md).
Full RFC goal remains active. No new timing/memory/FPS gain is claimed.

## Normal framework behavior

- Test-only ExplicitCells consumes public column/row placement and justify-self
  in both independent slot allocation and measured physical placement. It
  intentionally supports only two equal explicit columns/two fixed rows; this
  client does not ship as a Grid solver. Compare against ordinary full Taffy
  Grid at120/160/100/120/120root widths while changing spans, negative lines,
  row position, fixed/auto width, alignment safety and hidden/restored state.
  Typed child-style source and atomic resolved/local edits publish complete
  results. Both leaf sizes/positions and all CPU pixels match full Grid. Idle
  equal writes keep revision and invoke no layout. Old frames survive owner drop.
- An allocation builder consumes a child's independently assigned slot; a later
  sibling row/justify-self change preserves numeric width but fails final style
  observation. Source, stats and published revision roll back. Existing full
  effective immutable style observations already cover the new fields.
- Legacy NamedLine/NamedSpan and numeric line0/span0 fail public conversion
  explicitly. The owned kernel view preserves names rather than discarding them.
  Existing child-style value loop now verifies numeric placements and all
  align-self/justify-self keyword and safety combinations across normal probes.
- Real bundled Noto paragraph uses50% of one/two-column area, positive/negative
  column starts, row1/2 and end/center justification.180/240/180/180widths produce
  45/120/90/45px text widths. Resolved/local outputs compare complete child
  LayoutResult (including baseline) and all CPU glyph pixels against ordinary
  Grid; paragraph's actual wrap height is independently asserted. Old packets
  remain usable after owner drop.
- Kernel regression reads numeric/signed/span/named placements and both self
  alignments, replaces live style, compares an old clone and retains owned view
  after both trees retire. Feature guards follow Style's actual Flex/Grid/Block
  fields; no new persistent style/node/frame/GPU payload is added.

## Actual checks

| Check | Actual result |
| --- | --- |
| retained complete suite |492passed,1ignored manual benchmark |
| text+WGPU complete suite |525passed,38ignored |
| hello complete suite |53passed,7ignored |
| explicit custom Grid item GPU |1passed,5complete-pixel submissions |
| explicit real Noto Grid policy GPU |1passed,8complete-pixel submissions |
| vendor default/affine+serde/project-affine lib |233/238/229passed |
| vendor alloc+tree+Grid/Flex/Block+affine checks |passed |
| vendor all-features check |passed |
| framework library Clippy `-D warnings`, scoped rustfmt/diff |passed |

GPU adapter:NVIDIA GeForce GTX1650Ti/Vulkan/driver571.96. Three regular
framework cases, one vendor case and one regular actual-glyph case were added;
the existing style-value loop was extended, plus two explicit hardware cases.
Counts overlap and skipped hardware cases count only when explicitly run.
The kernel view's conditional Copy derive preserves non-Grid builds; feature
checks and affine+serde suite were rerun after that refinement. Existing
retained-only unused SpatialUpdate fields/dependency linker messages remain.
No fresh timing, RSS or native visible acceptance is implied by these checks.

## Reproduction

Use task-local TEMP/TMP on D: when C: is full:

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/layout-item-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_custom_grid_item_metadata -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_grid_metadata_parent -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Initial link failed LNK1108 writing a C: temporary file before tests; rerun with
task-local D: TEMP/TMP. Initial two client tests required an incoming known width
even though independent allocation runs before resolving host style. Corrected
the client to use incoming Definite soft input and passed the actual width as
layout constraints, retaining a separate180px paint viewport. No engine protocol
or runtime algorithm was changed to make this fixture pass. Early1pass/2fail
is not the final result.

No fresh browser capture is claimed: this extends item input observability,
without changing normal Flex/Grid algorithms. Existing browser reference tests
continue in regular suites. The finite custom policy is not a complete alignment,
intrinsic/auto-placement/Grid conformance implementation. In particular it does
not establish custom margin/relative-position behavior as ordinary CSS Grid.
Broad conformance, names in public styles, full-axis/content feedback proofs,
automatic builder/query scheduling, RenderBox, incremental candidates and global
budgets remain open. Native visible presentation, modern desktop heavy effects,
CPU continuous complete frames,60/120Hz and input tails remain separate gates.
