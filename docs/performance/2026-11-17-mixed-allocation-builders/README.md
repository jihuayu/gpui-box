# Mixed Flex allocation-builder validation — 2026-11-17

Windows x64 / Rust1.98.1 locked/offline, existing research worktree.
This stage expands a public dependency/input contract, not frame timing claims.

## Commands

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline mixed_allocation_tests -- --include-ignored --nocapture
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --locked --offline --lib
cargo +1.98.1 test --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --no-default-features --features alloc,taffy_tree,flexbox
cargo +1.98.1 check --manifest-path vendor/taffy/Cargo.toml --locked --offline --all-features
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
```

Adapter-dependent ordinary-suite ignored tests are not counted as passed.
Existing retained-only unused SpatialUpdate and Windows linker notices remain.

Final results:

| Check | Result |
| --- | --- |
| Framework, retained | 343 passed |
| Framework, retained-text + retained-wgpu | 376 passed, 13 ignored |
| Hello, retained-wgpu | 28 passed, 2 ignored |
| Explicit mixed-allocation suite, including actual WGPU | 7 passed |
| Taffy default library suite | 206 passed |
| Taffy project-feature library suite | 200 passed |
| Taffy no-default, alloc/Flex and all-feature checks | All passed |
| Framework library Clippy, warnings denied | Passed |
| git diff --check | Passed |

The focused seven-case run overlaps the framework suite; it is not an extra
seven distinct regressions. Its explicitly enabled GPU case is normally ignored
in the ordinary suite. All-targets Clippy was not rerun; the previous stage's
four existing test-only lints remain outside this stage's successful library
Clippy result.

## Public mixed chain

Ordinary Row width80/160 contains a fixed20 rail plus a growable full-size/layout
contained boundary. Its real assigned width60/140 and height80 are observable as
known dimensions with no fabricated hard bounds. An enforced custom allocator
passes loose width60/140 and unbounded block space to a content-sized inner host.
Nested allocation builders run once and construct keyed three-item wrapping
Flex output. At width60 content is60x30; at loose maximum140 it is120x10, retaining
the distinction between available interval and actual intrinsic used size.

Resolved and local complete CPU bytes/hits match a fresh ordinary forced-full
tree without custom layout delegates or builder callbacks. The reference
explicitly selects min(available60/140,intrinsic120) host width and uses ordinary
Flex for wrapping. Widths alternate four times, checking scope retirement and
old frames after UI drop. It is not a second runtime allocation algorithm.

An additional **48-case** finite matrix covers four Flex directions, LTR/RTL,
nowrap/wrap/wrap-reverse and plain/decorated boundaries, ancestor and boundary
aspect ratios, percentage padding, margins and last-baseline parent alignment.
Each builds oversized content under a contained ordinary Flex item; final
boundary/child LayoutResult and all CPU bytes equal a fresh forced-full ordinary
tree with identical styles and content. This validates builder composition;
it is not independent CSS/Chromium conformance evidence. Existing independent
browser matrices remain in the full regression suite.

Other checks cover a full-size-contained Loose/Loose ConstraintBox with5px
padding: outgoing intervals90x50 allow actual40x20 content; an unbounded external
contained root retains unknown input dimensions and zero empty-content used
size; a Column's known used block size0 remains percentage-indefinite, while
the incoming parent percentage basis is independently Some(0). InlineSize alone,
arbitrary measured parent, late containment removal, equal-number algorithm
replacement and ancestor-list insertion reject atomically, preserving source,
stats and prior frame. Hidden/OOF/Grid/Block proofs remain outside scope.

## Owned kernel lifecycle

Two vendor tests inspect actual final incoming input storage. Repeated cache
hits retain observations; ComputeSize probes with a different percentage basis
cannot overwrite them; private clones stay independent. Hidden ancestor layout
clears descendants, restoration repopulates, containment removal retires,
node deletion/generation reuse and clear release both records and membership.
Equal known width80/height40 with changed soft width40/70/40 preserves the full
input through exact invocation guards. The test uses the real custom child entry
with independent known/soft fields, not a root style assumption.

## Real text and GPU

Bundled Noto Sans CJK SC is shaped/rasterized by the existing native path. An
ordinary Row with rail20 allocates width60/140/90/60 to a Size-contained custom
parent; its independent full slot feeds a content-sized host built from keyed
paragraph+footer. Actual paragraph heights and footer positions reflow at the
final content width; root boundary height400 remains isolated. Every CPU byte
matches an independent ordinary forced-full Flex tree. Visible glyphs/hit and
old snapshot retention after UI/font/raster owners drop are asserted.

Explicit GPU: NVIDIA GeForce GTX1650Ti / Vulkan / driver571.96, four actual local
submissions at outer widths80/160/80/160. Readback pixels match the ordinary
forced-full reference in every frame. This proves offscreen output on that
adapter, not modern-GPU heavy effects, sustained60/120Hz, visible presentation,
CPU continuous complete frames or display-visible input tails.

## Costs and limitations

The new public fields enlarge observations. Two sparse owned-kernel maps and
invocation-wide exact cache mode increase retention/clone/key-validation costs;
ordinary size-contained trees may get fewer lossy CSS cache hits. Existing full
private candidates, preview layouts, style/proof copies and final checks remain.
No GPU payload or per-node NodeData format changes, no unsafe/workload-specific
shortcut and no measured performance gain. General ordinary content-feedback
edges without a cut, custom allocator child-style dependencies, complete
RenderBox and incremental scheduling/rollback remain open in the
[contract](../../RETAINED_MIXED_ALLOCATION_BUILDERS.md).
