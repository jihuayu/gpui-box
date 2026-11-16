# Atomic LocalFrame edit validation — 2026-11-16

Windows / Rust 1.98.1 / locked offline. No performance benchmark in this stage.

```powershell
cargo +1.98.1 test --locked --offline -p gpui-box --features retained --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib --quiet
cargo +1.98.1 test --locked --offline -p gpui-box-example-hello --features retained-wgpu --quiet
cargo +1.98.1 clippy --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib -- -D warnings
cargo +1.98.1 test --locked --offline -p gpui-box --features retained-text,retained-wgpu --lib hardware_atomic_local -- --ignored --nocapture
```

Results: retained library **279 passed**; text/WGPU library **312 passed,
4 ignored**; hello **19 passed, 1 ignored**; Clippy with `-D warnings` **passed**.
The ignored hardware case below was run explicitly and passed separately;
ignored cases are not included in the normal-suite pass counts.

Eight new regular framework cases cover:

- Repeated atomic typed-source translation + material changes: exact local
  predecessor and topology reuse, one local/material change, untouched resolved
  cache and unchanged resolved owner baseline. The latest-frame queue coalesces
  three intermediate snapshots without resolving them. Explicit compatibility
  pixels match a resolved owner, and a subsequent idle edit reuses the snapshot.
- Inline builder widths 80/80/140/140/80: content-derived host height, following
  footer placement/hit, keyed state identity and effect run/cleanup order, old
  snapshot pixels after owner destruction. Full CPU pixels equal independent
  ordinary forced-full trees.
- Sparse and structural geometry-overflow rejection: no source/effect/stat or
  owner local snapshot change, no compatibility-cache materialization, retired
  provisional scope/source/node IDs, successful later edits and usable old hits.
- Changed final builder input: old geometry and exact local snapshot survive.
- Both atomic output APIs reject a translated own clip overflowing to infinity
  even when the inherited viewport intersection would otherwise hide it.
- Size-contained full-axis builder plus real custom ParentLayout: hard slot
  assignment, measured child geometry, typed snapshot binding and complete
  source/layout/local-frame rollback on a later algorithm error.
- Same-intrinsic-size image replacements with group opacity, clip and scrolling
  remain sparse; complete CPU output equals a fresh ordinary full reference.
- Independent viewport resize with zero layout, invalid viewport rollback,
  hidden/show layout recovery and independent old-frame revision/hit semantics.

One new hello case shapes bundled Noto Sans CJK SC and rasterizes actual glyphs.
At 50/90/140/50 content widths an atomic inline builder publishes the paragraph,
its real height/baseline dependencies and following footer. A second atomic
local edit changes only text material and host translation: one material and
one placement change, no complete local publication. Every cyan glyph pixel
matches an independent full tree; real coverage is asserted. An old snapshot
reproduces its original pixels after UI/font/raster owner destruction.

One default-ignore hardware case was explicitly run: **1 passed**.
Adapter: **NVIDIA GeForce GTX 1650 Ti / Vulkan / driver 571.96**.
Five produced local changes include one deliberately skipped submission. The
four consumed outputs match every pixel of a fresh ordinary full reference;
consecutive supported updates do not materialize resolved snapshots, the
skipped predecessor triggers full resync, then local incremental use resumes.
Each consumed frame has the expected hit geometry. GPU completion is not visible
presentation and this is not a modern GPU or frame-budget qualification.

Taffy source and GPU formats are unchanged. Existing retained-only unused
SpatialUpdate fields and Windows macro linker informational warnings remain.
Atomic owner copies, per-builder previews and O(N) final geometry validation
remain explicitly recorded costs, not performance improvements.
