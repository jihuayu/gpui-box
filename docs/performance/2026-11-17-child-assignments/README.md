# Selected child size / definiteness — 2026-11-17

Windows x64, Rust1.98.1, locked/offline in the original research worktree.

```powershell
cargo +1.98.1 test -p gpui-box --features retained --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline --quiet
cargo +1.98.1 test -p gpui-box --features retained-text,retained-wgpu --locked --offline parent_layout::assigned_size_tests -- --include-ignored --nocapture
cargo +1.98.1 test -p gpui-box-example-hello --features retained-wgpu --locked --offline --quiet
cargo +1.98.1 clippy -p gpui-box --features retained-text,retained-wgpu --lib --no-deps --locked --offline -- -D warnings
git diff --check
```

## Results

| Check | Result |
| --- | --- |
| Framework retained complete suite | 394 passed |
| Framework text/WGPU complete suite | 427 passed, 21 ignored |
| Explicit selected-size suite, including actual WGPU | 7 passed |
| Hello retained-wgpu complete suite | 37 passed, 2 ignored |
| Framework library Clippy with warnings denied | Passed |
| Diff check | Passed |

The final suites all include the measured-parent regression added during this
stage. No Taffy source or feature configuration changes in this stage, so no new
standalone vendor test-suite result is claimed. Existing dependency linker/test-only dead-code
warnings are retained; only the framework library Clippy run denies warnings.

## Evidence

A regression first failed because the previous ChildLayoutInput round trip
discarded selected70×40 and returned no known dimensions. The new input preserves
both sizes and definiteness independently from hard intervals, available space
and percentage bases. Validation rejects nonfinite/negative/outside-interval
known sizes. Tight hard inputs remain definite; absent known flags normalize,
and width-only assignment replaces inline selected size while preserving block.

Normal typed parent snapshot updates false→true→false→true keep the child host
at80×60 while its 50% height leaf becomes10→30→10→30. One/two allocator levels and
resolved/local output compare complete host/leaf LayoutResults and all CPU pixels
with full ordinary Flex controls specifying the expected leaf sizes explicitly.
The reference is not a fixed-height Flex pretending its percentages are
indefinite. Conflicting child measure/placement requests cannot replace the
enforced input; nested parent input reads are asserted. Cache-warm repeated frames
and restored original source handles perform zero layout. Old frames survive
owner retirement.

An ordinary measured ParentMeasure/ParentLayout, without an independent phase,
uses the same selected inputs; final geometry, baseline dry query and CPU pixels
compare with the full control. A public complete dry query uses End alignment to
distinguish baseline53 from33 at the same selected80×60 host size, detecting the
10px vs30px percentage leaf. Independent incoming basis160 resolves a 50% host
width80, while selected width40 overrides the preferred percentage width. Dry
queries preserve the absence of a published frame.

Keyed allocation builders observe all selected fields with unbounded hard
intervals and soft width180. Conditional branches match full CPU pixels;
invalid selected NaN in source edits rolls back source/stats/previous revision
for both local/resolved publication. Bundled Noto glyph selected widths60/140/
90/60 rewrap actual glyphs under both measured and independent parent policies;
complete host/leaf/footer results, explicit query and all CPU glyph pixels compare
with ordinary full Flex and old packets survive all owner retirement.

Actual NVIDIA GeForce GTX1650Ti / Vulkan / NVIDIA driver571.96 runs selected
height definite false→true→false through three local submits and compares every
readback pixel with independently sized ordinary full Flex controls. Numerical
leaf heights are asserted separately. This is offscreen output correctness,
not display-visible timing or sustained presentation.

## Limits

Finite same-kernel reference controls do not establish broad browser conformance,
arbitrary client algorithm correctness or complete RenderBox. Suites overlap;
ignored hardware tests count only when explicitly run. Only framework library
Clippy is claimed. No wall-time/memory reduction/p99/sustained-frame result is
claimed. Temporary public input/placement payload grows; the kernel input and
cache formats, persistent node and GPU/FramePacket formats are unchanged.
