# Owned primitive UI input evidence,2026-11-18

Worktree:`D:\oss\ggpui-worktrees\rendering-performance-research`.
Branch:`codex/rendering-performance-research`.
Base:`21f404071804210115d901ca6837a8cd957758f9`.
See [protocol and ADR](../../RETAINED_OWNED_UI_INPUT.md).
Full RFC goal remains active. No new wall-time/peak/RSS/FPS claim is made.

## Behavior controls

Ten regular framework cases cover:

- Send+Sync compile checks for snapshots, batches and publications; a real worker
  constructs a snapshot, another owner accepts it and a worker consumes its
  publication after owner retirement, including external hit identity.
- Color property batch uses ordinary InstancePatch with zero layout/chunk
  recording. Net restored properties advance source revision/sequence while
  retaining frame revision/no request_frame. Exact replay does not reexecute or
  adopt different supplied host input; same-sequence changes reject.
- Mixed Grid/Flex creation/reparent/child order/layout-order against fresh
  complete snapshot output; surviving internal render IDs stay stable. Rescue a
  child out of a removed host's final graph; old frame hits still refer to old
  external identity.
- Cycles/multi-parent/duplicate/unreachable lists, measured containers, invalid
  final opacity and missing resource all reject without source/frame changes.
  Invalid viewport fails after staged edits; retrying the same sequence succeeds.
- Removed-slot ABA rejects; later greater generation works. Newer complete
  recovery preserves tombstones, rejects regression/resurrection and stale
  deltas. Last accepted replay is bounded to one payload and not snapshot-exported.
- Immutable image table IDs, same-value bind, conflicting bind and wrong table
  reject. Image→Fixed→Image→Box uses the normal intrinsic/resource lifecycle;
  retained-vs-fresh complete CPU pixels agree.
- Schema/runtime/epoch/base/sequence/revision and nodes/depth/generation slots/
  child-list/operations/shadows/Grid-entry limits reject before adoption.
- Nested creation under a preexisting child resets the temporary root list,
  final graph remains valid. Successful create+cancel retains an external
  tombstone without a visual revision; old generation cannot be recovered live.
- Fixed measurement→container+new child→Fixed transition joins one final-state
  transaction; valid intermediate writes coalesce. NaN followed by a valid
  restore still rejects before adoption, preserving exact replay equality.
- Mixed clip/scroll/translation/isolated opacity/decorated quad/image compares all
  CPU pixels against an independently authored ordinary forced-full tree. Old
  publications retain geometry/resource/ID mappings after owner/table drop.

Explicit hardware repeats the mixed scene including a decorative shadow over
four property updates and compares every pixel to the direct complete path.
CPU Exact does not support shadows: its regular case has none, and the hardware
case's extra CPU check explicitly uses OmitDecorativeShadows on both controls.
This does not upgrade CPU shadow fidelity or claim CPU/GPU AA equivalence.

One regular real-font case plus one explicit hardware case use bundled Noto in
an ordinary1fr/2fr Grid, padded text leaf and eight source batches. Resize changes
wrap width, one update changes text color, then Text→Image→Fixed→Box→Text checks
old intrinsic-owner clearing. Complete LayoutResult/baselines and all CPU pixels
match a direct ordinary full tree. GPU consumes the local publication across
eight submissions, including blank/fixed/resource transitions; all pixels match
the direct reference. The initial glyph packet remains usable after document,
table, text system and raster owner retirement.

## Actual final checks

| Check | Actual result |
| --- | --- |
| retained complete suite |502passed,1ignored manual benchmark |
| text+WGPU complete suite |535passed,39ignored |
| hello complete suite |54passed,8ignored |
| explicit mixed UI input GPU |1passed,4complete-pixel submissions |
| explicit real text/resource transition GPU |1passed,8complete-pixel submissions |
| framework library Clippy `-D warnings` |passed |
| scoped framework/text rustfmt and diff checks |passed |

Hardware:NVIDIA GeForce GTX1650Ti/Vulkan/driver571.96. Ten regular framework
tests, one real-font example test and two explicit hardware cases were added.
The final source/operation/child/Grid limit refinements are covered by repeated
complete suites and both explicit hardware reruns. Counts overlap; the ignored
benchmark is distinct from hardware skips. There is no new vendor algorithm or
feature change and no corresponding fresh vendor/browser run is claimed.

## Reproduction

```powershell
$env:TEMP='D:/oss/ggpui-worktrees/rendering-performance-research/target/ui-ir-temp'
$env:TMP=$env:TEMP
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet hardware_ui_ir_matches -- --ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet hardware_owned_ui_ir_real -- --ignored --nocapture
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Initial test compile tried private mapping access, then incorrect PaintItem
field names; tests now inspect public immutable frame identity and fill_color.
Initial mixed CPU case requested Exact with shadows, producing the existing
UnsupportedShadows error; fixtures now explicitly distinguish regular supported
CPU content from hardware shadow/declared-omission controls. Earlier7pass/1fail
is not final acceptance. Clippy's internal eight-argument stage helper was
factored to pair host inputs; no broad warning suppression/unsafe added.

Final review tightened per-operation numeric validation, matching ordinary
transactions: an overwritten NaN must not survive in the retained last-batch
payload, because NaN is not equal to itself and would break exact replay.
The regression now rejects NaN→valid restore and separately accepts valid
intermediate→restored no-op writes. Complete suites and hardware controls were
rerun after this change; final topology still uses complete final child lists.

Final limits include child lists, authored Grid entries and generation history;
these refinements require the final full-suite and explicit GPU reruns recorded
with this stage. Existing dependency linker messages/retained-only unused
SpatialUpdate fields remain. Suites overlap and ignored hardware cases count
only when explicitly run. No kernel algorithm/features changed, so no fresh
vendor conformance suite or browser capture is claimed.

The input model is experimental Rust data, not serialized language-neutral ABI
or full semantic UI IR. Logical scope/binding/handler/semantics/focus, StyleRef/
tokens/themes, portable text/font/resource import, App Store/Command and
component macros remain missing. Full source graph/candidate/geometry work,
resource-table byte budget/eviction and automatic builder/query scheduling remain
open. Native visible independent presentation, modern desktop effects, CPU
continuous complete-frame smoothness,60/120Hz and input tails retain unmet gates.
