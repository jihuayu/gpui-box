# Custom parent layout validation — 2026-11-16

Windows, Rust 1.98.1, locked/offline. This stage adds framework capability;
there is no benchmark or performance improvement claim.

Commands executed from the retained research worktree:

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib hardware_custom_parent -- --ignored --nocapture
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --lib --quiet
cargo +1.98.1 test --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox,grid,block_layout,float_layout,content_size --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --no-default-features --features std,taffy_tree,flexbox --lib --quiet
cargo +1.98.1 check --locked --offline --manifest-path vendor/taffy/Cargo.toml --all-features --lib --quiet
```

Nine new regular framework regressions cover:

- Real nested wrap Flex at widths 90/50/130/90; content-driven host/ancestor
  heights and following siblings, physical offsets, CPU full-image equality
  against a fresh ordinary Flex reference, explicit hit tests and old frames.
- Snapshot pointer equality, paint-only skipping, changed child height,
  replaced delegate and removal restoring ordinary Flex layout.
- Nine invalid output cases, repeated failure without cached success,
  preservation of persistent Taffy geometry/cache and the old FramePacket.
- Final property/topology protocol validation; measured/OOF/ConstraintBox
  conflicts and valid coalesced switching back to ConstraintBox.
- Unified edit construction with an installed snapshot, reparenting, subtree
  retirement and old-frame node identity.
- Typed reactive snapshot binding inside `edit_frame`, preserving source,
  effect observation count and old pixels when final layout fails.
- Same child measured against 20/40/20 vertical percentage bases returns
  10/20/10 heights; ordinary lossy size cache is not accepted for this API.
- Published and queried baseline, intrinsic subtree measure, local/resolved
  CPU equality, hide/show recovery.
- Nested custom parents, root hard bounds, scroll overflow and property-only
  scrolling with updated hit testing.

One hello test uses bundled Noto Sans CJK SC, real shaping and glyph masks at
50/90/140/50 content widths. Custom parent height, first baseline and footer
geometry match the paragraph's measured lines. Every CPU pixel matches an
independent ordinary Flex reference, real glyph coverage is asserted, and the
old frame reproduces its pixels after dropping UI/font/raster owners.

One ignored-by-default hardware test was explicitly run: **1 passed**.
NVIDIA GeForce GTX 1650 Ti, discrete GPU, Vulkan, NVIDIA driver 571.96.
Three width/height updates compare every actual GPU pixel with a fresh
ordinary Flex full reference and assert a red content pixel. No visible
presentation or modern desktop GPU timing conclusion follows from this test.

Two vendor regressions cover final-layout cache reuse, hidden children,
delegate remove/clear ownership, invalid node and direct OOF rejection before
observer writes. Default Taffy: **195 passed**; project features: **189 passed**.
No-default, Flex-only and all-features library checks pass. The existing
`util/print.rs` ignored writeln Result warning remains in a check configuration.

Final framework results: retained **271 passed**; text/WGPU **304 passed,
3 ignored**; hello **18 passed, 1 ignored**; library Clippy **-D warnings passed**.
The custom hardware case is one of those default ignores and was run explicitly.
The no_std alloc + taffy_tree + flexbox library configuration also passes. Existing retained-only unused
SpatialUpdate fields and Windows macro linker informational warnings remain.

There are no new browser-reference cases or performance recordings. Dirty
custom layout uses an O(tree) Taffy clone, callbacks can allocate multiple plans,
and ComputeSize cache bypass adds work. These costs are deliberately recorded
instead of interpreting zero final-frame layout on paint-only updates as proof
of end-to-end tail latency or sustained smoothness.
